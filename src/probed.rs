//! This module supports the collection and aggregation of the values of a designated thread-local variable
//! across threads. The following features and constraints apply ...
//! - The designated thread-local variable may be defined and used in the thread responsible for
//! collection/aggregation.
//! - The values of linked thread-local variables are collected and aggregated into the [Control] object's
//! accumulated value when the thread-local variables are dropped following thread termination.
//! - The [`Control`] object's collection/aggregation function may be executed at any time as it ensures
//! synchronization with the participating threads, However, thread-local values need to be initialized again
//! if used after such a call.
//! - After all participating threads other than the thread responsible for collection/aggregation have
//! terminated and joined directly or indirectly into the thread respnosible for collection, a call to the
//! collection/aggregation function will result in the final aggregated value. Implicit joins by
//! scoped threads are correctly handled.

use crate::{
    common::{ControlG, ControlStateG, HolderG, HolderLocalKey, Param},
    TmapD,
};
use std::{
    borrow::BorrowMut,
    cell::RefCell,
    marker::PhantomData,
    ops::DerefMut,
    sync::{Arc, Mutex},
    thread::{LocalKey, ThreadId},
};

//=================
// Core implementation based on common module

#[derive(Debug)]
pub struct ProbedP<T, U>(PhantomData<T>, PhantomData<U>);

impl<T, U> Param for ProbedP<T, U> {
    type Dat = T;
    type Acc = U;
    type Node = Arc<Mutex<Option<T>>>;
    type GData = Arc<Mutex<Option<T>>>;
    type Discr = TmapD<Self>;
}

type ProbedState<T, U> = ControlStateG<ProbedP<T, U>>;

pub type Control<T, U> = ControlG<ProbedP<T, U>>;

impl<T, U> Control<T, U>
where
    T: 'static,
    U: 'static,
{
    pub fn new(acc_base: U, op: impl Fn(T, &mut U, &ThreadId) + 'static + Send + Sync) -> Self {
        Control {
            state: Arc::new(Mutex::new(ProbedState::new(acc_base))),
            op: Arc::new(op),
        }
    }

    pub fn take_tls(&self) {
        let mut guard = self.lock();
        // Need explicit deref_mut to avoid compilation error in for loop.
        let state = guard.borrow_mut().deref_mut();
        for (tid, node) in state.d.tmap.iter() {
            log::trace!("executing `take_tls` for key={:?}", tid);
            let data = node.lock().unwrap().take();
            log::trace!("executed `take` -- `take_tls` for key={:?}", tid);
            if let Some(data) = data {
                log::trace!("executing `op` -- `take_tls` for key={:?}", tid);
                (self.op)(data, &mut state.acc, tid);
            }
        }
    }

    pub fn probe_tls(&self) -> U
    where
        T: Clone,
        U: Clone,
    {
        let state = self.lock();
        let mut acc_clone = state.acc.clone();
        for (tid, node) in state.d.tmap.iter() {
            log::trace!("executing `probe_tls` for key={:?}", tid);
            let data = node.lock().unwrap().clone();
            log::trace!("executed `clone` -- `probe_tls` for key={:?}", tid);
            if let Some(data) = data {
                log::trace!("executing `op` -- `probe_tls` for key={:?}", tid);
                (self.op)(data, &mut acc_clone, tid);
            }
        }
        acc_clone
    }
}

pub type Holder<T, U> = HolderG<ProbedP<T, U>>;

impl<T, U: 'static> Holder<T, U> {
    /// Creates a new `Holder` instance with a function to initialize the data.
    ///
    /// The `make_data` function will be called lazily when data is first accessed to initialize
    /// the inner data value.
    pub fn new(make_data: fn() -> T) -> Self {
        Self {
            data: Arc::new(Mutex::new(None)),
            control: RefCell::new(None),
            make_data,
            init_control_fn: Self::init_control_fn,
            drop_data_fn: Self::drop_data_fn,
        }
    }
}

//=================
// Implementation of HolderLocalKey.

impl<T, U> HolderLocalKey<ProbedP<T, U>> for LocalKey<Holder<T, U>> {
    /// Establishes link with control.
    fn init_control(&'static self, control: &Control<T, U>) {
        self.with(|h| {
            let node = h.data.clone();
            h.init_control(&control, node);
        })
    }

    /// Initializes [`Holder`] data.
    fn init_data(&'static self) {
        self.with(|h| h.init_data())
    }

    /// Ensures [`Holder`] is properly initialized by initializing it if not.
    fn ensure_initialized(&'static self, control: &Control<T, U>) {
        self.with(|h| {
            let node = h.data.clone();
            h.ensure_initialized(&control, node);
        })
    }

    /// Invokes `f` on [`Holder`] data. Panics if data is [`None`].
    fn with_data<V>(&'static self, f: impl FnOnce(&T) -> V) -> V {
        self.with(|h| h.with_data(f))
    }

    /// Invokes `f` on [`Holder`] data. Panics if data is [`None`].
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
        ops::Deref,
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
                let guard = control.acc();
                let acc = guard.deref();
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

            control.take_tls();
            // let keys = [];
            // assert_control_map(&control, &keys, "After call to `take_tls`");
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
            insert_tl_entry(1, Foo("a".to_owned()), &control);
            insert_tl_entry(2, Foo("b".to_owned()), &control);
            println!("after main thread inserts: {:?}", control);

            let _other = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
            // assert_tl(&other, "After main thread inserts");
        }

        thread::sleep(Duration::from_millis(100));

        thread::scope(|s| {
            let h = s.spawn(|| {
                let mut lock = spawned_tid.write().unwrap();
                *lock = thread::current().id();
                drop(lock);

                insert_tl_entry(1, Foo("aa".to_owned()), &control);

                let _other = HashMap::from([(1, Foo("aa".to_owned()))]);
                // assert_tl(&other, "Before spawned thread sleep");

                thread::sleep(Duration::from_millis(200));

                insert_tl_entry(2, Foo("bb".to_owned()), &control);

                let _other = HashMap::from([(1, Foo("aa".to_owned())), (2, Foo("bb".to_owned()))]);
                // assert_tl(&other, "After spawned thread sleep");
            });

            thread::sleep(Duration::from_millis(50));

            let spawned_tid = spawned_tid.try_read().unwrap();
            println!("spawned_tid={:?}", spawned_tid);

            let _keys = [main_tid.clone(), spawned_tid.clone()];
            // assert_control_map(&control, &keys, "Before joining spawned thread");

            h.join().unwrap();

            println!("after h.join(): {:?}", control);

            control.take_tls();
            // let keys = [];
            // assert_control_map(&control, &keys, "After call to `take_tls`");
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
}
