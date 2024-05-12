//! This module supports the collection and aggregation of the values of a designated thread-local variable
//! across threads (see package [overview and core concepts](crate)). The following features and constraints apply ...
//! - The designated thread-local variable may be defined and used in the thread responsible for
//! collection/aggregation.
//! - The values of linked thread-local variables are collected and aggregated into the [Control] object's
//! accumulated value when the thread-local variables are dropped following thread termination.
//! - The [`Control`] object's collection/aggregation functions may be executed at any time as they ensure
//! synchronization with the participating threads.
//! - After all participating threads other than the thread responsible for collection/aggregation have
//! terminated and EXPLICITLY joined, directly or indirectly, into the thread responsible for collection,
//! a call to one of the collection/aggregation functions will result in the final aggregated value.
//!
//! ## Usage pattern
//!
//! Here's an outline of how this little framework can be used:
//!
//! ```rust
//! use std::{
//!     ops::Deref,
//!     thread::{self, ThreadId},
//!     time::Duration,
//! };
//! use thread_local_collect::tlm::probed::{Control, Holder};
//!
//! // Define your data type, e.g.:
//! type Data = i32;
//!
//! // Define your accumulated value type.
//! type AccValue = i32;
//!
//! // Define your thread-local:
//! thread_local! {
//!     static MY_TL: Holder<Data, AccValue> = Holder::new();
//! }
//!
//! // Define your accumulation operation.
//! fn op(data: Data, acc: &mut AccValue, _: ThreadId) {
//!     *acc += data;
//! }
//!
//! // Create a function to update the thread-local value:
//! fn update_tl(value: Data, control: &Control<Data, AccValue>) {
//!     control.with_data_mut(|data| {
//!         *data = value;
//!     });
//! }
//!
//! fn main() {
//!     let control = Control::new(&MY_TL, 0, op);
//!
//!     update_tl(1, &control);
//!
//!     let h = thread::spawn({
//!         // Clone control for the new thread.
//!         let control = control.clone();
//!         move || {
//!             update_tl(10, &control);
//!             thread::sleep(Duration::from_millis(10));
//!             update_tl(20, &control);
//!         }
//!     });
//!
//!     // Wait for spawned thread to do some work.
//!     thread::sleep(Duration::from_millis(5));
//!
//!     // Probe the thread-local variables and get the accuulated value computed from
//!     // current thread-local values without updating the accumulated value in `control`.
//!     let acc = control.probe_tls();
//!     println!("non-final accumulated from probe_tls(): {}", acc);
//!
//!     h.join().unwrap();
//!
//!     // Probe the thread-local variables and get the accuulated value computed from
//!     // final thread-local values without updating the accumulated value in `control`.
//!     let acc = control.probe_tls();
//!     println!("final accumulated from probe_tls(): {}", acc);
//!
//!     // Take the final thread-local values and accumulate them in `control`.
//!     control.take_tls();
//!
//!     // Different ways to print the accumulated value in `control`.
//!
//!     println!("final accumulated={}", control.acc().deref());
//!
//!     let acc = control.acc();
//!     println!("final accumulated: {}", acc.deref());
//!     drop(acc);
//!
//!     control.with_acc(|acc| println!("final accumulated: {}", acc));
//!
//!     let acc = control.clone_acc();
//!     println!("final accumulated: {}", acc);
//!
//!     let acc = control.probe_tls();
//!     println!("final accumulated from probe_tls(): {}", acc);
//!
//!     let acc = control.take_acc(0);
//!     println!("final accumulated: {}", acc);
//! }
//! ````
//!
//! ## Other examples
//!
//! See another example at [`examples/tlm_probed_map_accumulator`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/tlm_probed_map_accumulator.rs).

use crate::tlm::{
    common::{
        ControlG, CoreParam, GDataParam, HolderG, NodeParam, SubStateParam, WithNode,
        POISONED_GUARDED_DATA_MUTEX,
    },
    tmap_d::TmapD,
};
use std::{
    marker::PhantomData,
    mem::take,
    ops::DerefMut,
    sync::{Arc, Mutex},
};

use super::common::{CtrlStateG, CtrlStateParam, HldrParam};

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

#[doc(hidden)]
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
pub type Control<T, U> = ControlG<P<T, U>, WithNode>;

impl<T, U> Control<T, U>
where
    T: 'static,
    U: 'static,
{
    /// Takes the values of any remaining linked thread-local-variables and aggregates those values
    /// with this object's accumulator, replacing those values with the evaluation of the `make_data` function
    /// passed to [`Holder::new`].
    ///
    /// Panics if `self`'s mutex is poisoned.
    /// Panics if [`Holder`] guarded data mutex is poisoned.
    pub fn take_tls(&self) {
        let mut guard = self.lock();
        // Need explicit deref_mut to avoid compilation error in for loop.
        let state = guard.deref_mut();
        for (tid, node) in state.s.tmap.iter() {
            log::trace!("executing `take_tls` for key={:?}", tid);
            let mut data_guard = node.data.lock().expect(POISONED_GUARDED_DATA_MUTEX);
            let data =
                take(data_guard.deref_mut()).expect("Holder data is always initialized before use");
            log::trace!("executed `take` -- `take_tls` for key={:?}", tid);
            log::trace!("executing `op` -- `take_tls` for key={:?}", tid);
            (self.op)(data, &mut state.acc, *tid);
        }
    }

    /// Collects the values of any remaining linked thread-local-variables, without changing those values,
    /// aggregates those values with a clone of this object's accumulator, and returns the aggregate
    /// value. This object's accumulator remains unchanged.
    ///
    /// Panics if `self`'s mutex is poisoned.
    /// Panics if [`Holder`] guarded data mutex is poisoned.
    pub fn probe_tls(&self) -> U
    where
        T: Clone,
        U: Clone,
    {
        let state = self.lock();
        let mut acc_clone = state.acc.clone();
        for (tid, node) in state.s.tmap.iter() {
            log::trace!("executing `probe_tls` for key={:?}", tid);
            let data = node
                .data
                .lock()
                .expect(POISONED_GUARDED_DATA_MUTEX)
                .clone()
                .expect("node guaranteed never to be None");
            log::trace!("executed `clone` -- `probe_tls` for key={:?}", tid);
            log::trace!("executing `op` -- `probe_tls` for key={:?}", tid);
            (self.op)(data, &mut acc_clone, *tid);
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
    use crate::test_support::{assert_eq_and_println, ThreadGater};
    use std::{
        collections::HashMap,
        fmt::Debug,
        ops::Deref,
        sync::Mutex,
        thread::{self, ThreadId},
    };

    #[derive(Debug, Clone, PartialEq)]
    struct Foo(String);

    type Data = HashMap<u32, Foo>;

    type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

    thread_local! {
        static MY_TL: Holder<Data, AccValue> = Holder::new();
    }

    fn insert_tl_entry(k: u32, v: Foo, control: &Control<Data, AccValue>) {
        control.with_data_mut(|data| data.insert(k, v));
    }

    fn op(data: HashMap<u32, Foo>, acc: &mut AccValue, tid: ThreadId) {
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
    fn own_thread_and_explicit_join() {
        let control = Control::new(&MY_TL, HashMap::new(), HashMap::new, op);

        let main_tid = thread::current().id();
        println!("main_tid={:?}", main_tid);

        let main_thread_gater = ThreadGater::new("main");
        let spawned_thread_gater = ThreadGater::new("spawned");

        let expected_acc_mutex = Mutex::new(HashMap::new());

        let assert_acc = |acc: &AccValue, msg: &str| {
            let exp_guard = expected_acc_mutex.try_lock().unwrap();
            let exp = exp_guard.deref();

            assert_eq_and_println(acc, exp, msg);
        };

        thread::scope(|s| {
            let h = s.spawn(|| {
                let spawned_tid = thread::current().id();
                println!("spawned tid={:?}", spawned_tid);

                let mut my_map = HashMap::<u32, Foo>::new();

                let process_value = |gate: u8,
                                     k: u32,
                                     v: Foo,
                                     my_map: &mut HashMap<u32, Foo>,
                                     assert_tl_msg: &str| {
                    main_thread_gater.wait_for(gate);
                    insert_tl_entry(k, v.clone(), &control);
                    my_map.insert(k, v);
                    assert_tl(my_map, assert_tl_msg, &control);

                    let mut exp = expected_acc_mutex.try_lock().unwrap();
                    op(my_map.clone(), &mut exp, spawned_tid);

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
                let acc = control.probe_tls();
                assert_eq_and_println(
                    &acc,
                    map.deref(),
                    "Accumulator after main thread inserts and probe_tls",
                );
                main_thread_gater.open(0);
            }

            {
                spawned_thread_gater.wait_for(0);
                let acc = control.probe_tls();
                assert_acc(
                    &acc,
                    "Accumulator after 1st spawned thread insert and probe_tls",
                );
                main_thread_gater.open(1);
            }

            {
                spawned_thread_gater.wait_for(1);
                control.take_tls();
                assert_acc(
                    control.acc().deref(),
                    "Accumulator after 2nd spawned thread insert and take_tls",
                );
                main_thread_gater.open(2);
            }

            {
                spawned_thread_gater.wait_for(2);
                let acc = control.probe_tls();
                assert_acc(
                    &acc,
                    "Accumulator after 3rd spawned thread insert and probe_tls",
                );
                main_thread_gater.open(3);
            }

            {
                // done with thread gaters
                h.join().unwrap();

                {
                    control.take_tls();
                    assert_acc(
                        control.acc().deref(),
                        "Accumulator after 4th spawned thread insert and take_tls",
                    );
                }

                {
                    control.take_tls();
                    assert_acc(control.acc().deref(), "Idempotency of control.take_tls()");
                }

                {
                    let acc = control.probe_tls();
                    assert_acc(&acc, "After take_tls(), probe_tls() the same acc value");
                }

                {
                    control.with_acc(|acc| {
                        assert_acc(
                            acc,
                            "Accumulator after 4th spawned thread insert, using control.with_acc()",
                        );
                    });
                }

                {
                    let acc = control.clone_acc();
                    assert_acc(
                        &acc,
                        "Accumulator after 4th spawned thread insert, using control.clone_acc()",
                    );
                }

                {
                    let acc = control.take_acc(HashMap::new());
                    assert_acc(
                        &acc,
                        "Accumulator after 4th spawned thread insert, using control.take_acc()",
                    );
                }

                {
                    control.with_acc(|acc| {
                        assert_eq_and_println(
                            acc,
                            &HashMap::new(),
                            "Accumulator after control.take_acc()",
                        );
                    });
                }
            }
        });
    }
}
