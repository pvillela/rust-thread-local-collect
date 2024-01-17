//! Support for ensuring that destructors are run on thread-local variables after the threads terminate,
//! as well as support for accumulating the thread-local values using a binary operation.

use std::{
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    mem::replace,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex, MutexGuard,
    },
    thread::{self, LocalKey, ThreadId},
};
pub struct ChanneledState<T, U> {
    pub(crate) acc: U,
    pub(crate) tmap: HashMap<ThreadId, Receiver<T>>,
}

impl<T, U> ChanneledState<T, U> {
    pub(crate) fn new(acc: U) -> Self {
        Self {
            acc,
            tmap: HashMap::new(),
        }
    }

    fn acc(&self) -> &U {
        &self.acc
    }

    fn acc_mut(&mut self) -> &mut U {
        &mut self.acc
    }

    fn register_node(&mut self, node: Receiver<T>, tid: &ThreadId) {
        self.tmap.insert(tid.clone(), node);
    }

    fn deregister_thread(
        &mut self,
        tid: &ThreadId,
        op: &(dyn Fn(T, &mut U, &ThreadId) + Send + Sync),
    ) {
        if let Some(receiver) = self.tmap.remove(tid) {
            // Drain data in receiver
            for data in receiver {
                op(data, &mut self.acc, tid)
            }
        }
    }

    fn collect_all(&mut self, op: &(dyn Fn(T, &mut U, &ThreadId) + Send + Sync)) {
        // println!("entered `ChanneledState::collect_all`");
        for (tid, receiver) in self.tmap.iter() {
            // Drain data in receiver
            while let Ok(data) = receiver.try_recv() {
                op(data, &mut self.acc, &tid)
            }
        }
        // println!("exited `ChanneledState::collect_all`");
    }
}

/// Controls the destruction of thread-local values registered with it.
/// Such values of type `T` must be held in thread-locals of type [`Holder<T>`].
/// `U` is the type of the accumulated value resulting from an initial base value and
/// the application of an operation to each thread-local value and the current accumulated
/// value upon dropping of each thread-local value. (See [`new`](Control::new) method.)
pub struct Control<T, U> {
    /// Keeps track of registered threads and accumulated value.
    pub(crate) state: Arc<Mutex<ChanneledState<T, U>>>,
    /// Binary operation that combines data from thread-locals with accumulated value.
    #[allow(clippy::type_complexity)]
    pub(crate) op: Arc<dyn Fn(T, &mut U, &ThreadId) + Send + Sync>,
}

impl<T, U> Clone for Control<T, U> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            op: self.op.clone(),
        }
    }
}

impl<T, U> Control<T, U>
where
    T: 'static,
    U: 'static,
{
    pub fn new(acc_base: U, op: impl Fn(T, &mut U, &ThreadId) + 'static + Send + Sync) -> Self {
        Control {
            state: Arc::new(Mutex::new(ChanneledState::new(acc_base))),
            op: Arc::new(op),
        }
    }

    /// Acquires a lock for use by public `Control` methods that require its internal Mutex to be locked.
    ///
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub fn lock<'a>(&'a self) -> MutexGuard<'a, ChanneledState<T, U>> {
        self.state.lock().unwrap()
    }

    /// Provides access to the accumulated value in the [Control] struct.
    ///
    /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub fn with_acc<V>(
        &self,
        lock: &MutexGuard<'_, ChanneledState<T, U>>,
        f: impl FnOnce(&U) -> V,
    ) -> V {
        let acc = lock.acc();
        f(acc)
    }

    /// Returns the accumulated value in the [Control] struct, using a value of the same type to replace
    /// the existing accumulated value.
    ///
    /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub fn take_acc(&self, lock: &mut MutexGuard<'_, ChanneledState<T, U>>, replacement: U) -> U {
        let acc = lock.acc_mut();
        replace(acc, replacement)
    }

    fn register_node(&self, node: Receiver<T>, tid: &ThreadId) {
        let mut lock = self.lock();
        lock.register_node(node, tid)
    }

    pub fn collect_all(&self, lock: &mut MutexGuard<'_, ChanneledState<T, U>>) {
        // println!("entered `Control::collect_all`");
        lock.collect_all(self.op.as_ref());
        // println!("exited `Control::collect_all`");
    }

    fn tl_sender_dropped(&self, tid: &ThreadId) {
        let mut lock = self.lock();
        lock.deregister_thread(tid, self.op.as_ref());
    }
}

/// Holds thead-local data to enable registering it with [`Control`].
pub struct Holder<T, U>
where
    T: 'static,
    U: 'static,
{
    pub(crate) data: RefCell<Option<Sender<T>>>,
    pub(crate) control: RefCell<Option<Control<T, U>>>,
}

impl<T, U: 'static> Holder<T, U> {
    /// Creates a new `Holder` instance with a function to initialize the data.
    ///
    /// The `make_data` function will be called lazily when data is first accessed to initialize
    /// the inner data value.
    pub fn new() -> Self {
        Self {
            data: RefCell::new(None),
            control: RefCell::new(None),
        }
    }

    fn control(&self) -> Ref<'_, Option<Control<T, U>>> {
        self.control.borrow()
    }

    fn sender_guard(&self) -> RefMut<'_, Option<Sender<T>>> {
        self.data.borrow_mut()
    }

    /// Establishes link with control.
    fn init_control(&self, control: &Control<T, U>, node: Receiver<T>) {
        let mut ctrl_ref = self.control.borrow_mut();
        *ctrl_ref = Some(control.clone());
        control.register_node(node, &thread::current().id())
    }

    fn init_channel(&self) -> Receiver<T> {
        let mut guard = self.sender_guard();
        let (sender, receiver) = channel();
        *guard = Some(sender);
        receiver
    }

    pub fn ensure_initialized(&self, control: &Control<T, U>) {
        if self.control().as_ref().is_none() {
            let node = self.init_channel();
            self.init_control(control, node);
        }
    }

    fn drop_sender(&self) {
        let mut sender_guard = self.sender_guard();
        let sender = sender_guard.take();
        drop(sender);
        let control: Ref<'_, Option<Control<T, U>>> = self.control();
        match control.as_ref() {
            None => (),
            Some(control) => control.tl_sender_dropped(&thread::current().id()),
        }
    }

    pub fn send_data(&self, data: T) {
        self.data.borrow().as_ref().unwrap().send(data).unwrap();
    }
}

impl<T, U> Drop for Holder<T, U> {
    /// Ensures the held data, if any, is deregistered from the associated [`Control`] instance
    /// and the control instance's accumulation operation is invoked with the held data.
    fn drop(&mut self) {
        self.drop_sender()
    }
}

pub trait HolderLocalKey<T, Ctrl> {
    fn ensure_initialized(&'static self, control: &Ctrl);

    fn send_data(&'static self, data: T);
}

impl<T, U> HolderLocalKey<T, Control<T, U>> for LocalKey<Holder<T, U>> {
    fn ensure_initialized(&'static self, control: &Control<T, U>) {
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
        sync::RwLock,
        thread::{self, ThreadId},
        time::Duration,
    };

    #[derive(Debug, Clone, PartialEq)]
    struct Foo(String);

    type Data = (u32, Foo);

    type AccumulatorMap = HashMap<ThreadId, HashMap<u32, Foo>>;

    thread_local! {
        static MY_FOO_MAP: Holder<Data, AccumulatorMap> = Holder::new();
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

            println!("after hs join: {:?}", control.lock().acc);
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
                let lock = control.lock();
                let acc = &lock.acc;
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
            println!("after main thread inserts: {:?}", control.lock().acc);
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

            println!("after h.join(): {:?}", control.lock().acc);

            control.collect_all(&mut control.lock());
        });

        {
            let spawned_tid = spawned_tid.try_read().unwrap();
            let map1 = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
            let map2 = HashMap::from([(1, Foo("aa".to_owned())), (2, Foo("bb".to_owned()))]);
            let map = HashMap::from([(main_tid.clone(), map1), (spawned_tid.clone(), map2)]);

            {
                let lock = control.lock();
                let acc = &lock.acc;
                assert_eq!(acc, &map, "Accumulator check");
            }
        }
    }
}
