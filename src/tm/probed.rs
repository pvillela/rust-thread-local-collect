// Modify this as needed, using APIs from control_g.rs as well.

//! This module supports the collection and aggregation of the values from a designated thread-local variable
//! across threads (see package [overfiew and core concepts](super)). The following capabilities and constraints apply ...
//! - The designated thread-local variable may be used in the thread responsible for
//! collection/aggregation.
//! - The linked thread-local variables hold a [`Sender`] that sends values to be aggregated into the
//! [Control] object's accumulated value.
//! - The [`Control`] object provides functions to receive thread-local values on a background thread,
//! stop receiving on a background thread, drain the [`Receiver`], and retrieve the accumulated value.
//! - After all participating threads other than the thread responsible for collection/aggregation have
//! stopped sending values, a call to [`Control::drain_tls`] followed by a call to one of the accumulated
//! value retrieval functions will result in the final aggregated value.
//! - [`Control::start_receiving_tls`] and [`Control::drain_tls`] may be called at any time, followed by a call to
//! [`Control::clone_acc`], to retrieve a partially accumulated value before all threads terminate.
//!
//! ## Usage pattern

//! ```rust
#![doc = include_str!("../../examples/tlm_channeled_i32_accumulator.rs")]
//! ````

//!
//! ## Other examples
//!
//! See another example at [`examples/tlm_channeled_map_accumulator`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/tlm_channeled_map_accumulator.rs).

use super::{thread_map::ThreadMap, POISONED_THREADMAP_LOCK};
use crate::tlm::common::POISONED_CONTROL_MUTEX;
use std::{
    fmt::Debug,
    mem::replace,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
    thread::ThreadId,
};

/// Controls the collection and accumulation of thread-local variables linked to this object.
///
/// `T` is the type of the values sent on the channel to this object and `U` is the type of the accumulated value.
/// The thread-locals must be of type [`Holder<T>`].
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
    /// - `tl` - reference to thread-local static.
    /// - `acc_base` - initial value for accumulation.
    /// - `op` - operation that combines data from thread-locals with accumulated value.
    pub fn new(
        value_init: fn() -> T,
        acc_base: U,
        op: impl Fn(T, &mut U, ThreadId) + 'static + Send + Sync,
    ) -> Self {
        let tmap = ThreadMap::new(value_init);
        Control {
            tmap: tmap.into(),
            acc: Mutex::new(acc_base).into(),
            op: Arc::new(op),
        }
    }

    /// Acquires a lock on [`Control`]'s internal mutex.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    fn lock(&self) -> MutexGuard<'_, U> {
        self.acc.lock().expect(POISONED_CONTROL_MUTEX)
    }

    /// Returns a guard object that dereferences to `self`'s accumulated value. A lock is held during the guard's
    /// lifetime.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    pub fn acc(&self) -> impl Deref<Target = U> + '_ {
        self.lock()
    }

    /// Provides access to `self`'s accumulated value.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    pub fn with_acc<V>(&self, f: impl FnOnce(&U) -> V) -> V {
        let acc = self.acc();
        f(&acc)
    }

    /// Returns a clone of `self`'s accumulated value.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
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
    /// If `self`'s mutex is poisoned.
    pub fn take_acc(&self, replacement: U) -> U {
        let mut lock = self.lock();
        let acc = lock.deref_mut();
        replace(acc, replacement)
    }

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
        let mut lock = self.lock();
        let acc = lock.deref_mut();
        let map = self.tmap.drain().expect(POISONED_THREADMAP_LOCK);
        map.into_iter().fold(acc, |u, (tid, v)| {
            (self.op)(v, u, tid);
            u
        });
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
    pub fn with_data<V>(&self, f: impl FnOnce(&T) -> V) -> V {
        self.tmap.with(f)
    }

    /// Invokes `f` mutably on the held data.
    pub fn with_data_mut<V>(&self, f: impl FnOnce(&mut T) -> V) -> V {
        self.tmap.with_mut(f)
    }
}

// #[cfg(test)]
// #[allow(clippy::unwrap_used)]
// mod tests {
//     use super::{Control, Holder, MultipleReceiverThreadsError};
//     use crate::dev_support::{assert_eq_and_println, ThreadGater};
//     use std::{
//         collections::HashMap,
//         fmt::Debug,
//         ops::Deref,
//         sync::Mutex,
//         thread::{self, ThreadId},
//         time::Duration,
//     };

//     #[derive(Debug, Clone, PartialEq)]
//     struct Foo(String);

//     type Data = (i32, Foo);

//     type AccValue = HashMap<ThreadId, HashMap<i32, Foo>>;

//     thread_local! {
//         static MY_TL: Holder<Data> = Holder::new();
//     }

//     fn op(data: Data, acc: &mut AccValue, tid: ThreadId) {
//         println!(
//             "`op` called from {:?} with data {:?}",
//             thread::current().id(),
//             data
//         );

//         acc.entry(tid).or_default();
//         let (k, v) = data;
//         acc.get_mut(&tid).unwrap().insert(k, v.clone());
//     }

//     #[test]
//     fn own_thread_and_explicit_join() {
//         let control = Control::new(&MY_TL, HashMap::new(), op);

//         let main_tid = thread::current().id();
//         println!("main_tid={:?}", main_tid);

//         let main_thread_gater = ThreadGater::new("main");
//         let spawned_thread_gater = ThreadGater::new("spawned");

//         let expected_acc_mutex = Mutex::new(HashMap::new());

//         let assert_acc = |acc: &AccValue, msg: &str| {
//             let exp_guard = expected_acc_mutex.try_lock().unwrap();
//             let exp = exp_guard.deref();

//             assert_eq_and_println(acc, exp, msg);
//         };

//         thread::scope(|s| {
//             let h = s.spawn(|| {
//                 let spawned_tid = thread::current().id();
//                 println!("spawned tid={:?}", spawned_tid);

//                 let mut my_map = HashMap::<i32, Foo>::new();

//                 let mut process_value = |gate: u8, k: i32, v: Foo| {
//                     main_thread_gater.wait_for(gate);
//                     control.send_data((k, v.clone()));
//                     my_map.insert(k, v);
//                     expected_acc_mutex
//                         .try_lock()
//                         .unwrap()
//                         .insert(spawned_tid, my_map.clone());
//                     // allow background receiving thread to receive above send
//                     thread::sleep(Duration::from_millis(10));
//                     spawned_thread_gater.open(gate);
//                 };

//                 process_value(0, 1, Foo("aa".to_owned()));
//                 process_value(1, 2, Foo("bb".to_owned()));
//                 process_value(2, 3, Foo("cc".to_owned()));
//                 process_value(3, 4, Foo("dd".to_owned()));
//             });

//             {
//                 control.start_receiving_tls().unwrap();
//             }

//             {
//                 control.send_data((1, Foo("a".to_owned())));
//                 control.send_data((2, Foo("b".to_owned())));
//                 let my_map = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);

//                 // Allow background receiving thread to receive above sends.
//                 thread::sleep(Duration::from_millis(10));

//                 expected_acc_mutex
//                     .try_lock()
//                     .unwrap()
//                     .insert(main_tid, my_map);
//                 assert_acc(control.acc().deref(), "Accumulator after main thread sends");
//                 main_thread_gater.open(0);
//             }

//             {
//                 spawned_thread_gater.wait_for(0);
//                 assert_acc(
//                     control.acc().deref(),
//                     "Accumulator after 1st spawned thread send",
//                 );

//                 {
//                     control.stop_receiving_tls();
//                     // Allow background receiving thread to process command.
//                     thread::sleep(Duration::from_millis(10));
//                 }

//                 main_thread_gater.open(1);
//             }

//             {
//                 spawned_thread_gater.wait_for(1);
//                 {
//                     let exp = expected_acc_mutex.try_lock().unwrap();
//                     let acc = control.acc();
//                     assert_ne!(
//                         acc.deref(),
//                         exp.deref(),
//                         "Accumulator should not reflect 2nd spawned thread send",
//                     );
//                 }
//                 main_thread_gater.open(2);
//             }

//             {
//                 control.start_receiving_tls().unwrap();
//                 // Allow background receiving thread to process command.
//                 thread::sleep(Duration::from_millis(10));
//             }

//             {
//                 spawned_thread_gater.wait_for(2);
//                 assert_acc(
//                     control.acc().deref(),
//                     "Accumulator should reflect 2nd and 3rd spawned thread sends",
//                 );

//                 {
//                     control.stop_receiving_tls();
//                     // Allow background receiving thread to process command.
//                     thread::sleep(Duration::from_millis(10));
//                 }

//                 main_thread_gater.open(3);
//             }

//             {
//                 // Join spawned thread.
//                 h.join().unwrap();

//                 {
//                     let exp = expected_acc_mutex.try_lock().unwrap();
//                     let acc = control.acc();
//                     assert_ne!(
//                         acc.deref(),
//                         exp.deref(),
//                         "Accumulator should not reflect 4th spawned thread send",
//                     );
//                 }

//                 control.drain_tls();

//                 assert_acc(
//                     control.acc().deref(),
//                     "Accumulator should reflect 4th spawned thread send",
//                 );
//             }

//             {
//                 {
//                     control.with_acc(|acc| {
//                         assert_acc(
//                             acc,
//                             "Accumulator after spawned thread join, using control.with_acc()",
//                         );
//                     });
//                 }

//                 {
//                     let acc = control.clone_acc();
//                     assert_acc(
//                         &acc,
//                         "Accumulator after spawned thread join, using control.clone_acc()",
//                     );
//                 }

//                 {
//                     let acc = control.take_acc(HashMap::new());
//                     assert_acc(
//                         &acc,
//                         "Accumulator after spawned thread join, using control.take_acc()",
//                     );
//                 }

//                 {
//                     control.with_acc(|acc| {
//                         assert_eq_and_println(
//                             acc,
//                             &HashMap::new(),
//                             "Accumulator after control.take_acc()",
//                         );
//                     });
//                 }
//             }
//         });
//     }

//     #[test]
//     fn multiple_receiver_threads() {
//         let control = Control::new(&MY_TL, HashMap::new(), op);

//         thread::scope(|s| {
//             s.spawn(|| {
//                 control.send_data((0, Foo("aa".to_owned())));
//             });

//             control.start_receiving_tls().unwrap();
//             let res = control.start_receiving_tls();
//             match res {
//                 Err(MultipleReceiverThreadsError) => (),
//                 _ => panic!("unexpected result {res:?}"),
//             }
//         });
//     }
// }
