//! Variant of module [`crate::tlm::joined`] with a `send` API similar to that of [`crate::tlcr`] sub-modules.
//!
//! This module supports the collection and aggregation of values across threads (see package
//! [overview and core concepts](crate)). The following features and constraints apply ...
//! - Values may be collected from the thread responsible for collection/aggregation.
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
//! use thread_local_collect::tlm::send::joined::{Control, Holder};
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
//!     // Send data to control from main thread if desired.
//!     control.send_data(100);
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
//! ````
//!
//! ## Other examples
//!
//! See another example at [`examples/tlmsend_joined_map_accumulator`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/tlmsend_joined_map_accumulator.rs).

use super::control_send::{ControlSendG, WithTakeTls};
use crate::tlm::joined::{Control as ControlInner, Holder as HolderInner, P as POrig};

pub type Control<T, U> = ControlSendG<POrig<U, Option<U>>, T, U>;

impl<T, U> WithTakeTls<POrig<U, Option<U>>, U> for Control<T, U>
where
    U: 'static,
{
    fn take_tls(control: &ControlInner<U, Option<U>>) {
        control.take_own_tl();
    }
}

pub type Holder<U> = HolderInner<U, Option<U>>;

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
    fn own_thread_and_explicit_joins() {
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

            // drain_tls again
            {
                let acc = control.drain_tls();
                assert_eq_and_println(&acc, &HashMap::new(), "empty accumulatore expected");
            }
        }
    }

    #[test]
    fn own_thread_only() {
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
    fn no_thread() {
        let mut control = Control::new(&MY_TL, HashMap::new, op, op_r);
        let acc = control.drain_tls();
        assert_eq_and_println(&acc, &HashMap::new(), "empty accumulatore expected");
    }
}
