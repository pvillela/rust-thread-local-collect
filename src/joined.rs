//! This module supports the collection and aggregation of the values of a designated thread-local variable
//! across threads. The following features and constraints apply ...
//! - The designated thread-local variable may be defined and used in the thread responsible for
//! collection/aggregation.
//! - The collection/aggregation function is unsafe unless executed after all participating threads,
//! other than the thread responsible for collection/aggregation, have
//! terminated and joined directly or indirectly into the thread respnosible for collection. Implicit joins by
//! scoped threads are correctly handled.

use crate::common::{ControlG, ControlStateG, HolderG, HolderLocalKey, Param};
use std::{
    cell::RefCell,
    marker::PhantomData,
    ops::DerefMut,
    sync::{Arc, Mutex},
    thread::{LocalKey, ThreadId},
};

//=================
// Core implementation based on common module

#[derive(Debug)]
pub struct JoinedParam<T, U>(PhantomData<T>, PhantomData<U>);

impl<T, U> Param for JoinedParam<T, U> {
    type Dat = T;
    type Acc = U;
    type Node = usize;
    type GData = RefCell<Option<T>>;
}

type JoinedState<T, U> = ControlStateG<JoinedParam<T, U>>;

fn addr_of_tl<H>(tl: &LocalKey<H>) -> usize {
    let tl_ptr: *const LocalKey<H> = tl;
    tl_ptr as usize
}

unsafe fn tl_from_addr<H>(addr: usize) -> &'static LocalKey<H> {
    &*(addr as *const LocalKey<H>)
}

pub type Control<T, U> = ControlG<JoinedParam<T, U>>;

impl<T, U> Control<T, U>
where
    T: 'static,
    U: 'static,
{
    pub fn new(acc_base: U, op: impl Fn(T, &mut U, &ThreadId) + 'static + Send + Sync) -> Self {
        Control {
            state: Arc::new(Mutex::new(JoinedState::new(acc_base))),
            op: Arc::new(op),
        }
    }

    /// This function is UNSAFE. Its unsafe nature is masked so that it can be used as part of the framework
    /// defined in the [crate::common] module. [`Control::take_tls`] is flagged as unsafe because this
    /// function is unsafe.
    ///
    /// This function can be called safely provided that:
    /// - All other threads have terminaged and been joined, which means that there is a proper
    ///   "happens-before" relationship and the only possible remaining activity on those threads
    ///   would be Holder drop method execution on implicitly joined scoped threads,
    ///   but that method uses the above Mutex to prevent race conditions.
    pub unsafe fn take_tls(&self) {
        let mut guard = self.lock();
        // Need explicit deref_mut to avoid compilation error in for loop.
        let state = guard.deref_mut();
        for (tid, addr) in state.tmap.iter() {
            log::trace!("executing `take_tls` for key={:?}", tid);
            // Safety: provided that:
            // - All other threads have terminaged and been joined, which means that there is a proper
            //   "happens-before" relationship and the only possible remaining activity on those threads
            //   would be Holder drop method execution on implicitly joined scoped threads,
            //   but that method uses the above Mutex to prevent race conditions.
            let tl: &LocalKey<Holder<T, U>> = tl_from_addr(*addr);
            tl.with(|h| {
                let mut data_ref = h.data.borrow_mut();
                let data = data_ref.take();
                log::trace!("executed `take` -- `take_tls` for key={:?}", tid);
                if let Some(data) = data {
                    log::trace!("executing `op` -- `take_tls` for key={:?}", tid);
                    (self.op)(data, &mut state.acc, tid);
                }
            });
        }
    }
}

pub type Holder<T, U> = HolderG<JoinedParam<T, U>>;

impl<T, U: 'static> Holder<T, U> {
    pub fn new(make_data: fn() -> T) -> Self {
        Self {
            data: RefCell::new(None),
            control: RefCell::new(None),
            make_data,
        }
    }
}

// //=================
// // Public wrappers

// /// Guard object of a [`Control`]'s `acc` field.
// pub struct AccGuard<'a, T, U>(AccGuardS<'a, JoinedParam<T, U>>);

// impl<'a, T, U> Deref for AccGuard<'a, T, U> {
//     type Target = U;

//     fn deref(&self) -> &Self::Target {
//         self.0.deref()
//     }
// }

// /// Keeps track of threads using the designated thread-local variable. Contains an accumulation operation `op`
// /// and an accumulated value `acc`. The accumulated value is updated by applying `op` to each thread-local
// /// data value and `acc` when the thread-local value is dropped or the [`Control::take_tls`] method is called.
// #[derive(Debug)]
// pub struct Control<T, U>(Control0<T, U>);

// impl<T, U> Control<T, U>
// where
//     T: 'static,
//     U: 'static,
// {
//     /// Instantiates a [`Control`] object with `acc`_base as the initial value of its accumulator and
//     /// `op` as its aggregation operation.
//     pub fn new(acc_base: U, op: impl Fn(T, &mut U, &ThreadId) + 'static + Send + Sync) -> Self {
//         Self(Control0::new(acc_base, op))
//     }

//     /// Returns a guard of the object's `acc` field. This guard holds a lock on `self` which is only released
//     /// when the guard object is dropped.
//     pub fn acc(&self) -> AccGuard<'_, T, U> {
//         AccGuard(self.0.acc())
//     }

//     /// Provides read-only access to the accumulated value in the [Control] struct.
//     ///
//     /// Note:
//     /// - The aggregated value is reflective of all participating threads if and only if it is accessed after
//     /// all participating theads have
//     ///   - terminated and joined directly or indirectly into the thread respnosible for collection; and
//     ///   - [`Control::take_tls`] has been called after the above.
//     /// - Implicit joins by scoped threads are correctly handled.
//     ///
//     /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
//     ///
//     /// See also [`take_acc`](Self::take_acc) and [`ControlLock::acc`].
//     pub fn with_acc<V>(&self, f: impl FnOnce(&U) -> V) -> V {
//         self.0.with_acc(f)
//     }

//     /// Returns the accumulated value in the [Control] struct, using a value of the same type to replace
//     /// the existing accumulated value.
//     ///
//     /// Note:
//     /// - The aggregated value is reflective of all participating threads if and only if it is accessed after
//     /// all participating theads have
//     ///   - terminated and joined directly or indirectly into the thread respnosible for collection; and
//     ///   - [`Control::take_tls`] has been called after the above.
//     /// - Implicit joins by scoped threads are correctly handled.
//     ///
//     /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
//     ///
//     /// See also [`with_acc`](Self::with_acc) and [`ControlLock::acc`].
//     pub fn take_acc(&self, replacement: U) -> U {
//         self.0.take_acc(replacement)
//     }

//     /// Forces all registered thread-local values that have not already been dropped to be effectively dropped
//     /// by replacing the [`Holder`] data with [`None`], and accumulates the values contained in those thread-locals.
//     ///
//     /// This function can be safely called from a thread (typically the main thread) under the following conditions:
//     /// - All other threads that use this [`Control`] instance must have been directly or indirectly spawned
//     ///   from this thread; ***and***
//     /// - Any prior updates to holder values must have had a *happened before* relationship to this call;
//     ///   ***and***
//     /// - Any further updates to holder values must have a *happened after* relationship to this call.
//     ///
//     /// In particular, the last two conditions are satisfied if the call to this method takes place after
//     /// this thread joins (directly or indirectly) with all threads that have registered with this [`Control`]
//     /// instance. This holds even for implicit joins from scoped threads.
//     ///
//     /// These conditions ensure the absence of data races with a proper "happens-before" condition between any
//     /// thread-local data updates and this call.
//     ///
//     /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
//     pub unsafe fn take_tls(&self) {
//         self.0.take_tls()
//     }
// }

// /// Holds the data to be accumulated and registers with a [`Control`] instance to provide the framework's capabilities.
// ///
// /// Functionality for [`Holder`] is provided through [`HolderLocalKey`].
// #[derive(Debug)]
// pub struct Holder<T, U>(Holder0<T, U>)
// where
//     T: 'static;

// impl<T, U> Holder<T, U>
// where
//     U: 'static,
// {
//     /// Creates a new `Holder` instance with a function to initialize the data.
//     ///
//     /// The `make_data` function will be called by [`HolderLocalKey::init_data`].
//     pub fn new(make_data: fn() -> T) -> Self {
//         Self(Holder0::new(make_data))
//     }

//     /// Establishes link with control.
//     fn init_control(&self, control: &Control<T, U>, node: usize) {
//         self.0.init_control(&control.0, node)
//     }

//     fn init_data(&self) {
//         self.0.init_data()
//     }

//     fn ensure_initialized(&self, control: &Control<T, U>, node: usize) {
//         self.0.ensure_initialized(&control.0, node)
//     }

//     /// Invokes `f` on data. Panics if data is [`None`].
//     fn with_data<V>(&self, f: impl FnOnce(&T) -> V) -> V {
//         self.0.with_data(f)
//     }

//     /// Invokes `f` on data. Panics if data is [`None`].
//     fn with_data_mut<V>(&self, f: impl FnOnce(&mut T) -> V) -> V {
//         self.0.with_data_mut(f)
//     }
// }

//=================
// Implementation of HolderLocalKey.

impl<T, U> HolderLocalKey<T, Control<T, U>> for LocalKey<Holder<T, U>> {
    /// Establishes link with control.
    fn init_control(&'static self, control: &Control<T, U>) {
        self.with(|h| h.init_control(&control, addr_of_tl(self)))
    }

    /// Initializes [`Holder`] data.
    fn init_data(&'static self) {
        self.with(|h| h.init_data())
    }

    /// Ensures [`Holder`] is properly initialized by initializing it if not.
    fn ensure_initialized(&'static self, control: &Control<T, U>) {
        self.with(|h| h.ensure_initialized(&control, addr_of_tl(self)))
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

            unsafe { control.take_tls() };
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
