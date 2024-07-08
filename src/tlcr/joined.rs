//! This module supports the collection and aggregation of values across threads (see package
//! [overview and core concepts](crate)).
//! It is present only when the **"tlcr"** feature flag is enabled.
//! The following capabilities and constraints apply ...
//! - Values may be collected from the thread responsible for collection/aggregation, provided that the `control`
//! object of type [`Control`] is created on that thread and is not cloned by that thread.
//! - The participating threads update thread-local data via the clonable `control` object which contains a
//! [`ThreadLocal`](https://docs.rs/thread_local/latest/thread_local/) instance and aggregates the values.
//! - The [`Control::drain_tls`] function can be called to return the accumulated value after all participating
//! threads (other than the thread responsible for collection) have terminated (joins are not necessary).
//!
//! ## Usage pattern

//! ```rust
#![doc = include_str!("../../examples/tlcr_joined_i32_accumulator.rs")]
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
/// Method was called while some thread that contributed a value for accumulation was still active.
#[error("method called while thread-locals were arctive")]
pub struct ActiveThreadLocalsError;

/// Controls the collection and accumulation of thread-local values.
///
/// `U` is the type of the accumulated value.
///
/// This type holds the following:
/// - A state object based on [`ThreadLocal`](https://docs.rs/thread_local/latest/thread_local/struct.ThreadLocal.html).
/// - A nullary closure that produces a zero value of type `U`, which is needed to obtain consistent aggregation results.
/// - A binary operation that reduces two accumulated values into one.
pub struct Control<U>
where
    U: Send,
{
    /// Keeps track of registered threads and accumulated value.
    state: Arc<ThreadLocal<RefCell<U>>>,
    /// Produces a zero value of type `U`, which is needed to obtain consistent aggregation results.
    acc_zero: Arc<dyn Fn() -> U + Send + Sync>,
    /// Binary operation that reduces two accumulated values into one.
    op_r: Arc<dyn Fn(U, U) -> U + Send + Sync>,
}

impl<U> Clone for Control<U>
where
    U: Send,
{
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            op_r: self.op_r.clone(),
            acc_zero: self.acc_zero.clone(),
        }
    }
}

impl<U> Debug for Control<U>
where
    U: Send + Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.state)
    }
}

impl<U> Control<U>
where
    U: Send,
{
    /// Instantiates a [`Control`] object with an empty
    /// [`ThreadLocal`](https://docs.rs/thread_local/latest/thread_local/struct.ThreadLocal.html) state.
    ///
    /// - `acc_zero` - produces a zero value of type `U`, which is needed to obtain consistent aggregation results.
    /// - `op_r` - binary operation that reduces two accumulated values into one.
    pub fn new(
        acc_zero: impl Fn() -> U + 'static + Send + Sync,
        op_r: impl Fn(U, U) -> U + 'static + Send + Sync,
    ) -> Self {
        Control {
            state: Arc::new(ThreadLocal::new()),
            acc_zero: Arc::new(acc_zero),
            op_r: Arc::new(op_r),
        }
    }

    /// Called from a thread to access the thread's local accumulated value.
    pub fn with_tl_acc<V>(&self, f: impl FnOnce(&U) -> V) -> V {
        let cell = self.state.get_or(|| RefCell::new((self.acc_zero)()));
        let u = cell.borrow();
        f(&u)
    }

    /// Called from a thread to mutably access the thread's local accumulated value.
    pub fn with_tl_acc_mut<V>(&self, f: impl FnOnce(&mut U) -> V) -> V {
        let cell = self.state.get_or(|| RefCell::new((self.acc_zero)()));
        let mut u = cell.borrow_mut();
        f(&mut u)
    }

    /// Called from a thread to aggregate data with aggregation operation `op`.
    pub fn aggregate_data<T>(&self, data: T, op: impl FnOnce(T, &mut U, ThreadId)) {
        self.with_tl_acc_mut(|acc| op(data, acc, thread::current().id()))
    }

    /// Returns the accumulation of the thread-local values, restoring `self`'s state to what it was when
    /// it was instantiated with [`Control::new`].
    ///
    /// # Errors
    /// - Returns an error if any thread, other than the thread where this function is called from,
    /// holds a clone of `self`. In this case, the state of `self` is left unchanged.
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
    use crate::dev_support::assert_eq_and_println;
    use std::{
        collections::HashMap,
        fmt::Debug,
        iter::once,
        thread::{self, ThreadId},
        time::Duration,
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

    const NTHREADS: usize = 5;

    #[test]
    fn own_thread_and_explicit_joins() {
        let mut control = Control::new(HashMap::new, op_r);

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
            let map = once((tid_own, map_own))
                .chain(tid_map_pairs)
                .collect::<HashMap<_, _>>();

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
            assert_eq_and_println(&acc, &Ok(map), "take_acc - control reused");
        }
    }

    #[test]
    fn own_thread_only() {
        let mut control = Control::new(HashMap::new, op_r);

        control.aggregate_data((1, Foo("a".to_owned())), op);
        control.aggregate_data((2, Foo("b".to_owned())), op);

        let tid_own = thread::current().id();
        let map_own = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);

        let map = HashMap::from([(tid_own, map_own)]);

        let acc = control.drain_tls();
        assert_eq_and_println(&acc, &Ok(map), "Accumulator check");
    }

    #[test]
    fn no_thread() {
        let mut control = Control::new(HashMap::new, op_r);
        let acc = control.drain_tls();
        assert_eq_and_println(&acc, &Ok(HashMap::new()), "empty accumulatore expected");
    }

    #[test]
    fn active_thread_locals() {
        let mut control = Control::new(HashMap::new, op_r);

        thread::spawn({
            let control = control.clone();
            move || {
                control.aggregate_data((1, Foo("a".to_owned())), op);
                control.aggregate_data((2, Foo("b".to_owned())), op);

                thread::sleep(Duration::from_millis(10));
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
