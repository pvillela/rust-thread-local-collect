//! This module supports the collection and aggregation of the values of a designated thread-local variable
//! across threads (see package [overview and core concepts](crate)). It is a simplified version of the [`super::joined`]
//! module which does not support accumulating the thread-local value on the thread responsible for
//! collection/aggregation. The following constraints apply ...
//! - The designated thread-local variable should NOT be used in the thread responsible for
//! collection/aggregation. If this condition is violated, the thread-local value on that thread will NOT
//! be collected and aggregated.
//! - The values of linked thread-local variables are collected and aggregated into the [Control] object's
//! accumulated value when the thread-local variables are dropped following thread termination.
//! - After all participating threads other than the thread responsible for collection/aggregation have
//! terminated and EXPLICITLY joined, directly or indirectly, into the thread responsible for collection,
//! a call to one of the accumulator retrieval functions
//! will return the final aggregated value.
//!
//! ## Usage pattern

//! ```rust
#![doc = include_str!("../../examples/tlm_simple_joined_i32_accumulator.rs")]
//! ````

//!
//! ## Other examples
//!
//! See another example at [`examples/tlm_simple_joined_map_accumulator`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/tlm_simple_joined_map_accumulator.rs).

pub use crate::tlm::common::{ControlG, HolderG};

use super::common::{CtrlParam, DefaultDiscr, HldrParam};
use crate::tlm::common::{CoreParam, CtrlStateG, CtrlStateParam, GDataParam, New, SubStateParam};
use std::{cell::RefCell, marker::PhantomData};

//=================
// Core implementation based on common module

/// Parameter bundle that enables specialization of the common generic structs for this module.
#[derive(Debug)]
pub struct SimpleJoined<T, U> {
    _t: PhantomData<T>,
    _u: PhantomData<U>,
}

type P<T, U> = SimpleJoined<T, U>;

impl<T, U> CoreParam for P<T, U> {
    type Dat = T;
    type Acc = U;
}

impl<T, U> SubStateParam for P<T, U> {
    type SubState = Self;
}

impl<T, U> GDataParam for P<T, U> {
    type GData = RefCell<Option<T>>;
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

impl<T, U> CtrlParam for P<T, U>
where
    T: 'static,
    U: 'static,
{
    type Ctrl = Control<T, U>;
}

impl<T, U> HldrParam for P<T, U>
where
    T: 'static,
    U: 'static,
{
    type Hldr = Holder<T, U>;
}

type CtrlState<T, U> = CtrlStateG<P<T, U>, DefaultDiscr>;

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
pub type Holder<T, U> = HolderG<P<T, U>, DefaultDiscr>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Control, Holder};
    use crate::dev_support::assert_eq_and_println;
    use std::{
        collections::HashMap,
        fmt::Debug,
        thread::{self, ThreadId},
    };

    #[derive(Debug, Clone, PartialEq)]
    struct Foo(String);

    type Data = HashMap<i32, Foo>;

    type AccumulatorMap = HashMap<ThreadId, HashMap<i32, Foo>>;

    thread_local! {
        static MY_TL: Holder<Data, AccumulatorMap> = Holder::new();
    }

    fn insert_tl_entry(k: i32, v: Foo, control: &Control<Data, AccumulatorMap>) {
        control.with_data_mut(|data| data.insert(k, v));
    }

    fn op(data: HashMap<i32, Foo>, acc: &mut AccumulatorMap, tid: ThreadId) {
        println!(
            "`op` called from {:?} with data {:?}",
            thread::current().id(),
            data
        );

        acc.entry(tid).or_default();
        for (k, v) in data {
            acc.get_mut(&tid).unwrap().insert(k, v.clone());
        }
    }

    fn assert_tl(other: &Data, msg: &str, control: &Control<Data, AccumulatorMap>) {
        control.with_data(|map| {
            assert_eq!(map, other, "{msg}");
        });
    }

    #[test]
    fn test_all() {
        let control = Control::new(&MY_TL, HashMap::new(), HashMap::new, op);

        let tid_map_pairs = thread::scope(|s| {
            let hs = (0..2)
                .map(|i| {
                    let value1 = Foo("a".to_owned() + &i.to_string());
                    let value2 = Foo("a".to_owned() + &i.to_string());
                    let map_i = HashMap::from([(1, value1.clone()), (2, value2.clone())]);

                    s.spawn(|| {
                        insert_tl_entry(1, value1.clone(), &control);
                        let other = HashMap::from([(1, value1)]);
                        assert_tl(&other, "After 1st insert", &control);

                        insert_tl_entry(2, value2, &control);
                        assert_tl(&map_i, "After 2nd insert", &control);

                        let tid_spawned = thread::current().id();
                        (tid_spawned, map_i)
                    })
                })
                .collect::<Vec<_>>(); // needed to force threads to launch because Iterator is lazy
            hs.into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });

        // Different ways to get the accumulated value

        let map = tid_map_pairs.into_iter().collect::<HashMap<_, _>>();

        {
            let acc = control.with_acc(|acc| acc.clone());
            assert_eq_and_println(&acc, &map, "with_acc");

            let acc = control.clone_acc();
            assert_eq_and_println(&acc, &map, "clone_acc");
        }

        // take_acc
        {
            let acc = control.take_acc(HashMap::new());
            assert_eq_and_println(&acc, &map, "take_acc");

            let acc = control.take_acc(HashMap::new());
            assert_eq_and_println(&acc, &HashMap::new(), "2nd take_acc");
        }

        // Control reused.
        {
            let (tid_spawned, map_spawned) = thread::scope(|s| {
                let control = &control;

                let value1 = Foo("x".to_owned());
                let value2 = Foo("y".to_owned());
                let map_spawned = HashMap::from([(11, value1.clone()), (22, value2.clone())]);

                let tid = s
                    .spawn(move || {
                        insert_tl_entry(11, value1, control);
                        insert_tl_entry(22, value2, control);
                        thread::current().id()
                    })
                    .join()
                    .unwrap();

                (tid, map_spawned)
            });

            let map = HashMap::from([(tid_spawned, map_spawned)]);
            let acc = control.take_acc(HashMap::new());
            assert_eq_and_println(&acc, &map, "take_acc - control reused");
        }
    }
}
