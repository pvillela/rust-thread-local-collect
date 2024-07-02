//! This module supports the collection and aggregation of the values of a designated thread-local variable
//! across threads (see package [overview and core concepts](crate)). The following features and constraints apply ...
//! - The designated thread-local variable may be used in the thread responsible for
//! collection/aggregation.
//! - The values of linked thread-local variables are collected and aggregated into the [Control] object's
//! accumulated value when the thread-local variables are dropped following thread termination.
//! - After all participating threads other than the thread responsible for collection/aggregation have
//! terminated and EXPLICITLY joined, directly or indirectly, into the thread responsible for collection,
//! a call to [`Control::take_own_tl`] followed by a call to one of the accumulator retrieval functions
//! will return the final aggregated value.
//!
//! ## Usage pattern

//! ```rust
#![doc = include_str!("../../examples/tlm_joined_i32_accumulator.rs")]
//! ````

//!
//! ## Other examples
//!
//! See another example at [`examples/tlm_joined_map_accumulator`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/tlm_joined_map_accumulator.rs).

pub use crate::tlm::common::{ControlG, HolderG};

use super::common::{Ctrl, CtrlParam, DefaultDiscr, HldrParam};
use crate::tlm::common::{
    CoreParam, CtrlStateG, CtrlStateParam, CtrlStateWithNode, GDataParam, New, NodeParam,
    SubStateParam, WithNode,
};
use std::{
    cell::RefCell,
    marker::PhantomData,
    mem::replace,
    ops::DerefMut,
    thread::{self, ThreadId},
};

//=================
// Core implementation based on common module

/// Parameter bundle that enables specialization of the common generic structs for this module.
#[derive(Debug)]
pub struct Joined<T, U> {
    own_tl_used: bool,
    tid: ThreadId,
    _t: PhantomData<T>,
    _u: PhantomData<U>,
}

type P<T, U> = Joined<T, U>;

impl<T, U> CoreParam for P<T, U> {
    type Dat = T;
    type Acc = U;
}

impl<T, U> NodeParam for P<T, U>
where
    T: 'static,
    U: 'static,
{
    type Node = ();
    type NodeFnArg = Control<T, U>;

    fn node_fn(_arg: &Self::NodeFnArg) -> Self::Node {}
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
            own_tl_used: false,
            tid: thread::current().id(),
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

impl<T, U> CtrlStateWithNode<P<T, U>> for CtrlState<T, U>
where
    T: 'static,
    U: 'static,
{
    fn register_node(&mut self, _node: (), tid: ThreadId) {
        if tid == self.s.tid {
            self.s.own_tl_used = true;
        }
    }
}

/// Specialization of [`ControlG`] for this module.
/// Controls the collection and accumulation of thread-local values linked to this object.
///
/// `T` is the type of the thread-local values and `U` is the type of the accumulated value.
/// The data values are held in thread-locals of type [`Holder<T, U>`].
pub type Control<T, U> = ControlG<P<T, U>>;

impl<T, U> Control<T, U>
where
    T: 'static,
    U: 'static,
{
    /// This method takes the value of the designated thread-local variable in the thread responsible for
    /// collection/aggregation (i.e., the thread where `self` is instantiated), if that variable is used, and
    /// aggregates that value with this object's accumulator, replacing that value with the evaluation of the
    /// `make_data` function passed to [`Control::new`].
    ///
    /// This object's accumulated value reflects the aggregation of all participating thread-local values when this
    /// method is called from the thread responsible for collection/aggregation after the other threads have terminated
    /// and explicitly joined, directly or indirectly, into the thread responsible for collection/aggregation.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    pub fn take_own_tl(&self) {
        let mut guard = self.lock();
        // Need explicit deref_mut to avoid compilation error in for loop.
        let state = guard.deref_mut();
        if state.s.own_tl_used {
            self.tl.with(|h| {
                let mut data_guard = h.data_guard();
                let data = replace(data_guard.deref_mut(), Some(self.make_data()));
                if let Some(data) = data {
                    log::trace!("`take_own_tl`: executing `op`");
                    (self.op)(data, &mut state.acc, thread::current().id());
                }
            });
        }
    }
}

/// Specialization of [`HolderG`] for this module.
/// Holds thread-local data of type `T` and a smart pointer to a [`Control<T, U>`], enabling the linkage of
/// the held data with the control object.
pub type Holder<T, U> = HolderG<P<T, U>, WithNode>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::dev_support::assert_eq_and_println;
    use std::{
        collections::HashMap,
        fmt::Debug,
        iter::once,
        ops::Deref,
        sync::RwLock,
        thread::{self, ThreadId},
        time::Duration,
    };

    // Avoid using active lock in an assertion. Otherwise, if the assertion fails,
    // we abort with a Mutex poison error without knowing what exactly went wrong.

    #[derive(Debug, Clone, PartialEq)]
    struct Foo(String);

    type Data = HashMap<i32, Foo>;

    type AccValue = HashMap<ThreadId, HashMap<i32, Foo>>;

    thread_local! {
        static MY_TL: Holder<Data, AccValue> = Holder::new();
    }

    fn insert_tl_entry(k: i32, v: Foo, control: &Control<Data, AccValue>) {
        println!("****** insert_tl_entry");
        control.with_data_mut(|data| data.insert(k, v));
    }

    fn op(data: HashMap<i32, Foo>, acc: &mut AccValue, tid: ThreadId) {
        println!("Executing `op` on data={data:?}");
        acc.entry(tid).or_default();
        for (k, v) in data {
            acc.get_mut(&tid).unwrap().insert(k, v.clone());
        }
    }

    fn assert_tl(other: &Data, msg: &str, control: &Control<Data, AccValue>) {
        let data = control.with_data(|data| data.clone());
        assert_eq!(&data, other, "{msg}");
    }

    fn make_data() -> HashMap<i32, Foo> {
        println!("***** executed make_data");
        HashMap::new()
    }

    #[test]
    fn explicit_joins_no_take_own_tl() {
        // These are directly defined as references to prevent the move closure below from moving
        // `control` and `spawned_tids`values. The closure has to be `move` because it needs to own `i`.
        let control = &Control::new(&MY_TL, HashMap::new(), make_data, op);
        let spawned_tids = &RwLock::new(vec![thread::current().id(), thread::current().id()]);

        thread::scope(|s| {
            let hs = (0..2)
                .map(|i| {
                    s.spawn({
                        move || {
                            let si = i.to_string();

                            let mut lock = spawned_tids.write().unwrap();
                            lock[i] = thread::current().id();
                            drop(lock);

                            insert_tl_entry(1, Foo("a".to_owned() + &si), control);

                            let other = HashMap::from([(1, Foo("a".to_owned() + &si))]);
                            assert_tl(&other, "After 1st insert", control);

                            insert_tl_entry(2, Foo("b".to_owned() + &si), control);

                            let other = HashMap::from([
                                (1, Foo("a".to_owned() + &si)),
                                (2, Foo("b".to_owned() + &si)),
                            ]);
                            assert_tl(&other, "After 2nd insert", control);
                        }
                    })
                })
                .collect::<Vec<_>>();

            {
                thread::sleep(Duration::from_millis(50));
                let spawned_tids = spawned_tids.try_read().unwrap();
                println!("spawned_tid={:?}", spawned_tids);
            }

            hs.into_iter().for_each(|h| h.join().unwrap());

            println!("after hs join: {:?}", control);
        });

        {
            let spawned_tids = spawned_tids.try_read().unwrap();
            let map_0 = HashMap::from([(1, Foo("a0".to_owned())), (2, Foo("b0".to_owned()))]);
            let map_1 = HashMap::from([(1, Foo("a1".to_owned())), (2, Foo("b1".to_owned()))]);
            let map = HashMap::from([(spawned_tids[0], map_0), (spawned_tids[1], map_1)]);

            {
                let guard = control.acc();
                let acc = guard.deref();
                assert!(acc.eq(&map), "Accumulator check: acc={acc:?}, map={map:?}");
            }
        }
    }

    #[test]
    fn own_thread_and_explicit_joins() {
        let control = Control::new(&MY_TL, HashMap::new(), HashMap::new, op);

        let tid_own = thread::current().id();
        println!("main_tid={:?}", tid_own);

        let map_own = {
            let value1 = Foo("a".to_owned());
            let value2 = Foo("b".to_owned());
            let map_own = HashMap::from([(1, value1.clone()), (2, value2.clone())]);

            insert_tl_entry(1, value1, &control);
            insert_tl_entry(2, value2, &control);
            assert_tl(&map_own, "After main thread inserts", &control);

            map_own
        };

        thread::sleep(Duration::from_millis(100));

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

        {
            control.take_own_tl();
            let acc1 = control.clone_acc();

            control.take_own_tl();
            let acc2 = control.clone_acc();

            assert_eq_and_println(&acc1, &acc2, "Idempotency of control.take_own_tl()");
        }

        // Different ways to get the accumulated value

        let map = once((tid_own, map_own))
            .chain(tid_map_pairs)
            .collect::<HashMap<_, _>>();

        {
            let acc = control.with_acc(|acc| acc.clone());
            assert_eq_and_println(&acc, &map, "with_acc");

            let acc = control.clone_acc();
            assert_eq_and_println(&acc, &map, "clone_acc");
        }

        // Call take_own_tl again.
        {
            control.take_own_tl();
            println!("After 2nd take_own_tl: control={control:?}");

            let acc = control.with_acc(|acc| acc.clone());
            assert_eq_and_println(&acc, &map, "2nd take_own_tl, with_acc");
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
            let map_own = {
                let value1 = Foo("c".to_owned());
                let value2 = Foo("d".to_owned());
                let map_own = HashMap::from([(11, value1.clone()), (22, value2.clone())]);

                insert_tl_entry(11, value1, &control);
                insert_tl_entry(22, value2, &control);

                map_own
            };

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

            control.take_own_tl();
            let map = HashMap::from([(tid_own, map_own), (tid_spawned, map_spawned)]);
            let acc = control.take_acc(HashMap::new());
            assert_eq_and_println(&acc, &map, "take_acc - control reused");
        }
    }
}
