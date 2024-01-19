//! Support for ensuring that destructors are run on thread-local variables after the threads terminate,
//! as well as support for accumulating the thread-local values using a binary operation.

pub use crate::common::HolderLocalKey;
use crate::common::{ControlS, ControlStateS, HolderS};
use std::{
    cell::RefCell,
    sync::{Arc, Mutex, MutexGuard},
    thread::{LocalKey, ThreadId},
};

type JoinedState<T, U> = ControlStateS<T, U, usize>;

impl<T, U> JoinedState<T, U>
where
    T: 'static,
    U: 'static,
{
    fn ensure_tls_dropped(state: &mut Self, op: &(dyn Fn(T, &mut U, &ThreadId) + Send + Sync)) {
        for (tid, addr) in state.tmap.iter() {
            log::trace!("executing `ensure_tls_dropped` for key={:?}", tid);
            // Safety: provided that:
            // - All other threads have terminaged and been joined, which means that there is a proper
            //   "happens-before" relationship and the only possible remaining activity on those threads
            //   would be Holder drop method execution, but that method uses the above Mutex to prevent
            //   race conditions.
            let tl: &LocalKey<Holder0<T, U>> = unsafe { tl_from_addr(*addr) };
            tl.with(|h| {
                let mut data_ref = h.data.borrow_mut();
                let data = data_ref.take();
                log::trace!("executed `take` -- `ensure_tls_dropped` for key={:?}", tid);
                if let Some(data) = data {
                    log::trace!("executing `op` -- `ensure_tls_dropped` for key={:?}", tid);
                    op(data, &mut state.acc, tid);
                }
            });
        }
    }
}

fn addr_of_tl<H>(tl: &LocalKey<H>) -> usize {
    let tl_ptr: *const LocalKey<H> = tl;
    tl_ptr as usize
}

unsafe fn tl_from_addr<H>(addr: usize) -> &'static LocalKey<H> {
    &*(addr as *const LocalKey<H>)
}

type Control0<T, U> = ControlS<JoinedState<T, U>>;

impl<T, U> Control0<T, U>
where
    T: 'static,
    U: 'static,
{
    fn new(acc_base: U, op: impl Fn(T, &mut U, &ThreadId) + 'static + Send + Sync) -> Self {
        Control0 {
            state: Arc::new(Mutex::new(JoinedState::new(
                acc_base,
                JoinedState::ensure_tls_dropped,
            ))),
            op: Arc::new(op),
        }
    }
}

type Holder0<T, U> = HolderS<RefCell<Option<T>>, JoinedState<T, U>>;

impl<T, U: 'static> Holder0<T, U> {
    /// Creates a new `Holder` instance with a function to initialize the data.
    ///
    /// The `make_data` function will be called lazily when data is first accessed to initialize
    /// the inner data value.
    fn new(make_data: fn() -> T) -> Self {
        Self {
            data: RefCell::new(None),
            control: RefCell::new(None),
            make_data,
        }
    }
}

//=================
// Public wrappers

pub struct ControlLock<'a, T, U>(MutexGuard<'a, JoinedState<T, U>>);

impl<'a, T, U> ControlLock<'a, T, U> {
    pub fn acc(&self) -> &U {
        self.0.acc()
    }
}

#[derive(Debug)]
pub struct Control<T, U>(Control0<T, U>);

impl<T, U> Control<T, U>
where
    T: 'static,
    U: 'static,
{
    pub fn new(acc_base: U, op: impl Fn(T, &mut U, &ThreadId) + 'static + Send + Sync) -> Self {
        Self(Control0::new(acc_base, op))
    }

    /// Acquires a lock for use by public `Control` methods that require its internal Mutex to be locked.
    ///
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub fn lock(&self) -> ControlLock<'_, T, U> {
        ControlLock(self.0.lock())
    }

    /// Provides access to the accumulated value in the [Control] struct.
    ///
    /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub fn with_acc<V>(&self, lock: &ControlLock<'_, T, U>, f: impl FnOnce(&U) -> V) -> V {
        self.0.with_acc(&lock.0, f)
    }

    /// Returns the accumulated value in the [Control] struct, using a value of the same type to replace
    /// the existing accumulated value.
    ///
    /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub fn take_acc(&self, lock: &mut ControlLock<'_, T, U>, replacement: U) -> U {
        self.0.take_acc(&mut lock.0, replacement)
    }

    /// Forces all registered thread-local values that have not already been dropped to be effectively dropped
    /// by replacing the [`Holder`] data with [`None`], and accumulates the values contained in those thread-locals.
    ///
    /// Should only be called from a thread (typically the main thread) under the following conditions:
    /// - All other threads that use this [`Control`] instance must have been directly or indirectly spawned
    ///   from this thread; ***and***
    /// - Any prior updates to holder values must have had a *happened before* relationship to this call;
    ///   ***and***
    /// - Any further updates to holder values must have a *happened after* relationship to this call.
    ///   
    /// In particular, the last two conditions are satisfied if the call to this method takes place after
    /// this thread joins (directly or indirectly) with all threads that have registered with this [`Control`]
    /// instance.
    ///
    /// These conditions ensure the absence of data races with a proper "happens-before" condition between any
    /// thread-local data updates and this call.
    ///
    /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub fn ensure_tls_dropped(&self, lock: &mut ControlLock<'_, T, U>) {
        self.0.ensure_tls_dropped(&mut lock.0)
    }
}

#[derive(Debug)]
pub struct Holder<T, U>(Holder0<T, U>)
where
    T: 'static;

impl<T, U> Holder<T, U>
where
    U: 'static,
{
    /// Creates a new `Holder` instance with a function to initialize the data.
    ///
    /// The `make_data` function will be called lazily when data is first accessed to initialize
    /// the inner data value.
    pub fn new(make_data: fn() -> T) -> Self {
        Self(Holder0::new(make_data))
    }

    /// Establishes link with control.
    fn init_control(&self, control: &Control<T, U>, node: usize) {
        self.0.init_control(&control.0, node)
    }

    fn init_data(&self) {
        self.0.init_data()
    }

    fn ensure_initialized(&self, control: &Control<T, U>, node: usize) {
        self.0.ensure_initialized(&control.0, node)
    }

    /// Invokes `f` on data. Panics if data is [`None`].
    fn with_data<V>(&self, f: impl FnOnce(&T) -> V) -> V {
        self.0.with_data(f)
    }

    /// Invokes `f` on data. Panics if data is [`None`].
    fn with_data_mut<V>(&self, f: impl FnOnce(&mut T) -> V) -> V {
        self.0.with_data_mut(f)
    }
}

impl<T, U> HolderLocalKey<T, Control<T, U>> for LocalKey<Holder<T, U>> {
    /// Establishes link with control.
    fn init_control(&'static self, control: &Control<T, U>) {
        self.with(|h| h.init_control(&control, addr_of_tl(self)))
    }

    fn init_data(&'static self) {
        self.with(|h| h.init_data())
    }

    fn ensure_initialized(&'static self, control: &Control<T, U>) {
        self.with(|h| h.ensure_initialized(&control, addr_of_tl(self)))
    }

    /// Invokes `f` on data. Panics if data is [`None`].
    fn with_data<V>(&'static self, f: impl FnOnce(&T) -> V) -> V {
        self.with(|h| h.with_data(f))
    }

    /// Invokes `f` on data. Panics if data is [`None`].
    fn with_data_mut<V>(&'static self, f: impl FnOnce(&mut T) -> V) -> V {
        self.with(|h| h.with_data_mut(f))
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

    type Data = HashMap<u32, Foo>;

    type AccumulatorMap = HashMap<ThreadId, HashMap<u32, Foo>>;

    thread_local! {
        static MY_FOO_MAP: Holder<Data, AccumulatorMap> = Holder::new(HashMap::new);
    }

    fn insert_tl_entry(k: u32, v: Foo, control: &Control<Data, AccumulatorMap>) {
        MY_FOO_MAP.ensure_initialized(control);
        MY_FOO_MAP.with_data_mut(|data| data.insert(k, v));
    }

    fn op(data: HashMap<u32, Foo>, acc: &mut AccumulatorMap, tid: &ThreadId) {
        println!(
            "`op` called from {:?} with data {:?}",
            thread::current().id(),
            data
        );

        acc.entry(tid.clone()).or_insert_with(|| HashMap::new());
        for (k, v) in data {
            acc.get_mut(tid).unwrap().insert(k, v.clone());
        }
    }

    fn assert_tl(other: &Data, msg: &str) {
        MY_FOO_MAP.with_data(|map| {
            assert_eq!(map, other, "{msg}");
        });
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

                            insert_tl_entry(1, Foo("a".to_owned() + &si), control);

                            let other = HashMap::from([(1, Foo("a".to_owned() + &si))]);
                            assert_tl(&other, "After 1st insert");

                            insert_tl_entry(2, Foo("b".to_owned() + &si), control);

                            let other = HashMap::from([
                                (1, Foo("a".to_owned() + &si)),
                                (2, Foo("b".to_owned() + &si)),
                            ]);
                            assert_tl(&other, "After 2nd insert");
                        }
                    })
                })
                .collect::<Vec<_>>();

            thread::sleep(Duration::from_millis(50));

            let spawned_tids = spawned_tids.try_read().unwrap();
            println!("spawned_tid={:?}", spawned_tids);

            hs.into_iter().for_each(|h| h.join().unwrap());

            println!("after hs join: {:?}", control);
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
                let acc = lock.acc();
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
            insert_tl_entry(1, Foo("a".to_owned()), &control);
            insert_tl_entry(2, Foo("b".to_owned()), &control);
            println!("after main thread inserts: {:?}", control);

            let other = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
            assert_tl(&other, "After main thread inserts");
        }

        thread::sleep(Duration::from_millis(100));

        thread::scope(|s| {
            let h = s.spawn(|| {
                let mut lock = spawned_tid.write().unwrap();
                *lock = thread::current().id();
                drop(lock);

                insert_tl_entry(1, Foo("aa".to_owned()), &control);

                let other = HashMap::from([(1, Foo("aa".to_owned()))]);
                assert_tl(&other, "Before spawned thread sleep");

                thread::sleep(Duration::from_millis(200));

                insert_tl_entry(2, Foo("bb".to_owned()), &control);

                let other = HashMap::from([(1, Foo("aa".to_owned())), (2, Foo("bb".to_owned()))]);
                assert_tl(&other, "After spawned thread sleep");
            });

            thread::sleep(Duration::from_millis(50));

            let spawned_tid = spawned_tid.try_read().unwrap();
            println!("spawned_tid={:?}", spawned_tid);

            // let keys = [main_tid.clone(), spawned_tid.clone()];
            // assert_control_map(&control, &keys, "Before joining spawned thread");

            h.join().unwrap();

            println!("after h.join(): {:?}", control);

            control.ensure_tls_dropped(&mut control.lock());
            // let keys = [];
            // assert_control_map(&control, &keys, "After call to `ensure_tls_dropped`");
        });

        {
            let spawned_tid = spawned_tid.try_read().unwrap();
            let map1 = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
            let map2 = HashMap::from([(1, Foo("aa".to_owned())), (2, Foo("bb".to_owned()))]);
            let map = HashMap::from([(main_tid.clone(), map1), (spawned_tid.clone(), map2)]);

            {
                let lock = control.lock();
                let acc = lock.acc();
                assert_eq!(acc, &map, "Accumulator check");
            }
        }
    }
}
