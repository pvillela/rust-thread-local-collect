//! Variant of module [`crate::tlm::probed`] with a `send` API similar to that of [`crate::tlcr::probed`].
//!
//! This module supports the collection and aggregation of values across threads (see package
//! [overview and core concepts](crate)), including the ability to inspect
//! the accumulated value before participating threads have terminated. The following features and constraints apply ...
//! - Values may be collected from the thread responsible for collection/aggregation.
//! - The participating threads update thread-local data via the clonable `control` object which is also
//! used to aggregate the values.
//! - The [`Control::probe_tls`] function can be called at any time to return a clone of the current aggregated value.
//! - The [`Control::drain_tls`] function can be called to return the accumulated value after all participating
//! threads (other than the thread responsible for collection) have terminated (joins are not necessary).
//!
//! ## Usage pattern

//! ```rust
#![doc = include_str!("../../../examples/tlmrestr_probed_i32_accumulator.rs")]
//! ````

//!
//! ## Other examples
//!
//! See another example at [`examples/tlmrestr_probed_map_accumulator`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/tlmrestr_probed_map_accumulator.rs).

pub use super::control_restr::ControlRestrG;

use super::control_restr::WithTakeTls;
use crate::tlm::probed::{Control as ControlInner, Holder as HolderInner, Probed};

/// Specialization of [`ControlRestrG`] for this module.
/// Controls the collection and accumulation of thread-local values linked to this object.
///
/// `U` is the type of the accumulated value.
/// Partially accumulated values are held in thread-locals of type [`Holder<U>`].
pub type Control<U> = ControlRestrG<Probed<U, Option<U>>, U>;

impl<U> WithTakeTls<Probed<U, Option<U>>, U> for Control<U>
where
    U: 'static,
{
    fn take_tls(control: &ControlInner<U, Option<U>>) {
        control.take_tls();
    }
}

impl<U> Control<U>
where
    U: Clone,
{
    pub fn probe_tls(&self) -> U {
        self.control
            .probe_tls()
            .expect("accumulator guaranteed to never be None")
    }
}

/// Specialization of [`crate::tlm::probed::Holder`] for this module.
/// Holds thread-local partially accumulated data of type `U` and a smart pointer to a [`Control<U>`],
/// enabling the linkage of the held data with the control object.
pub type Holder<U> = HolderInner<U, Option<U>>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Control, Holder};
    use crate::dev_support::{assert_eq_and_println, ThreadGater};
    use std::{
        collections::HashMap,
        fmt::Debug,
        sync::Mutex,
        thread::{self, ThreadId},
    };

    #[derive(Debug, Clone, PartialEq)]
    struct Foo(String);

    type Data = (i32, Foo);

    type AccValue = HashMap<ThreadId, HashMap<i32, Foo>>;

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
        let mut control = Control::new(&MY_TL, HashMap::new, op_r);

        let tid_own = thread::current().id();

        let map_own = {
            let value1 = Foo("a".to_owned());
            let value2 = Foo("b".to_owned());
            let map_own = HashMap::from([(1, value1.clone()), (2, value2.clone())]);

            control.aggregate_data((1, value1), op);
            control.aggregate_data((2, value2), op);

            map_own
        };

        let tid_map_pairs = thread::scope(|s| {
            let hs = (0..NTHREADS)
                .map(|i| {
                    let value1 = Foo("a".to_owned() + &i.to_string());
                    let value2 = Foo("a".to_owned() + &i.to_string());
                    let map_i = HashMap::from([(1, value1.clone()), (2, value2.clone())]);

                    s.spawn(|| {
                        control.aggregate_data((1, value1), op);
                        control.aggregate_data((2, value2), op);

                        let tid_spawned = thread::current().id();
                        (tid_spawned, map_i)
                    })
                })
                .collect::<Vec<_>>();

            hs.into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });

        {
            let map = std::iter::once((tid_own, map_own))
                .chain(tid_map_pairs)
                .collect::<HashMap<_, _>>();

            {
                let acc = control.drain_tls();
                assert_eq_and_println(&acc, &map, "Accumulator check");
            }

            // drain_tls again
            {
                let acc = control.drain_tls();
                assert_eq_and_println(&acc, &HashMap::new(), "empty accumulatore expected");
            }
        }

        // Control reused.
        {
            let map_own = {
                let value1 = Foo("c".to_owned());
                let value2 = Foo("d".to_owned());
                let map_own = HashMap::from([(11, value1.clone()), (22, value2.clone())]);

                control.aggregate_data((11, value1), op);
                control.aggregate_data((22, value2), op);

                map_own
            };

            let (tid_spawned, map_spawned) = thread::scope(|s| {
                let control = &control;

                let value1 = Foo("x".to_owned());
                let value2 = Foo("y".to_owned());
                let map_spawned = HashMap::from([(11, value1.clone()), (22, value2.clone())]);

                let tid = s
                    .spawn(move || {
                        control.aggregate_data((11, value1), op);
                        control.aggregate_data((22, value2), op);
                        thread::current().id()
                    })
                    .join()
                    .unwrap();

                (tid, map_spawned)
            });

            let map = HashMap::from([(tid_own, map_own), (tid_spawned, map_spawned)]);
            let acc = control.drain_tls();
            assert_eq_and_println(&acc, &map, "take_acc - control reused");
        }
    }

    #[test]
    fn own_thread_only_no_probe() {
        let mut control = Control::new(&MY_TL, HashMap::new, op_r);

        control.aggregate_data((1, Foo("a".to_owned())), op);
        control.aggregate_data((2, Foo("b".to_owned())), op);

        let tid_own = thread::current().id();
        let map_own = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);

        let map = HashMap::from([(tid_own, map_own)]);

        let acc = control.drain_tls();
        assert_eq_and_println(&acc, &map, "Accumulator check");
    }

    #[test]
    fn own_thread_and_explicit_join_with_probe() {
        let mut control = Control::new(&MY_TL, HashMap::new, op_r);

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

                let mut my_map = HashMap::<i32, Foo>::new();

                let mut process_value = |gate: u8, k: i32, v: Foo| {
                    main_thread_gater.wait_for(gate);
                    control.aggregate_data((k, v.clone()), op);
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
                control.aggregate_data((1, Foo("a".to_owned())), op);
                control.aggregate_data((2, Foo("b".to_owned())), op);
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
        let mut control = Control::new(&MY_TL, HashMap::new, op_r);
        let acc = control.drain_tls();
        assert_eq!(acc, HashMap::new(), "empty accumulator expected");
    }
}
