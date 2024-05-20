//! This module is supported on **`feature="tlcr"`** only.
//! This module supports the collection and aggregation of values across threads (see package
//! [overview and core concepts](crate)). The following features and constraints apply ...
//! - Values may be collected from the thread responsible for collection/aggregation, provided that the `control`
//! object of type [`Control`] is created on that thread and is not cloned by that thread.
//! - The participating threads *send* data to a clonable `control` object which contains a
//! [`ThreadLocal`](https://docs.rs/thread_local/latest/thread_local/) instance that aggregates the values.
//! - The [`Control::drain_tls`] function can be called to return the accumulated value after all participating
//! threads have terminated and EXPLICITLY joined, directly or indirectly, into the thread responsible for collection.
//!
//! ## Usage pattern
//!
//! Here's an outline of how this little framework can be used:
//!
//! ```rust
//! use std::thread::{self, ThreadId};
//! use thread_local_collect::tlcr::joined::Control;
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
//! const NTHREADS: i32 = 5;
//!
//! fn main() {
//!     // Instantiate the control object.
//!     let mut control = Control::new(acc_zero, op, op_r);
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
//!     let acc = control.drain_tls().unwrap();
//!
//!     // Print the accumulated value
//!     println!("accumulated={acc}");
//! }
//! ````
//!
//! ## Other examples
//!
//! See another example at [`examples/tlcr_joined_map_accumulator`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/tlcr_joined_map_accumulator.rs).

use std::{
    cell::RefCell,
    fmt::Debug,
    mem::replace,
    sync::Arc,
    thread::{self, ThreadId},
};
use thiserror::Error;
use thread_local::ThreadLocal;

#[derive(Error, Debug, PartialEq)]
/// Method was called while some thread that sent a value for accumulation was still active.
#[error("method called while thread-locals were arctive")]
pub struct ActiveThreadLocalsError;

/// Controls the collection and accumulation of thread-local values.
///
/// `T` is the type of the values *sent* to this object and `U` is the type of the accumulated value.
///
/// This type holds the following:
/// - A state object based on [`ThreadLocal`].
/// - A nullary closure that produces a zero value of type `U`, which is needed to obtain consistent aggregation results.
/// - An operation that combines the accumulated value with data sent from threads.
/// - A binary operation that reduces two accumulated values into one.
pub struct Control<T, U>
where
    U: Send,
{
    /// Keeps track of registered threads and accumulated value.
    state: Arc<ThreadLocal<RefCell<U>>>,
    /// Produces a zero value of type `U`, which is needed to obtain consistent aggregation results.
    acc_zero: Arc<dyn Fn() -> U + Send + Sync>,
    /// Operation that combines the accumulated value with data sent from threads.
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
    /// - `op` - operation that combines the accumulated value with data sent from threads.
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

    /// Returns the accumulation of the thread-local values, replacing the state of `self` with an empty
    /// [`ThreadLocal`](https://docs.rs/thread_local/latest/thread_local/struct.ThreadLocal.html).
    /// Returns an error if `self` has not been used by any threads or any thread
    /// using `self`, other than the thread where this function is called from, has not yet terminated and explicitly
    /// joined, directly or indirectly, the thread where this function is called from. In this case, the state of
    /// `self` is left unchanged.
    pub fn drain_tls(&mut self) -> Result<U, ActiveThreadLocalsError> {
        let state = replace(&mut self.state, Arc::new(ThreadLocal::new()));
        let unwr_state = match Arc::try_unwrap(state) {
            Ok(unwr_state) => unwr_state,
            Err(state) => {
                _ = replace(&mut self.state, state); // put it back
                return Err(ActiveThreadLocalsError);
            }
        };
        let res = unwr_state
            .into_iter()
            .map(|x| x.into_inner())
            .fold((self.acc_zero)(), self.op_r.as_ref());
        Ok(res)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{ActiveThreadLocalsError, Control};
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
    fn own_thread_and_explicit_joins() {
        let mut control = Control::new(HashMap::new, op, op_r);

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
                assert_eq_and_println(&acc, &Ok(map), "Accumulator check");
            }

            // drain_tls again
            {
                let acc = control.drain_tls();
                assert_eq_and_println(&acc, &Ok(HashMap::new()), "empty accumulatore expected");
            }
        }
    }

    #[test]
    fn own_thread_only() {
        let mut control = Control::new(HashMap::new, op, op_r);

        control.send_data((1, Foo("a".to_owned())));
        control.send_data((2, Foo("b".to_owned())));

        let tid_own = thread::current().id();
        let map_own = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);

        let map = HashMap::from([(tid_own, map_own)]);

        let acc = control.drain_tls();
        assert_eq_and_println(&acc, &Ok(map), "Accumulator check");
    }

    #[test]
    fn no_thread() {
        let mut control = Control::new(HashMap::new, op, op_r);
        let acc = control.drain_tls();
        assert_eq_and_println(&acc, &Ok(HashMap::new()), "empty accumulatore expected");
    }

    #[test]
    fn active_thread_locals() {
        let mut control = Control::new(HashMap::new, op, op_r);

        thread::spawn({
            let control = control.clone();
            move || {
                control.send_data((1, Foo("a".to_owned())));
                control.send_data((2, Foo("b".to_owned())));
            }
        });

        let acc = control.drain_tls();
        assert_eq!(
            acc,
            Err(ActiveThreadLocalsError),
            "error expected due to active thread(s)"
        );
    }
}
