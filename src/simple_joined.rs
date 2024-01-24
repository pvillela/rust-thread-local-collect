// //! This module supports the collection and aggregation of the values of a designated thread-local variable
// //! across threads. It is a simplified version of the [`crate::joined`] module which does not rely on
// //! unsafe code. The following constraints apply ...
// //! - The designated thread-local variable should NOT be used in the thread responsible for
// //! collection/aggregation. If this condition is violated, the thread-local value on that thread will NOT
// //! be collected and aggregated.
// //! - The collection/aggregation functionality occurs as a result of the execution of the destructors of the
// //! participating threads.
// //! - The aggregated value is reflective of all participating threads if and only if it is accessed after
// //! all participating theads have
// //! terminated and EXPLICITLY joined directly or indirectly into the thread respnosible for collection.
// //! - Implicit joins by scoped threads are NOT correctly handled as the aggregation relies on the destructors
// //! of thread-local variables and such a destructor is not guaranteed to have executed at the point of the
// //! implicit join of a scoped thread.

// use crate::common::{AccGuardS, ControlS, ControlStateS, HolderLocalKey, HolderS};
// use std::{
//     cell::RefCell,
//     ops::Deref,
//     sync::{Arc, Mutex},
//     thread::{LocalKey, ThreadId},
// };

// //=================
// // Core implementation based on common module

// type TrivialState<T, U> = ControlStateS<T, U, ()>;

// impl<T, U> TrivialState<T, U> {
//     fn take_tls(_state: &mut Self, _op: &(dyn Fn(T, &mut U, &ThreadId) + Send + Sync)) {}
// }

// type Control0<T, U> = ControlS<TrivialState<T, U>>;

// impl<T, U> Control0<T, U>
// where
//     T: 'static,
//     U: 'static,
// {
//     fn new(acc_base: U, op: impl Fn(T, &mut U, &ThreadId) + 'static + Send + Sync) -> Self {
//         Control0 {
//             state: Arc::new(Mutex::new(TrivialState::new(
//                 acc_base,
//                 TrivialState::take_tls,
//             ))),
//             op: Arc::new(op),
//         }
//     }
// }

// type Holder0<T, U> = HolderS<RefCell<Option<T>>, TrivialState<T, U>>;

// impl<T, U: 'static> Holder0<T, U> {
//     /// Creates a new `Holder` instance with a function to initialize the data.
//     ///
//     /// The `make_data` function will be called lazily when data is first accessed to initialize
//     /// the inner data value.
//     fn new(make_data: fn() -> T) -> Self {
//         Self {
//             data: RefCell::new(None),
//             control: RefCell::new(None),
//             make_data,
//         }
//     }
// }

// //=================
// // Public wrappers

// /// Guard object of a [`Control`]'s `acc` field.
// #[derive(Debug)]
// pub struct AccGuard<'a, T, U>(AccGuardS<'a, TrivialState<T, U>>);

// impl<'a, T, U> Deref for AccGuard<'a, T, U> {
//     type Target = U;

//     fn deref(&self) -> &Self::Target {
//         self.0.deref()
//     }
// }

// /// Contains an accumulation operation `op`
// /// and an accumulated value `acc`. The accumulated value is updated by applying `op` to each thread-local
// /// data value and `acc` when the thread-local value is dropped.
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
//     /// Limitations:
//     /// - The aggregated value is reflective of all participating threads if and only if it is accessed after
//     /// all participating theads have
//     /// terminated and EXPLICITLY joined directly or indirectly into the thread respnosible for collection.
//     /// - Implicit joins by scoped threads are NOT correctly handled as the aggregation relies on the destructors
//     /// of thread-local variables and such a destructor is not guaranteed to have executed at the point of the
//     /// implicit join of a scoped thread.
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
//     /// Limitations:
//     /// - The aggregated value is reflective of all participating threads if and only if it is accessed after
//     /// all participating theads have
//     /// terminated and EXPLICITLY joined directly or indirectly into the thread respnosible for collection.
//     /// - Implicit joins by scoped threads are NOT correctly handled as the aggregation relies on the destructors
//     /// of thread-local variables and such a destructor is not guaranteed to have executed at the point of the
//     /// implicit join of a scoped thread.
//     ///
//     /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
//     ///
//     /// See also [`with_acc`](Self::with_acc) and [`ControlLock::acc`].
//     pub fn take_acc(&self, replacement: U) -> U {
//         self.0.take_acc(replacement)
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
//     fn init_control(&self, control: &Control<T, U>) {
//         self.0.init_control(&control.0, ())
//     }

//     fn init_data(&self) {
//         self.0.init_data()
//     }

//     fn ensure_initialized(&self, control: &Control<T, U>) {
//         self.0.ensure_initialized(&control.0, ())
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

// //=================
// // Implementation of HolderLocalKey.

// impl<T, U> HolderLocalKey<T, Control<T, U>> for LocalKey<Holder<T, U>> {
//     /// Establishes link with control.
//     fn init_control(&'static self, control: &Control<T, U>) {
//         self.with(|h| h.init_control(&control))
//     }

//     /// Initializes [`Holder`] data.
//     fn init_data(&'static self) {
//         self.with(|h| h.init_data())
//     }

//     /// Ensures [`Holder`] is properly initialized by initializing it if not.
//     fn ensure_initialized(&'static self, control: &Control<T, U>) {
//         self.with(|h| h.ensure_initialized(&control))
//     }

//     /// Invokes `f` on [`Holder`] data. Panics if data is [`None`].
//     fn with_data<V>(&'static self, f: impl FnOnce(&T) -> V) -> V {
//         self.with(|h| h.with_data(f))
//     }

//     /// Invokes `f` on [`Holder`] data. Panics if data is [`None`].
//     fn with_data_mut<V>(&'static self, f: impl FnOnce(&mut T) -> V) -> V {
//         self.with(|h| h.with_data_mut(f))
//     }
// }

// #[cfg(test)]
// mod tests {
//     use super::{Control, Holder, HolderLocalKey};

//     use std::{
//         collections::HashMap,
//         fmt::Debug,
//         sync::RwLock,
//         thread::{self, ThreadId},
//         time::Duration,
//     };

//     #[derive(Debug, Clone, PartialEq)]
//     struct Foo(String);

//     type Data = HashMap<u32, Foo>;

//     type AccumulatorMap = HashMap<ThreadId, HashMap<u32, Foo>>;

//     thread_local! {
//         static MY_FOO_MAP: Holder<Data, AccumulatorMap> = Holder::new(HashMap::new);
//     }

//     fn insert_tl_entry(k: u32, v: Foo, control: &Control<Data, AccumulatorMap>) {
//         MY_FOO_MAP.ensure_initialized(control);
//         MY_FOO_MAP.with_data_mut(|data| data.insert(k, v));
//     }

//     fn op(data: HashMap<u32, Foo>, acc: &mut AccumulatorMap, tid: &ThreadId) {
//         println!(
//             "`op` called from {:?} with data {:?}",
//             thread::current().id(),
//             data
//         );

//         acc.entry(tid.clone()).or_insert_with(|| HashMap::new());
//         for (k, v) in data {
//             acc.get_mut(tid).unwrap().insert(k, v.clone());
//         }
//     }

//     fn assert_tl(other: &Data, msg: &str) {
//         MY_FOO_MAP.with_data(|map| {
//             assert_eq!(map, other, "{msg}");
//         });
//     }

//     #[test]
//     fn test_all() {
//         let control = Control::new(HashMap::new(), op);
//         let spawned_tids = RwLock::new(vec![thread::current().id(), thread::current().id()]);

//         thread::scope(|s| {
//             let hs = (0..2)
//                 .map(|i| {
//                     s.spawn({
//                         // These are to prevent the move closure from moving `control` and `spawned_tids`.
//                         // The closure has to be `move` because it needs to own `i`.
//                         let control = &control;
//                         let spawned_tids = &spawned_tids;

//                         move || {
//                             let si = i.to_string();

//                             let mut lock = spawned_tids.write().unwrap();
//                             lock[i] = thread::current().id();
//                             drop(lock);

//                             insert_tl_entry(1, Foo("a".to_owned() + &si), control);

//                             let other = HashMap::from([(1, Foo("a".to_owned() + &si))]);
//                             assert_tl(&other, "After 1st insert");

//                             insert_tl_entry(2, Foo("b".to_owned() + &si), control);

//                             let other = HashMap::from([
//                                 (1, Foo("a".to_owned() + &si)),
//                                 (2, Foo("b".to_owned() + &si)),
//                             ]);
//                             assert_tl(&other, "After 2nd insert");
//                         }
//                     })
//                 })
//                 .collect::<Vec<_>>();

//             thread::sleep(Duration::from_millis(50));

//             let spawned_tids = spawned_tids.try_read().unwrap();
//             println!("spawned_tid={:?}", spawned_tids);

//             hs.into_iter().for_each(|h| h.join().unwrap());

//             println!("after hs join: {:?}", control);
//         });

//         {
//             let spawned_tids = spawned_tids.try_read().unwrap();
//             let map_0 = HashMap::from([(1, Foo("a0".to_owned())), (2, Foo("b0".to_owned()))]);
//             let map_1 = HashMap::from([(1, Foo("a1".to_owned())), (2, Foo("b1".to_owned()))]);
//             let map = HashMap::from([
//                 (spawned_tids[0].clone(), map_0),
//                 (spawned_tids[1].clone(), map_1),
//             ]);

//             {
//                 let acc = control.acc();
//                 assert!(acc.eq(&map), "Accumulator check: acc={acc:?}, map={map:?}");
//             }
//         }
//     }
// }
