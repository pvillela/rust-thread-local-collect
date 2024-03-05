//! Support for ensuring that destructors are run on thread-local variables after the threads terminate,
//! as well as support for accumulating the thread-local values using a binary operation.

use std::{
    cell::RefCell,
    mem::replace,
    ops::Deref,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex, MutexGuard,
    },
    thread::{self, LocalKey, ThreadId},
};

enum ChannelItem<T> {
    StopReceiving,
    Payload(ThreadId, T),
}

enum ReceiveStatus {
    Stopped,
    CycleCompleted,
}

#[derive(Debug)]
pub struct ChanneledState<T, U> {
    acc: U,
    sender: Sender<ChannelItem<T>>,
    receiver: Receiver<ChannelItem<T>>,
}

impl<T, U> ChanneledState<T, U> {
    fn new(acc: U) -> Self {
        let (sender, receiver) = channel();
        Self {
            acc,
            sender,
            receiver,
        }
    }

    fn acc(&self) -> &U {
        &self.acc
    }

    fn acc_mut(&mut self) -> &mut U {
        &mut self.acc
    }

    fn receive_tls(&mut self, op: &(dyn Fn(T, &mut U, &ThreadId) + Send + Sync)) -> ReceiveStatus {
        while let Ok(payload) = self.receiver.try_recv() {
            match payload {
                ChannelItem::Payload(tid, data) => op(data, &mut self.acc, &tid),
                ChannelItem::StopReceiving => return ReceiveStatus::Stopped,
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

/// Controls the destruction of thread-local values registered with it.
/// Such values of type `T` must be held in thread-locals of type [`Holder<T>`].
/// `U` is the type of the accumulated value resulting from an initial base value and
/// the application of an operation to each thread-local value and the current accumulated
/// value upon dropping of each thread-local value. (See [`new`](Control::new) method.)
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
    pub fn new(acc_base: U, op: impl Fn(T, &mut U, &ThreadId) + 'static + Send + Sync) -> Self {
        Control {
            state: Arc::new(Mutex::new(ChanneledState::new(acc_base))),
            op: Arc::new(op),
        }
    }

    /// Returns a guard of the object's `acc` field. This guard holds a lock on `self` which is only released
    /// when the guard object is dropped.
    pub fn acc(&self) -> AccGuard<'_, T, U> {
        AccGuard(self.lock())
    }

    /// Acquires a lock for use by public `Control` methods that require its internal Mutex to be locked.
    ///
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    fn lock<'a>(&'a self) -> MutexGuard<'a, ChanneledState<T, U>> {
        self.state.lock().unwrap()
    }

    /// Provides access to the accumulated value in the [Control] struct.
    pub fn with_acc<V>(&self, f: impl FnOnce(&U) -> V) -> V {
        let acc = self.acc();
        f(&acc)
    }

    /// Returns the accumulated value in the [Control] struct, using a value of the same type to replace
    /// the existing accumulated value.
    pub fn take_acc(&self, replacement: U) -> U {
        let mut lock = self.lock();
        let acc = lock.acc_mut();
        replace(acc, replacement)
    }

    pub fn start_receiving_tls(&self)
    where
        T: 'static + Send,
        U: 'static + Send,
    {
        let control = self.clone();
        thread::spawn(move || {
            loop {
                let mut state = control.lock();
                let res = state.receive_tls(control.op.as_ref());
                if let ReceiveStatus::Stopped = res {
                    return;
                }
                thread::yield_now(); // this is unnecessary if Mutex is fair
            }
        });
    }

    pub fn stop_receiving_tls(&self) {
        self.lock().sender.send(ChannelItem::StopReceiving).unwrap();
    }

    pub fn receive_tls(&self) {
        self.lock().receive_tls(self.op.as_ref());
    }
}

struct HolderInner<T> {
    tid: ThreadId,
    sender: Sender<ChannelItem<T>>,
}

/// Holds thead-local data to enable registering it with [`Control`].
pub struct Holder<T>(RefCell<Option<HolderInner<T>>>)
where
    T: 'static;

impl<T> Holder<T> {
    /// Creates a new `Holder` instance with a function to initialize the data.
    ///
    /// The `make_data` function will be called lazily when data is first accessed to initialize
    /// the inner data value.
    pub fn new() -> Self {
        Self(RefCell::new(None))
    }

    pub fn ensure_initialized<U>(&self, control: &Control<T, U>) {
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

    pub fn send_data(&self, data: T) {
        let inner_opt = self.0.borrow();
        let inner = inner_opt.as_ref().unwrap();
        inner
            .sender
            .send(ChannelItem::Payload(inner.tid, data))
            .unwrap();
    }
}

pub trait HolderLocalKey<T> {
    fn ensure_initialized<U>(&'static self, control: &Control<T, U>);

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

            control.receive_tls();

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

            control.receive_tls();

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

        control.start_receiving_tls();

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
                control.receive_tls();

                let acc = control.acc();
                assert_eq!(acc.deref(), &map, "Accumulator check");
            }
        }
    }
}
