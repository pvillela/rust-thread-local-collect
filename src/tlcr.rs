//! This module is supported on **`feature="tlcr"`** only.
//! This module supports the collection and aggregation of values across threads (see package
//! [overview and core concepts](super)). The following features and constraints apply ...
//! - Values may NOT be collected from the thread responsible for collection/aggregation.
//! (If this condition is violated, the [`Control::drain_tls`] function returns an error.)
//! - The participating threads access a [`ThreadLocal`](https://docs.rs/thread_local/latest/thread_local/) instance
//! that is a clone of an object of the same type held in a [Control] object to accumulate thread-local values that
//! are *sent* to it.
//! - The [`Control::drain_tls`] function can be called after all participating threads have
//! terminated and EXPLICITLY joined, directly or indirectly, into the thread responsible for collection.
//!
//! ## Usage pattern
//!
//! Here's an outline of how this little framework can be used:
//!
//! ```rust
//! use std::thread::{self, ThreadId};
//! use thread_local_collect::tlcr::Control;
//!
//! // Define your data type, e.g.:
//! type Data = i32;
//!
//! // Define your accumulated value type.
//! type AccValue = i32;
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
//! const NTHREADS: i32 = 5;
//!
//! fn main() {
//!     let mut control = Control::new(|| 0, op, op_r);
//!
//!     thread::scope(|s| {
//!         let hs = (0..NTHREADS)
//!             .map(|i| {
//!                 let control = control.clone();
//!                 s.spawn({
//!                     move || {
//!                         control.send_data(i);
//!                     }
//!                 })
//!             })
//!             .collect::<Vec<_>>();
//!
//!         hs.into_iter().for_each(|h| h.join().unwrap());
//!
//!         // Drain thread-locals.
//!         let acc = control.drain_tls().unwrap();
//!
//!         // Print the accumulated value
//!
//!         println!("accumulated={acc}");
//!     });
//! }
//! ````
//!
//! ## Other examples
//!
//! See another example at [`examples/tlcr_map_accumulator.rs`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/tlcr_map_accumulator.rs).

use std::{
    cell::RefCell,
    fmt::Debug,
    mem::replace,
    sync::Arc,
    thread::{self, ThreadId},
};
use thiserror::Error;
use thread_local::ThreadLocal;

/// Errors returned by [`Control::drain_tls`].
#[derive(Error, Debug)]
pub enum DrainTlsError {
    /// Method was called while thread-locals were arctive.
    #[error("method called while thread-locals were arctive")]
    ActiveThreadLocalsError,

    /// There were no thread-locals to aggregate.
    #[error("there were no thread-locals to aggregate")]
    NoThreadLocalsUsed,
}

/// Controls the collection and accumulation of thread-local variables linked to this object.
///
/// `T` is the type of the values *sent* to this object and `U` is the type of the accumulated value.
///
/// This type holds the following:
/// - A state object based on [`ThreadLocal`].
/// - A nullary closure that produces a zero value of type `U`, which is needed to obtain consistent aggregation results.
/// - An operation that combines data sent from thread-locals with the accumulated value.
/// - A binary operation that reduces two accumulated values into one.
pub struct Control<T, U>
where
    U: Send,
{
    /// Keeps track of registered threads and accumulated value.
    state: Arc<ThreadLocal<RefCell<U>>>,
    /// Produces a zero value of type `U`, which is needed to obtain consistent aggregation results.
    acc_zero: Arc<dyn Fn() -> U + Send + Sync>,
    /// Operation that combines data sent from thread-locals with the accumulated value.
    #[allow(clippy::type_complexity)]
    op: Arc<dyn Fn(T, &mut U, ThreadId) + Send + Sync>,
    /// Binary operation that reduces two accumulated values into one.
    op_r: Arc<dyn Fn(U, U) -> U + Send + Sync>,
}

impl<T, U> Clone for Control<T, U>
where
    U: Send,
{
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            op: self.op.clone(),
            op_r: self.op_r.clone(),
            acc_zero: self.acc_zero.clone(),
        }
    }
}

impl<T, U> Debug for Control<T, U>
where
    U: Send + Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.state)
    }
}

impl<T, U> Control<T, U>
where
    U: Send,
{
    /// Instantiates a [`Control`] object.
    ///
    /// - `acc_zero` - produces a zero value of type `U`, which is needed to obtain consistent aggregation results.
    /// - `op` - operation that combines data sent from thread-locals with accumulated value.
    /// - `op_r` - binary operation that reduces two accumulated values into one.
    pub fn new(
        acc_zero: impl Fn() -> U + 'static + Send + Sync,
        op: impl Fn(T, &mut U, ThreadId) + 'static + Send + Sync,
        op_r: impl Fn(U, U) -> U + 'static + Send + Sync,
    ) -> Self {
        Control {
            state: Arc::new(ThreadLocal::new()),
            acc_zero: Arc::new(acc_zero),
            op: Arc::new(op),
            op_r: Arc::new(op_r),
        }
    }

    /// Sends data to be aggregated.
    pub fn send_data(&self, data: T) {
        let cell = self.state.get_or(|| RefCell::new((self.acc_zero)()));
        let mut u = cell.borrow_mut();
        (self.op)(data, &mut u, thread::current().id());
    }

    /// Returns the accumulation of the thread-local values.
    /// Returns an error if any designated thread-local variable instance still exists, i.e., the corresponding
    /// thread has not yet terminated and explicitly joined, directly or indirectly, the thread where this
    /// function is called from.
    pub fn drain_tls(&mut self) -> Result<U, DrainTlsError> {
        let state = replace(&mut self.state, Arc::new(ThreadLocal::new()));
        let unwr_state = match Arc::try_unwrap(state) {
            Ok(unwr_state) => unwr_state,
            Err(state) => {
                _ = replace(&mut self.state, state); // put it back
                return Err(DrainTlsError::ActiveThreadLocalsError);
            }
        };
        let res = unwr_state
            .into_iter()
            .map(|x| x.into_inner())
            .reduce(self.op_r.as_ref());
        res.ok_or(DrainTlsError::NoThreadLocalsUsed)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::Control;
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

    const NTHREADS: usize = 5;

    #[test]
    fn test() {
        let mut control = Control::new(HashMap::new, op, op_r);

        // This is directly defined as a reference to prevent the move closure below from moving the
        // `spawned_tids` value. The closure has to be `move` because it needs to own `i`.
        let spawned_tids = &RwLock::new(vec![thread::current().id(); NTHREADS]);

        thread::scope(|s| {
            let hs = (0..NTHREADS)
                .map(|i| {
                    let control = control.clone();
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
                let acc = control.drain_tls().unwrap();
                assert_eq_and_println(acc, map, "Accumulator check");
            }
        }
    }
}
