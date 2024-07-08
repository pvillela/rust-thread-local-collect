//! This module supports the collection and aggregation of the values of a designated thread-local variable
//! across threads (see package [overview and core concepts](crate)), including the ability to inspect
//! the accumulated value before participating threads have terminated. The following capabilities and constraints apply ...
//! - The designated thread-local variable may be used in the thread responsible for
//! collection/aggregation.
//! - The values of linked thread-local variables are collected and aggregated into the [Control] object's
//! accumulated value when the thread-local variables are dropped following thread termination.
//! - The [`Control::probe_tls`] function can be called at any time to return a clone of the current aggregated value.
//! - After all participating threads other than the thread responsible for collection/aggregation have
//! terminated (joins are not necessary), a call to [`Control::take_tls`] followed by a call to one of the accumulator retrieval functions
//! will return the final aggregated value.
//!
//! ## Usage pattern

//! ```rust
#![doc = include_str!("../../examples/tlm_probed_i32_accumulator.rs")]
//! ````

//!
//! ## Other examples
//!
//! See another example at [`examples/tlm_probed_map_accumulator`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/tlm_probed_map_accumulator.rs).

pub use crate::tlm::common::{ControlG, HolderG};

use super::common::{CtrlParam, CtrlStateG, CtrlStateParam, HldrParam};
use crate::tlm::{
    common::{
        CoreParam, Ctrl, GDataParam, NodeParam, SubStateParam, WithNode,
        POISONED_GUARDED_DATA_MUTEX,
    },
    tmap_d::TmapD,
};
use std::{
    marker::PhantomData,
    mem::replace,
    ops::DerefMut,
    sync::{Arc, Mutex},
};

//=================
// Core implementation based on common module

/// Parameter bundle that enables specialization of the common generic structs for this module.
#[derive(Debug)]
pub struct Probed<T, U> {
    _t: PhantomData<T>,
    _u: PhantomData<U>,
}

type P<T, U> = Probed<T, U>;

impl<T, U> CoreParam for P<T, U> {
    type Dat = T;
    type Acc = U;
}

#[doc(hidden)]
#[derive(Debug)]
pub struct Node<T> {
    data: Arc<Mutex<Option<T>>>,
}

impl<T, U> NodeParam for P<T, U>
where
    T: 'static,
    U: 'static,
{
    type Node = Node<T>;
    type NodeFnArg = Control<T, U>;

    fn node_fn(arg: &Self::NodeFnArg) -> Self::Node {
        arg.tl.with(|h| Node {
            data: h.data.clone(),
        })
    }
}

impl<T, U> SubStateParam for P<T, U>
where
    T: 'static,
    U: 'static,
{
    type SubState = TmapD<Self>;
}

impl<T, U> GDataParam for P<T, U> {
    type GData = Arc<Mutex<Option<T>>>;
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

type CtrlState<T, U> = CtrlStateG<P<T, U>, TmapD<P<T, U>>>;

impl<T, U> CtrlStateParam for P<T, U>
where
    T: 'static,
    U: 'static,
{
    type CtrlState = CtrlState<T, U>;
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
    /// Takes the values of any remaining linked thread-local-variables and aggregates those values
    /// with this object's accumulator, replacing those values with the evaluation of the `make_data` function
    /// passed to [`Control::new`].
    ///
    /// This object's accumulated value reflects the aggregation of all participating thread-local values when this
    /// method is called from the thread responsible for collection/aggregation after the other threads have terminated.
    ///
    /// # Panics
    /// - If `self`'s mutex is poisoned.
    /// - If [`Holder`] guarded data mutex is poisoned.
    pub fn take_tls(&self) {
        let mut guard = self.lock();
        // Need explicit deref_mut to avoid compilation error in for loop.
        let state = guard.deref_mut();
        for (tid, node) in state.s.tmap.iter() {
            log::trace!("executing `take_tls` for key={:?}", tid);
            let mut data_guard = node.data.lock().expect(POISONED_GUARDED_DATA_MUTEX);
            let data = replace(data_guard.deref_mut(), Some(self.make_data()));
            if let Some(data) = data {
                log::trace!("executed `take` -- `take_tls` for key={:?}", tid);
                log::trace!("executing `op` -- `take_tls` for key={:?}", tid);
                (self.op)(data, &mut state.acc, *tid);
            }
        }
    }

    /// Collects the values of any remaining linked thread-local-variables, without changing those values,
    /// aggregates those values with a clone of this object's accumulator, and returns the aggregate
    /// value. This object's accumulator remains unchanged.
    ///
    /// # Panics
    /// - If `self`'s mutex is poisoned.
    /// - If [`Holder`] guarded data mutex is poisoned.
    pub fn probe_tls(&self) -> U
    where
        T: Clone,
        U: Clone,
    {
        let state = self.lock();
        let mut acc_clone = state.acc.clone();
        for (tid, node) in state.s.tmap.iter() {
            log::trace!("executing `probe_tls` for key={:?}", tid);
            let data = node.data.lock().expect(POISONED_GUARDED_DATA_MUTEX).clone();
            if let Some(data) = data {
                log::trace!("executing `op` -- `probe_tls` for key={:?}", tid);
                (self.op)(data, &mut acc_clone, *tid);
            }
        }
        acc_clone
    }
}

/// Specialization of [`HolderG`] for this module.
/// Holds thread-local data of type `T` and a smart pointer to a [`Control<T, U>`], enabling the linkage of
/// the held data with the control object.
pub type Holder<T, U> = HolderG<P<T, U>, WithNode>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Control, Holder};
    use crate::dev_support::{assert_eq_and_println, ThreadGater};
    use std::{
        collections::HashMap,
        fmt::Debug,
        iter::once,
        sync::Mutex,
        thread::{self, ThreadId},
    };

    #[derive(Debug, Clone, PartialEq)]
    struct Foo(String);

    type Data = HashMap<i32, Foo>;

    type AccValue = HashMap<ThreadId, HashMap<i32, Foo>>;

    thread_local! {
        static MY_TL: Holder<Data, AccValue> = Holder::new();
    }

    fn insert_tl_entry(k: i32, v: Foo, control: &Control<Data, AccValue>) {
        control.with_data_mut(|data| data.insert(k, v));
    }

    fn op(data: HashMap<i32, Foo>, acc: &mut AccValue, tid: ThreadId) {
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

    fn assert_tl(other: &Data, msg: &str, control: &Control<Data, AccValue>) {
        control.with_data(|map| {
            assert_eq_and_println(map, other, msg);
        });
    }

    #[test]
    fn unprobed_explicit_joins_no_take_tls() {
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

    #[test]
    fn unprobed_own_thread_and_explicit_joins() {
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
            control.take_tls();
            let acc1 = control.clone_acc();

            control.take_tls();
            let acc2 = control.clone_acc();

            assert_eq_and_println(&acc1, &acc2, "Idempotency of control.take_tls()");
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

        // Call take_tls again.
        {
            control.take_tls();
            println!("After 2nd take_tls: control={control:?}");

            let acc = control.with_acc(|acc| acc.clone());
            assert_eq_and_println(&acc, &map, "2nd take_tls, with_acc");
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

            control.take_tls();
            let map = HashMap::from([(tid_own, map_own), (tid_spawned, map_spawned)]);
            let acc = control.take_acc(HashMap::new());
            assert_eq_and_println(&acc, &map, "take_acc - control reused");
        }
    }

    #[test]
    fn probed_own_thread_and_explicit_join() {
        let control = Control::new(&MY_TL, HashMap::new(), HashMap::new, op);

        let main_tid = thread::current().id();
        println!("main_tid={:?}", main_tid);

        let main_thread_gater = ThreadGater::new("main");
        let spawned_thread_gater = ThreadGater::new("spawned");

        let expected_acc_mutex = Mutex::new(HashMap::new());

        let assert_acc = |acc: AccValue, msg: &str| {
            // Use clone to avoid possible assert panic while owing Mutex lock.
            let exp = expected_acc_mutex.try_lock().unwrap().clone();
            assert_eq_and_println(&acc, &exp, msg);
        };

        thread::scope(|s| {
            let h = s.spawn(|| {
                let spawned_tid = thread::current().id();
                println!("spawned tid={:?}", spawned_tid);

                let mut my_map = HashMap::<i32, Foo>::new();

                let process_value = |gate: u8,
                                     k: i32,
                                     v: Foo,
                                     my_map: &mut HashMap<i32, Foo>,
                                     assert_tl_msg: &str| {
                    main_thread_gater.wait_for(gate);
                    insert_tl_entry(k, v.clone(), &control);
                    my_map.insert(k, v);
                    assert_tl(my_map, assert_tl_msg, &control);

                    let mut exp_acc = expected_acc_mutex.try_lock().unwrap();
                    op(my_map.clone(), &mut exp_acc, spawned_tid);
                    drop(exp_acc);

                    spawned_thread_gater.open(gate);
                };

                process_value(
                    0,
                    1,
                    Foo("aa".to_owned()),
                    &mut my_map,
                    "After spawned thread 1st insert",
                );

                process_value(
                    1,
                    2,
                    Foo("bb".to_owned()),
                    &mut my_map,
                    "After spawned thread 2nd insert",
                );

                my_map = HashMap::new();
                process_value(
                    2,
                    3,
                    Foo("cc".to_owned()),
                    &mut my_map,
                    "After take_tls and spawned thread 3rd insert",
                );

                process_value(
                    3,
                    4,
                    Foo("dd".to_owned()),
                    &mut my_map,
                    "After spawned thread 4th insert",
                );
            });

            {
                insert_tl_entry(1, Foo("a".to_owned()), &control);
                insert_tl_entry(2, Foo("b".to_owned()), &control);
                let my_map = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
                assert_tl(&my_map, "After main thread inserts", &control);

                let mut map = expected_acc_mutex.try_lock().unwrap();
                map.insert(main_tid, my_map);
                let map = map.clone();
                let acc = control.probe_tls();
                assert_eq_and_println(
                    &acc,
                    &map,
                    "Accumulator after main thread inserts and probe_tls",
                );
                main_thread_gater.open(0);
            }

            {
                spawned_thread_gater.wait_for(0);
                let acc = control.probe_tls();
                assert_acc(
                    acc,
                    "Accumulator after 1st spawned thread insert and probe_tls",
                );
                main_thread_gater.open(1);
            }

            {
                spawned_thread_gater.wait_for(1);
                control.take_tls();
                let acc = control.clone_acc();
                assert_acc(
                    acc,
                    "Accumulator after 2nd spawned thread insert and take_tls",
                );
                main_thread_gater.open(2);
            }

            {
                spawned_thread_gater.wait_for(2);
                let acc = control.probe_tls();
                assert_acc(
                    acc,
                    "Accumulator after 3rd spawned thread insert and probe_tls",
                );
                main_thread_gater.open(3);
            }

            // done with thread gaters
            h.join().unwrap();
        });

        {
            control.take_tls();
            assert_acc(
                control.clone_acc(),
                "Accumulator after 4th spawned thread insert and take_tls",
            );
        }

        {
            control.take_tls();
            assert_acc(control.clone_acc(), "Idempotency of control.take_tls()");
        }

        {
            let acc = control.probe_tls();
            assert_acc(acc, "After take_tls(), probe_tls() the same acc value");
        }

        // Different ways to get the accumulated value

        {
            let acc = control.with_acc(|acc| acc.clone());
            assert_acc(
                acc,
                "Accumulator after 4th spawned thread insert, using control.with_acc()",
            );
        }

        {
            let acc = control.clone_acc();
            assert_acc(
                acc,
                "Accumulator after 4th spawned thread insert, using control.clone_acc()",
            );
        }

        // take_acc
        {
            let acc = control.take_acc(HashMap::new());
            assert_acc(
                acc,
                "Accumulator after 4th spawned thread insert, using control.take_acc()",
            );

            let acc = control.with_acc(|acc| acc.clone());
            assert_eq_and_println(
                &acc,
                &HashMap::new(),
                "Accumulator after control.take_acc()",
            );
        }
    }
}
