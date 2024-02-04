// //! This module supports the collection and aggregation of the values of a designated thread-local variable
// //! across threads. It is a simplified version of the [`crate::joined`] module which does not rely on
// //! unsafe code. The following constraints apply ...
// //! - The designated thread-local variable should NOT be used in the thread responsible for
// //! collection/aggregation. If this condition is violated, the thread-local value on that thread will NOT
// //! be collected and aggregated.
// //! - The values of linked thread-local variables are collected and aggregated into the [Control] object's
// //! accumulated value when the thread-local variables are dropped following thread termination.
// //! - The aggregated value is reflective of all participating threads if and only if it is accessed after
// //! all participating theads have
// //! terminated and EXPLICITLY joined directly or indirectly into the thread respnosible for collection.
// //! - Implicit joins by scoped threads are NOT correctly handled as the aggregation relies on the destructors
// //! of thread-local variables and such a destructor is not guaranteed to have executed at the point of the
// //! implicit join of a scoped thread.

// use crate::common::{ControlG, ControlStateG, CoreParam, HolderG, HolderLocalKey};
// use std::{
//     cell::{Ref, RefCell},
//     marker::PhantomData,
//     sync::{Arc, Mutex},
//     thread::{self, LocalKey, ThreadId},
// };

// //=================
// // Core implementation based on common module

// #[derive(Debug)]
// pub struct SimpleP<T, U>(PhantomData<T>, PhantomData<U>);

// impl<T, U> CoreParam for SimpleP<T, U> {
//     type Dat = T;
//     type Acc = U;
//     type Node = ();
//     type GData = RefCell<Option<T>>;
//     type Discr = PhantomData<Self>;
// }

// type SimpleState<T, U> = ControlStateG<SimpleP<T, U>>;

// pub type Control<T, U> = ControlG<SimpleP<T, U>>;

// impl<T, U> Control<T, U>
// where
//     T: 'static,
//     U: 'static,
// {
//     pub fn new(acc_base: U, op: impl Fn(T, &mut U, &ThreadId) + 'static + Send + Sync) -> Self {
//         Control {
//             state: Arc::new(Mutex::new(SimpleState {
//                 acc: acc_base,
//                 d0: PhantomData,
//             })),
//             op: Arc::new(op),
//         }
//     }

//     /// Used by a `Holder` to notify its `Control` that the holder's data has been dropped.
//     fn tl_data_dropped(&self, data: Option<T>, tid: &ThreadId) {
//         let mut lock = self.lock();
//         let acc = &mut lock.acc;
//         if let Some(data) = data {
//             (self.op)(data, acc, tid);
//         }
//     }
// }

// pub type Holder<T, U> = HolderG<SimpleP<T, U>>;

// impl<T, U: 'static> Holder<T, U> {
//     /// Establishes link with control.
//     fn init_control_fn(this: &Self, control: &Control<T, U>, _node: ()) {
//         let mut ctrl_ref = this.control.borrow_mut();
//         *ctrl_ref = Some(control.clone());
//     }

//     fn drop_data_fn(this: &Self) {
//         let mut data_guard = this.data_guard();
//         let data = data_guard.take();
//         let control = this.control();
//         if control.is_none() {
//             return;
//         }
//         let control = Ref::map(control, |x| x.as_ref().unwrap());
//         control.tl_data_dropped(data, &thread::current().id());
//     }

//     /// Creates a new `Holder` instance with a function to initialize the data.
//     ///
//     /// The `make_data` function will be called lazily when data is first accessed to initialize
//     /// the inner data value.
//     pub fn new(make_data: fn() -> T) -> Self {
//         Self {
//             data: RefCell::new(None),
//             control: RefCell::new(None),
//             make_data,
//             init_control_fn: Self::init_control_fn,
//             drop_data_fn: Self::drop_data_fn,
//         }
//     }
// }

// //=================
// // Implementation of HolderLocalKey.

// impl<T, U> HolderLocalKey<SimpleP<T, U>> for LocalKey<Holder<T, U>> {
//     /// Establishes link with control.
//     fn init_control(&'static self, control: &Control<T, U>) {
//         self.with(|h| h.init_control(&control, ()))
//     }

//     /// Initializes [`Holder`] data.
//     fn init_data(&'static self) {
//         self.with(|h| h.init_data())
//     }

//     /// Ensures [`Holder`] is properly initialized by initializing it if not.
//     fn ensure_initialized(&'static self, control: &Control<T, U>) {
//         self.with(|h| h.ensure_initialized(&control, ()))
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
//         ops::Deref,
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
//                 let guard = control.acc();
//                 let acc = guard.deref();
//                 assert!(acc.eq(&map), "Accumulator check: acc={acc:?}, map={map:?}");
//             }
//         }
//     }
// }
