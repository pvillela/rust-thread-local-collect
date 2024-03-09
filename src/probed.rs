//! This module supports the collection and aggregation of the values of a designated thread-local variable
//! across threads. The following features and constraints apply ...
//! - The designated thread-local variable may be defined and used in the thread responsible for
//! collection/aggregation.
//! - The values of linked thread-local variables are collected and aggregated into the [Control] object's
//! accumulated value when the thread-local variables are dropped following thread termination.
//! - The [`Control`] object's collection/aggregation functions may be executed at any time as it ensures
//! synchronization with the participating threads. Thread-local values need to be initialized again
//! if used after a call to [`Control::take_tls`], but not after a call to [`Control::probe_tls`].
//! - After all participating threads other than the thread responsible for collection/aggregation have
//! terminated and EXPLICITLY joined, directly or indirectly, into the thread respnosible for collection,
//! a call to one of the collection/aggregation functions will result in the final aggregated value.
//!
//! See also [Core Concepts](super#core-concepts).
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
//! use thread_local_collect::probed::{Control, Holder, HolderLocalKey};
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
//!     MY_TL.ensure_initialized(control);
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
//!             thread::sleep(Duration::from_millis(10));
//!             update_tl(20, &control);
//!         });
//!
//!         {
//!             // Wait for spawned thread to do some work.
//!             thread::sleep(Duration::from_millis(5));
//!
//!             // Probe the thread-local variables and get the accuulated value computed from
//!             // current thread-local values without updating the accumulated value in `control`.
//!             let acc = control.probe_tls();
//!             println!("non-final accumulated from probe_tls(): {}", acc);
//!
//!             h.join().unwrap();
//!
//!             // Probe the thread-local variables and get the accuulated value computed from
//!             // final thread-local values without updating the accumulated value in `control`.
//!             let acc = control.probe_tls();
//!             println!("final accumulated from probe_tls(): {}", acc);
//!
//!             // Take the final thread-local values and accumulate them in `control`.
//!             control.take_tls();
//!
//!             // Different ways to print the accumulated value in `control`.
//!
//!             println!("final accumulated={}", control.acc().deref());
//!
//!             let acc = control.acc();
//!             println!("final accumulated: {}", acc.deref());
//!             drop(acc);
//!
//!             control.with_acc(|acc| println!("final accumulated: {}", acc));
//!
//!             let acc = control.clone_acc();
//!             println!("final accumulated: {}", acc);
//!
//!             let acc = control.probe_tls();
//!             println!("final accumulated from probe_tls(): {}", acc);
//!
//!             let acc = control.take_acc(0);
//!             println!("final accumulated: {}", acc);
//!         }
//!     });
//! }
//! ````
//!
//! ## Other examples
//!
//! See another example at [`examples/probed_map_accumulator.rs`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/probed_map_accumulator.rs).

pub use crate::common::HolderLocalKey;
use crate::common::{
    ControlG, CoreParam, GDataParam, HolderG, NodeParam, SubStateParam, TmapD, UseCtrlStateGDefault,
};
use std::{
    marker::PhantomData,
    ops::DerefMut,
    sync::{Arc, Mutex},
    thread::LocalKey,
};

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

impl<T, U> NodeParam for P<T, U> {
    type Node = Arc<Mutex<Option<T>>>;
}

impl<T, U> SubStateParam for P<T, U> {
    type SubState = TmapD<Self>;
}

impl<T, U> UseCtrlStateGDefault for P<T, U> {}

impl<T, U> GDataParam for P<T, U> {
    type GData = Arc<Mutex<Option<T>>>;
}

/// Specialization of [`ControlG`] for this module.
/// Controls the collection and accumulation of thread-local values linked to this object.
/// Such values, of type `T`, must be held in thread-locals of type [`Holder<T, U>`].
pub type Control<T, U> = ControlG<TmapD<P<T, U>>>;

impl<T, U> Control<T, U>
where
    T: 'static,
    U: 'static,
{
    /// Takes the values of any remaining linked thread-local-variables and aggregates those values
    /// with this object's accumulator, replacing those values with [`None`].
    pub fn take_tls(&self) {
        let mut guard = self.lock();
        // Need explicit deref_mut to avoid compilation error in for loop.
        let state = guard.deref_mut();
        for (tid, node) in state.s.tmap.iter() {
            log::trace!("executing `take_tls` for key={:?}", tid);
            let data = node.lock().unwrap().take();
            log::trace!("executed `take` -- `take_tls` for key={:?}", tid);
            if let Some(data) = data {
                log::trace!("executing `op` -- `take_tls` for key={:?}", tid);
                (self.op)(data, &mut state.acc, tid);
            }
        }
    }

    /// Collects the values of any remaining linked thread-local-variables, without changing those values,
    /// and aggregates those values with this object's accumulator.
    pub fn probe_tls(&self) -> U
    where
        T: Clone,
        U: Clone,
    {
        let state = self.lock();
        let mut acc_clone = state.acc.clone();
        for (tid, node) in state.s.tmap.iter() {
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

/// Specialization of [`HolderG`] for this module.
/// Holds thread-local data and a smart pointer to a [`Control`], enabling the linkage of the held data
/// with the control object.
pub type Holder<T, U> = HolderG<TmapD<P<T, U>>>;

//=================
// Implementation of HolderLocalKey.

impl<T, U> HolderLocalKey<TmapD<P<T, U>>> for LocalKey<Holder<T, U>> {
    /// Establishes link with control.
    fn init_control(&'static self, control: &Control<T, U>) {
        self.with(|h| {
            let node = h.data.clone();
            h.init_control_node(&control, node);
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
            h.ensure_initialized_node(&control, node);
        })
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
        static MY_FOO_MAP: Holder<Data, AccValue> = Holder::new(HashMap::new);
    }

    fn insert_tl_entry(k: u32, v: Foo, control: &Control<Data, AccValue>) {
        MY_FOO_MAP.ensure_initialized(control);
        MY_FOO_MAP.with_data_mut(|data| data.insert(k, v));
    }

    fn op(data: HashMap<u32, Foo>, acc: &mut AccValue, tid: &ThreadId) {
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
            assert_eq_and_println(map, other, msg);
        });
    }

    #[test]
    fn own_thread_and_explicit_join() {
        let control = Control::new(HashMap::new(), op);

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
                    assert_tl(my_map, assert_tl_msg);

                    let mut exp = expected_acc_mutex.try_lock().unwrap();
                    op(my_map.clone(), &mut exp, &spawned_tid);

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
                assert_tl(&my_map, "After main thread inserts");

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
