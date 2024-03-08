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

    use std::{
        collections::HashMap,
        fmt::Debug,
        ops::Deref,
        sync::RwLock,
        thread::{self, ThreadId},
        time::Duration,
    };

    #[derive(Debug, Clone, PartialEq)]
    struct Foo(String);

    type Data = (u32, Foo);

    type AccumulatorMap = HashMap<ThreadId, HashMap<u32, Foo>>;

    thread_local! {
        static MY_FOO_MAP: Holder<Data> = Holder::new();
    }

    fn send_tl_data(k: u32, v: Foo, control: &Control<Data, AccumulatorMap>) {
        MY_FOO_MAP.ensure_initialized(control);
        MY_FOO_MAP.send_data((k, v));
    }

    fn op(data: Data, acc: &mut AccumulatorMap, tid: &ThreadId) {
        println!(
            "`op` called from {:?} with data {:?}",
            thread::current().id(),
            data
        );

        acc.entry(tid.clone()).or_insert_with(|| HashMap::new());
        let (k, v) = data;
        acc.get_mut(tid).unwrap().insert(k, v.clone());
    }

    #[test]
    fn test_1() {
        let control = Control::new(HashMap::new(), op);
        let spawned_tids = RwLock::new(vec![thread::current().id(), thread::current().id()]);

        thread::scope(|s| {
            let hs = (0..2)
                .map(|i| {
                    s.spawn({
                        // These are to prevent the move closure from moving `control` and `spawned_tids`.
                        // The closure has to be `move` because it needs to own `i`.
                        let control = &control;
                        let spawned_tids = &spawned_tids;

                        move || {
                            let si = i.to_string();

                            let mut lock = spawned_tids.write().unwrap();
                            lock[i] = thread::current().id();
                            drop(lock);

                            send_tl_data(1, Foo("a".to_owned() + &si), control);
                            send_tl_data(2, Foo("b".to_owned() + &si), control);
                        }
                    })
                })
                .collect::<Vec<_>>();

            thread::sleep(Duration::from_millis(50));

            let spawned_tids = spawned_tids.try_read().unwrap();
            println!("spawned_tid={:?}", spawned_tids);

            hs.into_iter().for_each(|h| h.join().unwrap());

            control.drain_tls();

            println!("after hs join: {:?}", control.acc());
        });

        {
            let spawned_tids = spawned_tids.try_read().unwrap();
            let map_0 = HashMap::from([(1, Foo("a0".to_owned())), (2, Foo("b0".to_owned()))]);
            let map_1 = HashMap::from([(1, Foo("a1".to_owned())), (2, Foo("b1".to_owned()))]);
            let map = HashMap::from([
                (spawned_tids[0].clone(), map_0),
                (spawned_tids[1].clone(), map_1),
            ]);

            {
                let acc = control.acc();
                assert!(acc.eq(&map), "Accumulator check: acc={acc:?}, map={map:?}");
            }
        }
    }

    #[test]
    fn test_2() {
        let control = Control::new(HashMap::new(), op);

        let main_tid = thread::current().id();
        println!("main_tid={:?}", main_tid);
        let spawned_tid = RwLock::new(thread::current().id());

        {
            send_tl_data(1, Foo("a".to_owned()), &control);
            send_tl_data(2, Foo("b".to_owned()), &control);
            println!("after main thread inserts: {:?}", control.acc());
        }

        thread::sleep(Duration::from_millis(100));

        thread::scope(|s| {
            let h = s.spawn(|| {
                let mut lock = spawned_tid.write().unwrap();
                *lock = thread::current().id();
                drop(lock);

                send_tl_data(1, Foo("aa".to_owned()), &control);

                thread::sleep(Duration::from_millis(200));

                send_tl_data(2, Foo("bb".to_owned()), &control);
            });

            thread::sleep(Duration::from_millis(50));

            let spawned_tid = spawned_tid.try_read().unwrap();
            println!("spawned_tid={:?}", spawned_tid);

            h.join().unwrap();

            control.drain_tls();

            println!("after h.join(): {:?}", control.acc());
        });

        {
            let spawned_tid = spawned_tid.try_read().unwrap();
            let map1 = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
            let map2 = HashMap::from([(1, Foo("aa".to_owned())), (2, Foo("bb".to_owned()))]);
            let map = HashMap::from([(main_tid.clone(), map1), (spawned_tid.clone(), map2)]);

            {
                let acc = control.acc();
                assert_eq!(acc.deref(), &map, "Accumulator check");
            }
        }
    }

    #[test]
    fn test_3() {
        let control = Control::new(HashMap::new(), op);

        let main_tid = thread::current().id();
        println!("main_tid={:?}", main_tid);
        let spawned_tid = RwLock::new(thread::current().id());

        {
            send_tl_data(1, Foo("a".to_owned()), &control);
            send_tl_data(2, Foo("b".to_owned()), &control);
            println!("after main thread inserts: {:?}", control.acc());
        }

        thread::sleep(Duration::from_millis(100));

        control.start_receiving_tls().unwrap();

        thread::scope(|s| {
            let h = s.spawn(|| {
                let mut lock = spawned_tid.write().unwrap();
                *lock = thread::current().id();
                drop(lock);

                send_tl_data(1, Foo("aa".to_owned()), &control);

                thread::sleep(Duration::from_millis(200));

                send_tl_data(2, Foo("bb".to_owned()), &control);
            });

            thread::sleep(Duration::from_millis(50));

            let spawned_tid = spawned_tid.try_read().unwrap();
            println!("spawned_tid={:?}", spawned_tid);

            h.join().unwrap();

            control.stop_receiving_tls();

            println!("after h.join(): {:?}", control.acc());
        });

        {
            let spawned_tid = spawned_tid.try_read().unwrap();
            let map1 = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
            let map2 = HashMap::from([(1, Foo("aa".to_owned())), (2, Foo("bb".to_owned()))]);
            let map = HashMap::from([(main_tid.clone(), map1), (spawned_tid.clone(), map2)]);

            {
                // Drain channel.
                control.drain_tls();

                let acc = control.acc();
                assert_eq!(acc.deref(), &map, "Accumulator check");
            }
        }
    }
}
