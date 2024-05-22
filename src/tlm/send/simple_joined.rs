//! Variant of module [`crate::tlm::simple_joined`] with a `send` API similar to that of [`crate::tlcr::joined`].
//!
//! This module supports the collection and aggregation of values across threads (see package
//! [overview and core concepts](crate)). The following features and constraints apply ...
//! - The designated thread-local variable should NOT be used in the thread responsible for
//! collection/aggregation. If this condition is violated, the thread-local value on that thread will NOT
//! be collected and aggregated.
//! - The participating threads *send* data to a clonable `control` object instance that aggregates the values.
//! - The [`Control::drain_tls`] function can be called to return the accumulated value after all participating
//! threads have terminated and EXPLICITLY joined, directly or indirectly, into the thread responsible for collection.
//!
//! ## Usage pattern
//!
//! Here's an outline of how this little framework can be used:
//!
//! ```rust
//! use std::thread::{self, ThreadId};
//! use thread_local_collect::tlm::send::simple_joined::{Control, Holder};
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
//! const NTHREADS: i32 = 5;
//!
//! fn main() {
//!     // Instantiate the control object.
//!     let mut control = Control::new(&MY_TL, acc_zero, op, op_r);
//!
//!     // So NOT send data to control from main thread as it will not be accumulated.
//!
//!     let hs = (0..NTHREADS)
//!         .map(|i| {
//!             // Clone control for use in the new thread.
//!             let control = control.clone();
//!             thread::spawn({
//!                 move || {
//!                     // Send data from thread to control object.
//!                     control.send_data(i);
//!                 }
//!             })
//!         })
//!         .collect::<Vec<_>>();
//!
//!     // Join all threads.
//!     hs.into_iter().for_each(|h| h.join().unwrap());
//!
//!     // Drain thread-local values.
//!     let acc = control.drain_tls();
//!
//!     // Print the accumulated value
//!     println!("accumulated={acc}");
//! }
//! ``````
//!
//! ## Other examples
//!
//! See another example at [`examples/tlmsend_simple_joined_map_accumulator`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/tlmsend_simple_joined_map_accumulator.rs).

use super::control_send::{ControlSendG, WithTakeTls};
use crate::tlm::simple_joined::{Control as ControlOrig, Holder as HolderOrig, P as POrig};

/// Specialization of [`ControlSendG`] for this module.
/// Controls the collection and accumulation of thread-local values linked to this object.
///
/// `T` is the type of the data sent from threads for accumulation and `U` is the type of the accumulated value.
/// Partially accumulated values are held in thread-locals of type [`Holder<U>`].
pub type Control<T, U> = ControlSendG<POrig<U, Option<U>>, T, U>;

impl<T, U> WithTakeTls<POrig<U, Option<U>>, U> for Control<T, U>
where
    U: 'static,
{
    fn take_tls(_control: &ControlOrig<U, Option<U>>) {}
}

/// Specialization of [`crate::tlm::joined::Holder`] for this module.
/// Holds thread-local partially accumulated data of type `U` and a smart pointer to a [`Control<T, U>`],
/// enabling the linkage of the held data with the control object.
pub type Holder<U> = HolderOrig<U, Option<U>>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Control, Holder};
    use crate::test_support::assert_eq_and_println;
    use std::{
        collections::HashMap,
        fmt::Debug,
        sync::RwLock,
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
    fn explicit_joins() {
        let mut control = Control::new(&MY_TL, HashMap::new, op, op_r);

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
            let map = maps.into_iter().collect::<HashMap<_, _>>();

            {
                let acc = control.drain_tls();
                assert_eq_and_println(&acc, &map, "after drain_tls");
            }

            // drain_tls again
            {
                let acc = control.drain_tls();
                assert_eq_and_println(&acc, &HashMap::new(), "empty accumulatore expected");
            }
        }
    }

    #[test]
    fn no_thread() {
        let mut control = Control::new(&MY_TL, HashMap::new, op, op_r);
        let acc = control.drain_tls();
        assert_eq_and_println(&acc, &HashMap::new(), "after drain_tls");
    }
}
