//! This module supports the collection and aggregation of the values from a designated thread-local variable
//! across threads (see package [overfiew and core concepts](super)). The following capabilities and constraints apply ...
//! - The designated thread-local variable may be used in the thread responsible for
//! collection/aggregation.
//! - The linked thread-local variables hold a [`Sender`] that sends values to be aggregated into the
//! [Control] object's accumulated value.
//! - The [`Control`] object provides functions to receive thread-local values on a background thread,
//! stop receiving on a background thread, drain the [`Receiver`], and retrieve the accumulated value.
//! - After all participating threads other than the thread responsible for collection/aggregation have
//! stopped sending values, a call to [`Control::drain_tls`] followed by a call to one of the accumulated
//! value retrieval functions will result in the final aggregated value.
//! - [`Control::start_receiving_tls`] and [`Control::drain_tls`] may be called at any time, followed by a call to
//! [`Control::clone_acc`], to retrieve a partially accumulated value before all threads terminate.
//!
//! ## Usage pattern

//! ```rust
#![doc = include_str!("../../examples/tlm_channeled_i32_accumulator.rs")]
//! ````

//!
//! ## Other examples
//!
//! See another example at [`examples/tlm_channeled_map_accumulator`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/tlm_channeled_map_accumulator.rs).

use crate::tlm::common::POISONED_CONTROL_MUTEX;
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

// Error consts
const RECEIVER_DISCONNECTED: &str = "receiver disconnected";

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

/// Indicates the illegal attempt to spawn multiple concurrent background receiving threads.
#[derive(Debug)]
pub struct MultipleReceiverThreadsError;

impl Display for MultipleReceiverThreadsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "Illegal call to start_receiving_tls as background receiver thread already exists.",
        )
    }
}

impl Error for MultipleReceiverThreadsError {}

/// State of [`Control`].
#[derive(Debug)]
struct ChanneledState<T, U> {
    acc: U,
    receiver: Receiver<ChannelItem<T>>,
    bkgd_recv_exists: bool,
}

impl<T, U> ChanneledState<T, U> {
    fn new(acc: U, receiver: Receiver<ChannelItem<T>>) -> Self {
        Self {
            acc,
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
        op: &(dyn Fn(T, &mut U, ThreadId) + Send + Sync),
    ) -> ReceiveStatus {
        while let Ok(payload) = self.receiver.try_recv() {
            match payload {
                ChannelItem::Payload(tid, data) => op(data, &mut self.acc, tid),
                ChannelItem::StopReceiving => match mode {
                    ReceiveMode::Background => return ReceiveStatus::Stopped,
                    ReceiveMode::Drain => continue,
                },
            }
        }
        ReceiveStatus::CycleCompleted
    }
}

/// Guard object of a [`Control`]'s `acc` field. A lock is held during the guard's lifetime.
#[derive(Debug)]
struct AccGuard<'a, T, U>(MutexGuard<'a, ChanneledState<T, U>>);

impl<'a, T, U> Deref for AccGuard<'a, T, U> {
    type Target = U;

    fn deref(&self) -> &Self::Target {
        self.0.acc()
    }
}

/// Controls the collection and accumulation of thread-local variables linked to this object.
///
/// `T` is the type of the values sent on the channel to this object and `U` is the type of the accumulated value.
/// The thread-locals must be of type [`Holder<T>`].
pub struct Control<T, U>
where
    T: 'static,
{
    /// Reference to thread-local
    pub(crate) tl: &'static LocalKey<Holder<T>>,
    /// Keeps track of registered threads and accumulated value.
    state: Arc<Mutex<ChanneledState<T, U>>>,
    /// Sender on channel that is received by control.
    sender: Sender<ChannelItem<T>>,
    /// Operation that combines data from thread-locals with accumulated value.
    #[allow(clippy::type_complexity)]
    op: Arc<dyn Fn(T, &mut U, ThreadId) + Send + Sync>,
}

impl<T, U> Clone for Control<T, U> {
    fn clone(&self) -> Self {
        Self {
            tl: self.tl,
            state: self.state.clone(),
            sender: self.sender.clone(),
            op: self.op.clone(),
        }
    }
}

impl<T, U> Control<T, U> {
    /// Instantiates a [`Control`] object.
    ///
    /// - `tl` - reference to thread-local static.
    /// - `acc_base` - initial value for accumulation.
    /// - `op` - operation that combines data from thread-locals with accumulated value.
    pub fn new(
        tl: &'static LocalKey<Holder<T>>,
        acc_base: U,
        op: impl Fn(T, &mut U, ThreadId) + 'static + Send + Sync,
    ) -> Self {
        let (sender, receiver) = channel();
        Control {
            tl,
            state: Arc::new(Mutex::new(ChanneledState::new(acc_base, receiver))),
            sender,
            op: Arc::new(op),
        }
    }

    /// Acquires a lock on [`Control`]'s internal mutex.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    fn lock(&self) -> MutexGuard<'_, ChanneledState<T, U>> {
        self.state.lock().expect(POISONED_CONTROL_MUTEX)
    }

    /// Returns a guard object that dereferences to `self`'s accumulated value. A lock is held during the guard's
    /// lifetime.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    pub fn acc(&self) -> impl Deref<Target = U> + '_ {
        AccGuard(self.lock())
    }

    /// Provides access to `self`'s accumulated value.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    pub fn with_acc<V>(&self, f: impl FnOnce(&U) -> V) -> V {
        let acc = self.acc();
        f(&acc)
    }

    /// Returns a clone of `self`'s accumulated value.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    pub fn clone_acc(&self) -> U
    where
        U: Clone,
    {
        self.acc().clone()
    }

    /// Returns `self`'s accumulated value, using a value of the same type to replace
    /// the existing accumulated value.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    pub fn take_acc(&self, replacement: U) -> U {
        let mut lock = self.lock();
        let acc = lock.acc_mut();
        replace(acc, replacement)
    }

    /// Spawns a background thread to receive thread-local values and aggregate them with this object's
    /// accumulated value. May be called repeatedly, provided that there are intervening calls to
    /// [`Self::stop_receiving_tls`] or [`Self::drain_tls`].
    ///
    /// # Errors
    /// Returns an error if there is already an active background receiver thread.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    pub fn start_receiving_tls(&self) -> Result<(), MultipleReceiverThreadsError>
    where
        T: 'static + Send,
        U: 'static + Send,
    {
        // Ensure a single instance of the background thread can be active.
        let mut state = self.lock();
        if state.bkgd_recv_exists {
            return Err(MultipleReceiverThreadsError);
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
        Ok(())
    }

    /// Signals the background receiving thread to terminate itself.
    pub fn stop_receiving_tls(&self) {
        self.sender
            .send(ChannelItem::StopReceiving)
            .expect(RECEIVER_DISCONNECTED);
    }

    /// Receives all pending messages in channel and aggregates the corresponding values,
    /// terminating the background thread if it exists.
    /// May be called repeatedly, even before participating theads have terminated.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    pub fn drain_tls(&self) {
        self.stop_receiving_tls();
        self.lock()
            .receive_tls(ReceiveMode::Drain, self.op.as_ref());
    }

    /// Sends data from the thread where it is called to be accumulated by the [`Control`] instance;
    pub fn send_data(&self, data: T) {
        self.tl.with(|h| {
            h.ensure_linked(self);
            h.send_data(data, self)
        })
    }
}

/// Inner state of [`Holder`].
struct HolderInner<T> {
    tid: ThreadId,
    sender: Sender<ChannelItem<T>>,
}

/// Holds a thread-local [`Sender`], enabling the linkage of the thread-local with the control object.
///
/// `T` is the type of data sent on the channel.
pub struct Holder<T>(RefCell<Option<HolderInner<T>>>)
where
    T: 'static;

impl<T> Holder<T> {
    /// Instantiates a holder object.
    pub fn new() -> Self {
        Self(RefCell::new(None))
    }

    /// Ensures `self` is linked to control.
    fn ensure_linked<U>(&self, control: &Control<T, U>) {
        let mut inner = self.0.borrow_mut();
        if inner.is_none() {
            let sender = control.sender.clone();
            *inner = Some(HolderInner {
                tid: thread::current().id(),
                sender,
            })
        }
    }

    /// Send data to be aggregated in the `control` object. Returns an error if [`Holder`] is
    /// not initialized.
    fn send_data<U>(&self, data: T, control: &Control<T, U>) {
        self.ensure_linked(control);
        let inner_opt = self.0.borrow();
        match inner_opt.deref() {
            Some(inner) => {
                inner
                    .sender
                    .send(ChannelItem::Payload(inner.tid, data))
                    .expect(RECEIVER_DISCONNECTED);
            }
            None => unreachable!("Holder should be initialized by now"),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Control, Holder, MultipleReceiverThreadsError};
    use crate::dev_support::{assert_eq_and_println, ThreadGater};
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

    type Data = (i32, Foo);

    type AccValue = HashMap<ThreadId, HashMap<i32, Foo>>;

    thread_local! {
        static MY_TL: Holder<Data> = Holder::new();
    }

    fn op(data: Data, acc: &mut AccValue, tid: ThreadId) {
        println!(
            "`op` called from {:?} with data {:?}",
            thread::current().id(),
            data
        );

        acc.entry(tid).or_default();
        let (k, v) = data;
        acc.get_mut(&tid).unwrap().insert(k, v.clone());
    }

    #[test]
    fn own_thread_and_explicit_join() {
        let control = Control::new(&MY_TL, HashMap::new(), op);

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

                let mut my_map = HashMap::<i32, Foo>::new();

                let mut process_value = |gate: u8, k: i32, v: Foo| {
                    main_thread_gater.wait_for(gate);
                    control.send_data((k, v.clone()));
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
                control.send_data((1, Foo("a".to_owned())));
                control.send_data((2, Foo("b".to_owned())));
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

    #[test]
    fn multiple_receiver_threads() {
        let control = Control::new(&MY_TL, HashMap::new(), op);

        thread::scope(|s| {
            s.spawn(|| {
                control.send_data((0, Foo("aa".to_owned())));
            });

            control.start_receiving_tls().unwrap();
            let res = control.start_receiving_tls();
            match res {
                Err(MultipleReceiverThreadsError) => (),
                _ => panic!("unexpected result {res:?}"),
            }
        });
    }
}
