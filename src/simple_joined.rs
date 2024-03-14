//! This module supports the collection and aggregation of the values of a designated thread-local variable
//! across threads (see package [overfiew and core concepts](super)). It is a simplified version of the [`crate::joined`] module which does not rely on
//! unsafe code. The following constraints apply ...
//! - The designated thread-local variable should NOT be used in the thread responsible for
//! collection/aggregation. If this condition is violated, the thread-local value on that thread will NOT
//! be collected and aggregated.
//! - The values of linked thread-local variables are collected and aggregated into the [Control] object's
//! accumulated value when the thread-local variables are dropped following thread termination.
//! - The aggregated value is reflective of all participating threads if and only if it is accessed after
//! all participating threads have
//! terminated and EXPLICITLY joined directly or indirectly into the thread respnosible for collection.
//! - Implicit joins by scoped threads are NOT correctly handled as the aggregation relies on the destructors
//! of thread-local variables and such a destructor is not guaranteed to have executed at the point of the
//! implicit join of a scoped thread.
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
//! use thread_local_collect::simple_joined::{Control, Holder, HolderLocalKey};
//!
//! // Define your data type, e.g.:
//! type Data = i32;
//!
//! // Define your accumulated value type.
//! type AccValue = i32;
//!
//! // Define your thread-local:
//! thread_local! {
//!     static MY_TL: Holder<Data, AccValue> = Holder::new(|| 0);
//! }
//!
//! // Define your accumulation operation.
//! fn op(data: Data, acc: &mut AccValue, _: &ThreadId) {
//!     *acc += data;
//! }
//!
//! // Create a function to update the thread-local value:
//! fn update_tl(value: Data, control: &Control<Data, AccValue>) {
//!     MY_TL.ensure_linked(control);
//!     MY_TL.with_data_mut(|data| {
//!         *data = value;
//!     });
//! }
//!
//! fn main() {
//!     let control = Control::new(0, op);
//!
//!     update_tl(1, &control);
//!
//!     thread::scope(|s| {
//!         let h = s.spawn(|| {
//!             update_tl(10, &control);
//!         });
//!         h.join().unwrap();
//!     });
//!
//!     {
//!         // Different ways to print the accumulated value
//!
//!         println!("accumulated={}", control.acc().deref());
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
//! ```
//!
//! ## Other examples
//!
//! See another example at [`examples/simple_joined_map_accumulator.rs`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/simple_joined_map_accumulator.rs).

pub use crate::common::HolderLocalKey;
use crate::common::{
    ControlG, CoreParam, CtrlStateG, CtrlStateParam, GDataParam, HolderG, New, SubStateParam,
    UseCtrlStateGDefault,
};
use std::{cell::RefCell, marker::PhantomData, thread::LocalKey};

//=================
// Core implementation based on common module

/// Parameter bundle that enables specialization of the common generic structs for this module.
#[derive(Debug)]
pub struct P<T, U> {
    _t: PhantomData<T>,
    _u: PhantomData<U>,
}

impl<T, U> CoreParam for P<T, U> {
    type Dat = T;
    type Acc = U;
}

impl<T, U> SubStateParam for P<T, U> {
    type SubState = Self;
}

impl<T, U> UseCtrlStateGDefault for P<T, U> {}

impl<T, U> GDataParam for P<T, U> {
    type GData = RefCell<T>;
}

impl<T, U> New<P<T, U>> for P<T, U> {
    type Arg = ();

    fn new(_: ()) -> P<T, U> {
        Self {
            _t: PhantomData,
            _u: PhantomData,
        }
    }
}

type CtrlState<T, U> = CtrlStateG<P<T, U>>;

impl<T, U> CtrlStateParam for P<T, U> {
    type CtrlState = CtrlState<T, U>;
}

/// Specialization of [`ControlG`] for this module.
/// Controls the collection and accumulation of thread-local values linked to this object.
///
/// `T` is the type of the thread-local values and `U` is the type of the accumulated value.
/// The data values are held in thread-locals of type [`Holder<T, U>`].
pub type Control<T, U> = ControlG<P<T, U>>;

/// Specialization of [`HolderG`] for this module.
/// Holds thread-local data of type `T` and a smart pointer to a [`Control<T, U>`], enabling the linkage of
/// the held data with the control object.
pub type Holder<T, U> = HolderG<P<T, U>>;

//=================
// Implementation of HolderLocalKey.

impl<T, U> HolderLocalKey<P<T, U>> for LocalKey<Holder<T, U>> {
    /// Ensures [`Holder`] is properly initialized by initializing it if not.
    fn ensure_linked(&'static self, control: &Control<T, U>) {
        self.with(|h| h.ensure_linked(&control))
    }

    /// Invokes `f` on [`Holder`] data. Panics if data is [`None`].
    fn with_data<V>(&'static self, f: impl FnOnce(&T) -> V) -> V {
        self.with(|h| h.with_data(f))
    }

    /// Invokes `f` mutably on [`Holder`] data. Panics if data is [`None`].
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
        MY_FOO_MAP.ensure_linked(control);
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
    fn test_all() {
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
}
