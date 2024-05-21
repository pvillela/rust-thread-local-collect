//! Variant of module [`crate::tlm::probed`] with a `send` API similar to that of [`crate::tlcr::probed`].
//!
//! This module supports the collection and aggregation of values across threads (see package
//! [overview and core concepts](crate)), including the ability to inspect
//! the accumulated value before participating threads have terminated. The following features and constraints apply ...
//! - Values may be collected from the thread responsible for collection/aggregation.
//! - The participating threads *send* data to a clonable `control` object instance that aggregates the values.
//! - The [`Control::drain_tls`] function can be called to return the accumulated value after all participating
//! threads have terminated and EXPLICITLY joined, directly or indirectly, into the thread responsible for collection.
//! - The [`Control::probe_tls`] function can be called at any time to return a clone of the current aggregated value.
//!
//! ## Usage pattern
//!
//! Here's an outline of how this little framework can be used:
//!
//! ```rust
//! use std::{
//!     thread::{self, ThreadId},
//!     time::Duration,
//! };
//! use thread_local_collect::tlm::send::probed::{Control, Holder};
//!
//! // Define your data type, e.g.:
//! type Data = i32;
//!
//! // Define your accumulated value type.
//! type AccValue = i32;
//!
//! // Define your zero accumulated value function.
//! fn acc_zero() -> AccValue {
//!     0
//! }
//!
//! // Define your accumulation operation.
//! fn op(data: Data, acc: &mut AccValue, _: ThreadId) {
//!     *acc += data;
//! }
//!
//! // Define your accumulor reduction operation.
//! fn op_r(acc1: AccValue, acc2: AccValue) -> AccValue {
//!     acc1 + acc2
//! }
//!
//! thread_local! {
//!     static MY_TL: Holder<AccValue> = Holder::new();
//! }
//!
//! fn main() {
//!     // Instantiate the control object.
//!     let mut control = Control::new(&MY_TL, acc_zero, op, op_r);
//!
//!     // Send data to control from main thread if desired.
//!     control.send_data(1);
//!
//!     let h = thread::spawn({
//!         // Clone control for use in the new thread.
//!         let control = control.clone();
//!         move || {
//!             control.send_data(10);
//!             thread::sleep(Duration::from_millis(10));
//!             control.send_data(20);
//!         }
//!     });
//!
//!     // Wait for spawned thread to do some work.
//!     thread::sleep(Duration::from_millis(5));
//!
//!     // Probe the thread-local values and get the accuulated value computed from
//!     // current thread-local values.
//!     let acc = control.probe_tls();
//!     println!("non-final accumulated from probe_tls(): {}", acc);
//!
//!     h.join().unwrap();
//!
//!     // Probe the thread-local variables and get the accuulated value computed from
//!     // final thread-local values.
//!     let acc = control.probe_tls();
//!     println!("final accumulated from probe_tls(): {}", acc);
//!
//!     // Drain the final thread-local values.
//!     let acc = control.drain_tls();
//!
//!     // Print the accumulated value
//!     println!("accumulated={acc}");
//! }
//! ````
//!
//! ## Other examples
//!
//! See another example at [`examples/tlmsend_probed_map_accumulator`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/tlmsend_probed_map_accumulator.rs).

use super::control_send::{ControlSendG, WithTakeTls};
use crate::tlm::probed::{Control as ControlInner, Holder as HolderInner, P};

/// Specialization of [`ControlSendG`] for this module.
/// Controls the collection and accumulation of thread-local values linked to this object.
///
/// `T` is the type of the data sent from threads for accumulation and `U` is the type of the accumulated value.
/// Partially accumulated values are held in thread-locals of type [`Holder<U>`].
pub type Control<T, U> = ControlSendG<P<U, Option<U>>, T, U>;

impl<T, U> WithTakeTls<P<U, Option<U>>, U> for Control<T, U>
where
    U: 'static,
{
    fn take_tls(control: &ControlInner<U, Option<U>>) {
        control.take_tls();
    }
}

impl<T, U> Control<T, U>
where
    U: Clone,
{
    pub fn probe_tls(&self) -> U {
        self.control
            .probe_tls()
            .expect("accumulator guaranteed to never be None")
    }
}

/// Specialization of [`crate::tlm::joined::Holder`] for this module.
/// Holds thread-local partially accumulated data of type `U` and a smart pointer to a [`Control<T, U>`],
/// enabling the linkage of the held data with the control object.
pub type Holder<U> = HolderInner<U, Option<U>>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Control, Holder};
    use crate::test_support::{assert_eq_and_println, ThreadGater};
    use std::{
        collections::HashMap,
        fmt::Debug,
        sync::{Mutex, RwLock},
        thread::{self, ThreadId},
    };

    #[derive(Debug, Clone, PartialEq)]
    struct Foo(String);

    type Data = (u32, Foo);

    type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

    fn op(data: Data, acc: &mut AccValue, tid: ThreadId) {
        println!(
            "`op` called from {:?} with data {:?}",
            thread::current().id(),
            data
        );

        acc.entry(tid).or_default();
        let (k, v) = data;
        acc.get_mut(&tid).unwrap().insert(k, v.clone());
    }

    fn op_r(acc1: AccValue, acc2: AccValue) -> AccValue {
        println!(
            "`op_r` called from {:?} with acc1={:?} and acc2={:?}",
            thread::current().id(),
            acc1,
            acc2
        );

        let mut acc = acc1;
        acc2.into_iter().for_each(|(k, v)| {
            acc.insert(k, v);
        });
        acc
    }

    thread_local! {static MY_TL: Holder<AccValue> = Holder::new();}

    const NTHREADS: usize = 5;

    #[test]
    fn own_thread_and_explicit_joins_no_probe() {
        let mut control = Control::new(&MY_TL, HashMap::new, op, op_r);

        {
            control.send_data((1, Foo("a".to_owned())));
            control.send_data((2, Foo("b".to_owned())));
        }

        let tid_own = thread::current().id();
        let map_own = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);

        // This is directly defined as a reference to prevent the move closure below from moving the
        // `spawned_tids` value. The closure has to be `move` because it needs to own `i`.
        let spawned_tids = &RwLock::new(vec![thread::current().id(); NTHREADS]);

        thread::scope(|s| {
            let hs = (0..NTHREADS)
                .map(|i| {
                    let control = &control;
                    s.spawn({
                        move || {
                            let si = i.to_string();

                            let mut lock = spawned_tids.write().unwrap();
                            lock[i] = thread::current().id();
                            drop(lock);

                            control.send_data((1, Foo("a".to_owned() + &si)));
                            control.send_data((2, Foo("b".to_owned() + &si)));
                        }
                    })
                })
                .collect::<Vec<_>>();

            hs.into_iter().for_each(|h| h.join().unwrap());

            println!("after hs join: {:?}", control);
        });

        {
            let spawned_tids = spawned_tids.try_read().unwrap();
            let maps = (0..NTHREADS)
                .map(|i| {
                    let map_i = HashMap::from([
                        (1, Foo("a".to_owned() + &i.to_string())),
                        (2, Foo("b".to_owned() + &i.to_string())),
                    ]);
                    let tid_i = spawned_tids[i];
                    (tid_i, map_i)
                })
                .collect::<Vec<_>>();
            let mut map = maps.into_iter().collect::<HashMap<_, _>>();
            map.insert(tid_own, map_own);

            {
                let acc = control.drain_tls();
                assert_eq_and_println(&acc, &map, "Accumulator check");
            }
        }
    }

    #[test]
    fn own_thread_only_no_probe() {
        let mut control = Control::new(&MY_TL, HashMap::new, op, op_r);

        control.send_data((1, Foo("a".to_owned())));
        control.send_data((2, Foo("b".to_owned())));

        let tid_own = thread::current().id();
        let map_own = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);

        let map = HashMap::from([(tid_own, map_own)]);

        let acc = control.drain_tls();
        assert_eq_and_println(&acc, &map, "Accumulator check");
    }

    #[test]
    fn own_thread_and_explicit_join_with_probe() {
        let mut control = Control::new(&MY_TL, HashMap::new, op, op_r);

        let main_tid = thread::current().id();
        println!("main_tid={:?}", main_tid);

        let main_thread_gater = ThreadGater::new("main");
        let spawned_thread_gater = ThreadGater::new("spawned");

        let expected_acc_mutex = Mutex::new(HashMap::new());

        let assert_acc = |acc: &AccValue, msg: &str| {
            let exp = expected_acc_mutex.try_lock().unwrap().clone();
            assert_eq_and_println(acc, &exp, msg);
        };

        thread::scope(|s| {
            let h = s.spawn(|| {
                let spawned_tid = thread::current().id();
                println!("spawned tid={:?}", spawned_tid);

                let mut my_map = HashMap::<u32, Foo>::new();

                let mut process_value = |gate: u8, k: u32, v: Foo| {
                    main_thread_gater.wait_for(gate);
                    control.send_data((k, v.clone()));
                    my_map.insert(k, v);
                    expected_acc_mutex
                        .try_lock()
                        .unwrap()
                        .insert(spawned_tid, my_map.clone());
                    spawned_thread_gater.open(gate);
                };

                process_value(0, 1, Foo("aa".to_owned()));
                process_value(1, 2, Foo("bb".to_owned()));
                process_value(2, 3, Foo("cc".to_owned()));
                process_value(3, 4, Foo("dd".to_owned()));
            });

            {
                control.send_data((1, Foo("a".to_owned())));
                control.send_data((2, Foo("b".to_owned())));
                let my_map = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);

                let mut map = expected_acc_mutex.try_lock().unwrap();
                map.insert(main_tid, my_map);
                let map = map.clone(); // Mutex guard dropped here
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
                    &acc,
                    "Accumulator after 1st spawned thread insert and probe_tls",
                );
                main_thread_gater.open(1);
            }

            {
                spawned_thread_gater.wait_for(1);
                let acc = control.probe_tls();
                assert_acc(
                    &acc,
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
            }
        });

        {
            let acc = control.drain_tls();
            assert_acc(
                &acc,
                "Accumulator after 4th spawned thread insert and drain_tls",
            );
        }

        // drain_tls again
        {
            let acc = control.drain_tls();
            assert_eq_and_println(&acc, &HashMap::new(), "empty accumulatore expected");
        }
    }

    #[test]
    fn no_thread() {
        let mut control = Control::new(&MY_TL, HashMap::new, op, op_r);
        let acc = control.drain_tls();
        assert_eq!(acc, HashMap::new(), "empty accumulator expected");
    }
}
