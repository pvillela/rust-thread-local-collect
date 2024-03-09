//! This module supports the collection and aggregation of the values from a designated thread-local variable
//! across threads. The following features and constraints apply ...
//! - The designated thread-local variable may be defined and used in the thread responsible for
//! collection/aggregation.
//! - The linked thread-local variables hold a [`Sender`] that sends values to be aggregated into the
//! [Control] object's accumulated value.
//! - The [`Control`] object provides functions to receive thread-local values on a background thread,
//! stop receiving on a background thread, drain the [`Receiver`], and retrieve the accumulated value.
//! - After all participating threads other than the thread responsible for collection/aggregation have
//! stopped sending values, a call to [`Control::receive_tls`] followed by a call to one of the accumulated
//! value retrieval functions will result in the final aggregated value.
//!
//! See also [Core Concepts](super#core-concepts).
//!
//! ## Usage pattern
//!
//! Here's an outline of how this little framework can be used:
//!
//! ```rust
//! use std::{
//!     ops::Deref,
//!     thread::{self, ThreadId},
//! };
//! use thread_local_collect::channeled::{Control, Holder, HolderLocalKey};
//!
//! // Define your data type, e.g.:
//! type Data = i32;
//!
//! // Define your accumulated value type.
//! type AccValue = i32;
//!
//! // Define your thread-local:
//! thread_local! {
//!     static MY_TL: Holder<Data> = Holder::new();
//! }
//!
//! // Define your accumulation operation.
//! fn op(data: Data, acc: &mut AccValue, _: &ThreadId) {
//!     *acc += data;
//! }
//!
//! // Create a function to send the thread-local value:
//! fn send_tl_data(value: Data, control: &Control<Data, AccValue>) {
//!     MY_TL.ensure_initialized(control);
//!     MY_TL.send_data(value);
//! }
//!
//! fn main() {
//!     let control = Control::new(0, op);
//!
//!     send_tl_data(1, &control);
//!
//!     thread::scope(|s| {
//!         let h = s.spawn(|| {
//!             send_tl_data(10, &control);
//!         });
//!         h.join().unwrap();
//!     });
//!
//!     {
//!         // Drain channel.
//!         control.receive_tls();
//!
//!         // Different ways to print the accumulated value
//!
//!         let acc = control.acc();
//!         println!("accumulated={}", acc.deref());
//!         drop(acc);
//!
//!         control.with_acc(|acc| println!("accumulated={}", acc));
//!
//!         let acc = control.clone_acc();
//!         println!("accumulated={}", acc);
//!
//!         let acc = control.take_acc(0);
//!         println!("accumulated={}", acc);
//!     }
//! }
//! ````

use std::{
    cell::RefCell,
    error::Error,
    fmt::Display,
    mem::replace,
    ops::Deref,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex, MutexGuard,
    },
    thread::{self, LocalKey, ThreadId},
};

/// Data structure transmitted on channel.
enum ChannelItem<T> {
    StopReceiving,
    Payload(ThreadId, T),
}

/// Status of background thread receiving on channel. from thread-locals.
enum ReceiveStatus {
    Stopped,
    CycleCompleted,
}

/// Modes of receiving thread-local values on channel.
enum ReceiveMode {
    Drain,
    Background,
}

/// Indicates attempt to have multiple concurrent background receiving threads.
#[derive(Debug)]
pub struct MultipleThreadError;

impl Display for MultipleThreadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{MultipleThreadError:?}: Illegal call to start_receiving_tls as active background thread already exists.")
    }
}

impl Error for MultipleThreadError {}

/// State of [`Control`].
#[derive(Debug)]
struct ChanneledState<T, U> {
    acc: U,
    sender: Sender<ChannelItem<T>>,
    receiver: Receiver<ChannelItem<T>>,
    bkgd_recv_exists: bool,
}

impl<T, U> ChanneledState<T, U> {
    fn new(acc: U) -> Self {
        let (sender, receiver) = channel();
        Self {
            acc,
            sender,
            receiver,
            bkgd_recv_exists: false,
        }
    }

    fn acc(&self) -> &U {
        &self.acc
    }

    fn acc_mut(&mut self) -> &mut U {
        &mut self.acc
    }

    fn receive_tls(
        &mut self,
        mode: ReceiveMode,
        op: &(dyn Fn(T, &mut U, &ThreadId) + Send + Sync),
    ) -> ReceiveStatus {
        while let Ok(payload) = self.receiver.try_recv() {
            match payload {
                ChannelItem::Payload(tid, data) => op(data, &mut self.acc, &tid),
                ChannelItem::StopReceiving => match mode {
                    ReceiveMode::Background => return ReceiveStatus::Stopped,
                    ReceiveMode::Drain => continue,
                },
            }
        }
        ReceiveStatus::CycleCompleted
    }
}

/// Guard object of a [`Control`]'s `acc` field.
#[derive(Debug)]
pub struct AccGuard<'a, T, U>(MutexGuard<'a, ChanneledState<T, U>>);

impl<'a, T, U> Deref for AccGuard<'a, T, U> {
    type Target = U;

    fn deref(&self) -> &Self::Target {
        self.0.acc()
    }
}

/// Controls the collection and accumulation of thread-local variables linked to this object.
/// Such thread-locals must be of type [`Holder<T>`].
pub struct Control<T, U> {
    /// Keeps track of registered threads and accumulated value.
    state: Arc<Mutex<ChanneledState<T, U>>>,
    /// Binary operation that combines data from thread-locals with accumulated value.
    #[allow(clippy::type_complexity)]
    op: Arc<dyn Fn(T, &mut U, &ThreadId) + Send + Sync>,
}

impl<T, U> Clone for Control<T, U> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            op: self.op.clone(),
        }
    }
}

impl<T, U> Control<T, U> {
    /// Instantiates a [`Control`] object.
    pub fn new(acc_base: U, op: impl Fn(T, &mut U, &ThreadId) + 'static + Send + Sync) -> Self {
        Control {
            state: Arc::new(Mutex::new(ChanneledState::new(acc_base))),
            op: Arc::new(op),
        }
    }

    /// Acquires a lock on [`Control`]'s internal Mutex.
    fn lock<'a>(&'a self) -> MutexGuard<'a, ChanneledState<T, U>> {
        self.state.lock().unwrap()
    }

    /// Returns a guard object that dereferences to `self`'s accumulated value. A lock is held during the guard's
    /// lifetime.
    pub fn acc(&self) -> AccGuard<'_, T, U> {
        AccGuard(self.lock())
    }

    /// Provides access to `self`'s accumulated value.
    pub fn with_acc<V>(&self, f: impl FnOnce(&U) -> V) -> V {
        let acc = self.acc();
        f(&acc)
    }

    /// Returns a clone of `self`'s accumulated value.
    pub fn clone_acc(&self) -> U
    where
        U: Clone,
    {
        self.acc().clone()
    }

    /// Returns `self`'s accumulated value, using a value of the same type to replace
    /// the existing accumulated value.
    pub fn take_acc(&self, replacement: U) -> U {
        let mut lock = self.lock();
        let acc = lock.acc_mut();
        replace(acc, replacement)
    }

    /// Spawns a background thread to receive thread-local values if there is no such active thread,
    /// panics otherwise.
    pub fn start_receiving_tls(&self) -> Result<(), MultipleThreadError>
    where
        T: 'static + Send,
        U: 'static + Send,
    {
        // Ensure a single instance of the background thread can be active.
        let mut state = self.lock();
        if state.bkgd_recv_exists {
            return Err(MultipleThreadError);
        }
        state.bkgd_recv_exists = true;
        drop(state);

        let control = self.clone();
        thread::spawn(move || {
            loop {
                let mut state = control.lock();
                let res = state.receive_tls(ReceiveMode::Background, control.op.as_ref());
                if let ReceiveStatus::Stopped = res {
                    // Restore background thread status.
                    state.bkgd_recv_exists = false;
                    break;
                }
                drop(state); // release lock before yielding!
                thread::yield_now(); // this is unnecessary if Mutex is fair
            }
        });
        return Ok(());
    }

    /// Stop background thread receiving thread-local values.
    pub fn stop_receiving_tls(&self) {
        self.lock().sender.send(ChannelItem::StopReceiving).unwrap();
    }

    /// Receive all pending messages in channel, stopping the background thread if it exists.
    pub fn drain_tls(&self) {
        self.stop_receiving_tls();
        self.lock()
            .receive_tls(ReceiveMode::Drain, self.op.as_ref());
    }
}

/// Inner state of [`Holder`].
struct HolderInner<T> {
    tid: ThreadId,
    sender: Sender<ChannelItem<T>>,
}

/// Holds a thread-local [`Sender`] and a smart pointer to a [`Control`], enabling the linkage of the thread-local
/// with the control object.
pub struct Holder<T>(RefCell<Option<HolderInner<T>>>)
where
    T: 'static;

impl<T> Holder<T> {
    /// Instantiates a holder object.
    pub fn new() -> Self {
        Self(RefCell::new(None))
    }

    /// Ensures `self` is initialized, both the [`Sender`] and the `control` smart pointer.
    fn ensure_initialized<U>(&self, control: &Control<T, U>) {
        let mut inner = self.0.borrow_mut();
        if inner.is_none() {
            let state = control.lock();
            let sender = state.sender.clone();
            *inner = Some(HolderInner {
                tid: thread::current().id(),
                sender,
            })
        }
    }

    /// Send data to be aggregated in the `control` object.
    fn send_data(&self, data: T) {
        let inner_opt = self.0.borrow();
        let inner = inner_opt.as_ref().unwrap();
        inner
            .sender
            .send(ChannelItem::Payload(inner.tid, data))
            .unwrap();
    }
}

/// Provides access to the thread-local variable.
pub trait HolderLocalKey<T> {
    /// Ensures the [`Holder`] is initialized, both the [`Sender`] and the `control` smart pointer.
    fn ensure_initialized<U>(&'static self, control: &Control<T, U>);

    /// Send data to be aggregated in the `control` object.
    fn send_data(&'static self, data: T);
}

impl<T> HolderLocalKey<T> for LocalKey<Holder<T>> {
    fn ensure_initialized<U>(&'static self, control: &Control<T, U>) {
        self.with(|h| {
            h.ensure_initialized(&control);
        })
    }

    fn send_data(&'static self, data: T) {
        self.with(|h| h.send_data(data))
    }
}

#[cfg(test)]
mod tests {
    use super::{Control, Holder, HolderLocalKey};
    use crate::test_support::{assert_eq_and_println, ThreadGater};
    use std::{
        collections::HashMap,
        fmt::Debug,
        ops::Deref,
        sync::Mutex,
        thread::{self, ThreadId},
        time::Duration,
    };

    #[derive(Debug, Clone, PartialEq)]
    struct Foo(String);

    type Data = (u32, Foo);

    type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

    thread_local! {
        static MY_TL: Holder<Data> = Holder::new();
    }

    fn op(data: Data, acc: &mut AccValue, tid: &ThreadId) {
        println!(
            "`op` called from {:?} with data {:?}",
            thread::current().id(),
            data
        );

        acc.entry(tid.clone()).or_insert_with(|| HashMap::new());
        let (k, v) = data;
        acc.get_mut(tid).unwrap().insert(k, v.clone());
    }

    fn send_tl_data(k: u32, v: Foo, control: &Control<Data, AccValue>) {
        MY_TL.ensure_initialized(control);
        MY_TL.send_data((k, v));
    }

    #[test]
    fn own_thread_and_explicit_join() {
        let control = Control::new(HashMap::new(), op);

        let main_tid = thread::current().id();
        println!("main_tid={:?}", main_tid);

        let main_thread_gater = ThreadGater::new("main");
        let spawned_thread_gater = ThreadGater::new("spawned");

        let expected_acc_mutex = Mutex::new(HashMap::new());

        let assert_acc = |acc: &AccValue, msg: &str| {
            let exp_guard = expected_acc_mutex.try_lock().unwrap();
            let exp = exp_guard.deref();

            assert_eq_and_println(acc, exp, msg);
        };

        thread::scope(|s| {
            let h = s.spawn(|| {
                let spawned_tid = thread::current().id();
                println!("spawned tid={:?}", spawned_tid);

                let mut my_map = HashMap::<u32, Foo>::new();

                let mut process_value = |gate: u8, k: u32, v: Foo| {
                    main_thread_gater.wait_for(gate);
                    send_tl_data(k, v.clone(), &control);
                    my_map.insert(k, v);
                    expected_acc_mutex
                        .try_lock()
                        .unwrap()
                        .insert(spawned_tid, my_map.clone());
                    // allow background receiving thread to receive above send
                    thread::sleep(Duration::from_millis(10));
                    spawned_thread_gater.open(gate);
                };

                process_value(0, 1, Foo("aa".to_owned()));
                process_value(1, 2, Foo("bb".to_owned()));
                process_value(2, 3, Foo("cc".to_owned()));
                process_value(3, 4, Foo("dd".to_owned()));
            });

            {
                control.start_receiving_tls().unwrap();
            }

            {
                send_tl_data(1, Foo("a".to_owned()), &control);
                send_tl_data(2, Foo("b".to_owned()), &control);
                let my_map = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);

                // Allow background receiving thread to receive above sends.
                thread::sleep(Duration::from_millis(10));

                expected_acc_mutex
                    .try_lock()
                    .unwrap()
                    .insert(main_tid, my_map);
                assert_acc(control.acc().deref(), "Accumulator after main thread sends");
                main_thread_gater.open(0);
            }

            {
                spawned_thread_gater.wait_for(0);
                assert_acc(
                    control.acc().deref(),
                    "Accumulator after 1st spawned thread send",
                );

                {
                    control.stop_receiving_tls();
                    // Allow background receiving thread to process command.
                    thread::sleep(Duration::from_millis(10));
                }

                main_thread_gater.open(1);
            }

            {
                spawned_thread_gater.wait_for(1);
                {
                    let exp = expected_acc_mutex.try_lock().unwrap();
                    let acc = control.acc();
                    assert_ne!(
                        acc.deref(),
                        exp.deref(),
                        "Accumulator should not reflect 2nd spawned thread send",
                    );
                }
                main_thread_gater.open(2);
            }

            {
                control.start_receiving_tls().unwrap();
                // Allow background receiving thread to process command.
                thread::sleep(Duration::from_millis(10));
            }

            {
                spawned_thread_gater.wait_for(2);
                assert_acc(
                    control.acc().deref(),
                    "Accumulator should reflect 2nd and 3rd spawned thread sends",
                );

                {
                    control.stop_receiving_tls();
                    // Allow background receiving thread to process command.
                    thread::sleep(Duration::from_millis(10));
                }

                main_thread_gater.open(3);
            }

            {
                // Join spawned thread.
                h.join().unwrap();

                {
                    let exp = expected_acc_mutex.try_lock().unwrap();
                    let acc = control.acc();
                    assert_ne!(
                        acc.deref(),
                        exp.deref(),
                        "Accumulator should not reflect 4th spawned thread send",
                    );
                }

                control.drain_tls();

                assert_acc(
                    control.acc().deref(),
                    "Accumulator should reflect 4th spawned thread send",
                );
            }

            {
                {
                    control.with_acc(|acc| {
                        assert_acc(
                            acc,
                            "Accumulator after spawned thread join, using control.with_acc()",
                        );
                    });
                }

                {
                    let acc = control.clone_acc();
                    assert_acc(
                        &acc,
                        "Accumulator after spawned thread join, using control.clone_acc()",
                    );
                }

                {
                    let acc = control.take_acc(HashMap::new());
                    assert_acc(
                        &acc,
                        "Accumulator after spawned thread join, using control.take_acc()",
                    );
                }

                {
                    control.with_acc(|acc| {
                        assert_eq_and_println(
                            acc,
                            &HashMap::new(),
                            "Accumulator after control.take_acc()",
                        );
                    });
                }
            }
        });
    }
}
