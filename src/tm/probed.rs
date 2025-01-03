//! This module supports the collection and aggregation of values across threads (see package
//! [overview and core concepts](crate)), including the ability to inspect the accumulated value before
//! participating threads have terminated.
//! It is present only when the **"tm"** feature flag is enabled.
//! The following capabilities and constraints apply ...
//! - The participating threads update thread-local data via the clonable `control` object which contains a
//! [`ThreadMap`](https://docs.rs/thread_map/latest/thread_map/) instance and aggregates the values.
//! - The [`Control::probe_tls`] function can be called at any time, from any thread, to return a clone of the current
//! aggregated value.
//! - After all participating threads other than the thread responsible for collection/aggregation have
//! terminated (joins are not necessary), a call to [`Control::take_tls`] followed by a call to one of the accumulator
//! retrieval functions will return the final aggregated value.
//!
//! ## Usage pattern

//! ```rust
#![doc = include_str!("../../examples/tm_probed_i32_accumulator.rs")]
//! ````

//!
//! ## Other examples
//!
//! See another example at [`examples/tm_probed_map_accumulator`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/tm_probed_map_accumulator.rs).

use super::POISONED_ACCUMULATOR_MUTEX;
use super::POISONED_THREADMAP_LOCK;
use std::{
    fmt::Debug,
    mem::replace,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
    thread::ThreadId,
};
use thread_map::ThreadMap;

/// Controls the collection and accumulation of thread-local values.
///
/// The thread-local values are of type `T` and the accumulated value is of type `U`.
///
/// This type holds the following:
/// - A [`ThreadMap`] object that holds the thread-local values and a function to initialized those values.
/// - The accumulated value.
/// - An operation that is used to combine thread-local values with the accumulated value.
pub struct Control<T, U>
where
    T: 'static,
{
    tmap: Arc<ThreadMap<T>>,
    acc: Arc<Mutex<U>>,
    #[allow(clippy::type_complexity)]
    op: Arc<dyn Fn(T, &mut U, ThreadId) + Send + Sync>,
}

impl<T, U> Clone for Control<T, U> {
    fn clone(&self) -> Self {
        Self {
            tmap: self.tmap.clone(),
            acc: self.acc.clone(),
            op: self.op.clone(),
        }
    }
}

impl<T, U> Debug for Control<T, U>
where
    T: Debug,
    U: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "Control {{ tmap: {:?}, acc: {:?} }}",
            self.tmap, self.acc
        ))
    }
}

impl<T, U> Control<T, U> {
    /// Instantiates a [`Control`] object.
    ///
    /// - `acc_base` - initial value for accumulation.
    /// - `value_init` - constructs initial thread-local values.
    /// - `op` - operation that combines data from thread-local values with the accumulated value.
    pub fn new(
        acc_base: U,
        value_init: fn() -> T,
        op: impl Fn(T, &mut U, ThreadId) + 'static + Send + Sync,
    ) -> Self {
        let tmap = ThreadMap::new(value_init);
        Control {
            tmap: tmap.into(),
            acc: Mutex::new(acc_base).into(),
            op: Arc::new(op),
        }
    }

    /// Acquires a lock on `acc`'s mutex.
    ///
    /// # Panics
    /// If `acc`'s mutex is poisoned.
    fn lock_acc(&self) -> MutexGuard<'_, U> {
        self.acc.lock().expect(POISONED_ACCUMULATOR_MUTEX)
    }

    /// Returns a guard object that dereferences to `self`'s accumulated value. A mutex lock is held during the guard's
    /// lifetime.
    ///
    /// # Panics
    /// If `self`'s accumulated value mutex is poisoned.
    pub fn acc(&self) -> impl Deref<Target = U> + '_ {
        self.lock_acc()
    }

    /// Provides access to `self`'s accumulated value.
    ///
    /// # Panics
    /// If `self`'s accumulated value mutex is poisoned.
    pub fn with_acc<V>(&self, f: impl FnOnce(&U) -> V) -> V {
        let acc = self.acc();
        f(&acc)
    }

    /// Returns a clone of `self`'s accumulated value.
    ///
    /// # Panics
    /// If `self`'s accumulated value mutex is poisoned.
    pub fn clone_acc(&self) -> U
    where
        U: Clone,
    {
        self.acc().clone()
    }

    /// Returns `self`'s accumulated value, using a value of the same type to replace
    /// the existing accumulated value.
    ///
    /// # Panics
    /// If `self`'s accumulated value mutex is poisoned.
    pub fn take_acc(&self, replacement: U) -> U {
        let mut lock = self.lock_acc();
        let acc = lock.deref_mut();
        replace(acc, replacement)
    }

    /// Aggregates the thread-local values with this object's accumulator and clears the thread-local values.
    ///
    /// # Panics
    /// - If `self`'s [`ThreadMap`] state is poisoned.
    /// - If `self`'s accumulated value mutex is poisoned.
    pub fn take_tls(&self) {
        let mut lock = self.lock_acc();
        let acc = lock.deref_mut();
        let map = self.tmap.drain().expect(POISONED_THREADMAP_LOCK);
        map.into_iter().fold(acc, |u, (tid, v)| {
            (self.op)(v, u, tid);
            u
        });
    }

    /// Collects the thread-local-variables, without changing those values,
    /// aggregates those values with a clone of this object's accumulator, and returns the aggregate
    /// value. This object's accumulator remains unchanged.
    ///
    /// # Panics
    /// - If `self`'s [`ThreadMap`] state is poisoned.
    /// - If `self`'s accumulated value mutex is poisoned.
    pub fn probe_tls(&self) -> U
    where
        T: Clone,
        U: Clone,
    {
        let acc = self.acc().clone();
        self.tmap
            .fold(acc, |mut u, (tid, v)| {
                let mru = &mut u;
                (self.op)(v.clone(), mru, tid);
                u
            })
            .expect(POISONED_THREADMAP_LOCK)
    }

    /// Invokes `f` on the held data.
    ///
    /// # Panics
    /// If `self`'s [`ThreadMap`] state is poisoned.
    pub fn with_data<V>(&self, f: impl FnOnce(&T) -> V) -> V {
        self.tmap.with(f)
    }

    /// Invokes `f` mutably on the held data.
    ///
    /// # Panics
    /// If `self`'s [`ThreadMap`] state is poisoned.
    pub fn with_data_mut<V>(&self, f: impl FnOnce(&mut T) -> V) -> V {
        self.tmap.with_mut(f)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::Control;
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
    fn unprobed_own_thread_and_explicit_joins() {
        let control = Control::new(HashMap::new(), HashMap::new, op);

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
        let control = Control::new(HashMap::new(), HashMap::new, op);

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
