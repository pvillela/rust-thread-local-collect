//! This module is supported on **`feature="tlcr"`** only.
//! This module supports the collection and aggregation of the values from a designated thread-local variable
//! across threads (see package [overfiew and core concepts](super)). The following features and constraints apply ...
//! - The designated thread-local variable may NOT be used in the thread responsible for
//! collection/aggregation.
//! - The linked thread-local variables hold a [`ThreadLocal`] instance that is a clone of an object of the same
//! type held in a [Control] object to accumulate thread-local values that are *sent* to it.
//! - The [`Control::drain_tls`] function can be called after
//! all participating threads, other than the thread responsible for collection/aggregation, have
//! terminated and EXPLICITLY joined, directly or indirectly, into the thread respnosible for collection.
//!
//! ## Usage pattern
//!
//! Here's an outline of how this little framework can be used:
//!
//! ```rust
//! Example goes here
//! ````
//!
//! ## Other examples
//!
//! See another example at [`examples/tlcr_map_accumulator.rs`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/tlcr_map_accumulator.rs).

use std::{
    cell::RefCell,
    error::Error,
    fmt::{Debug, Display},
    mem::replace,
    ops::Deref,
    sync::Arc,
    thread::{self, LocalKey, ThreadId},
};
use thread_local::ThreadLocal;

/// Indicates attempt to access [`Holder`] before it has been initialized.
#[derive(Debug)]
pub struct UninitializedHolderError;

impl Display for UninitializedHolderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{UninitializedHolderError:?}: Attempt to access uninitialized Holder."
        )
    }
}

impl Error for UninitializedHolderError {}

/// Controls the collection and accumulation of thread-local variables linked to this object.
///
/// `T` is the type of the values *sent* to this object and `U` is the type of the accumulated value.
/// The thread-locals must be of type [`Holder<T>`].
pub struct Control<T, U>
where
    U: Send,
{
    /// Keeps track of registered threads and accumulated value.
    state: Arc<ThreadLocal<RefCell<U>>>,
    acc_zero: Arc<dyn Fn() -> U + Send + Sync>,
    /// Binary operation that combines data from thread-locals with accumulated value.
    #[allow(clippy::type_complexity)]
    op: Arc<dyn Fn(T, &mut U, &ThreadId) + Send + Sync>,
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
    pub fn new(
        acc_zero: impl Fn() -> U + 'static + Send + Sync,
        op: impl Fn(T, &mut U, &ThreadId) + 'static + Send + Sync,
        op_r: impl Fn(U, U) -> U + 'static + Send + Sync,
    ) -> Self {
        Control {
            state: Arc::new(ThreadLocal::new()),
            acc_zero: Arc::new(acc_zero),
            op: Arc::new(op),
            op_r: Arc::new(op_r),
        }
    }

    /// Returns the accumulation of the thread-local values.
    /// Returns an error if any designated thread-local variable instance still exists, i.e., the corresponding
    /// thread has not yet explicitly joined, directly or indirectly, the thread where this
    /// function is called from.
    pub fn drain_tls(&mut self) -> Result<U, ()> {
        let state = replace(&mut self.state, Arc::new(ThreadLocal::new()));
        let unwr_state = match Arc::try_unwrap(state) {
            Ok(unwr_state) => unwr_state,
            Err(state) => {
                _ = replace(&mut self.state, state); // put it back
                return Err(());
            }
        };
        let res = unwr_state
            .into_iter()
            .map(|x| x.into_inner())
            .reduce(self.op_r.as_ref());
        res.ok_or(())
    }
}

/// Inner state of [`Holder`].
struct HolderInner<T, U>
where
    U: Send,
{
    tid: ThreadId,
    control: Control<T, U>,
}

/// Holds a thread-local [`Sender`] and a smart pointer to a [`Control`], enabling the linkage of the thread-local
/// with the control object.
///
/// `T` is the type of data sent on the channel.
pub struct Holder<T, U>(RefCell<Option<HolderInner<T, U>>>)
where
    U: Send;
// T: 'static;

impl<T, U> Holder<T, U>
where
    U: Send,
{
    /// Instantiates a holder object.
    pub fn new() -> Self {
        Self(RefCell::new(None))
    }

    /// Ensures `self` is initialized.
    fn ensure_initialized(&self, control: &Control<T, U>) {
        let mut inner = self.0.borrow_mut();
        if inner.is_none() {
            *inner = Some(HolderInner {
                tid: thread::current().id(),
                control: control.clone(),
            })
        }
    }

    /// Send data to be aggregated in the `control` object.
    fn send_data(&self, data: T) -> Result<(), UninitializedHolderError> {
        let inner_opt = self.0.borrow();
        match inner_opt.deref() {
            None => Err(UninitializedHolderError),
            Some(inner) => {
                let control = &inner.control;
                let cell = control.state.get_or(|| RefCell::new((control.acc_zero)()));
                let mut u = cell.borrow_mut();
                (control.op)(data, &mut u, &inner.tid);
                Ok(())
            }
        }
    }
}

/// Provides access to the thread-local variable.
pub trait HolderLocalKey<T, U>
where
    U: Send,
{
    /// Ensures the [`Holder`] is initialized, both the [`Sender`] and the `control` smart pointer.
    fn ensure_initialized(&'static self, control: &Control<T, U>);

    /// Send data to be aggregated by the `control` object.
    fn send_data(&'static self, data: T) -> Result<(), UninitializedHolderError>;
}

impl<T, U> HolderLocalKey<T, U> for LocalKey<Holder<T, U>>
where
    U: Send,
{
    fn ensure_initialized(&'static self, control: &Control<T, U>) {
        self.with(|h| {
            h.ensure_initialized(&control);
        })
    }

    fn send_data(&'static self, data: T) -> Result<(), UninitializedHolderError> {
        self.with(|h| h.send_data(data))
    }
}

#[cfg(test)]
mod tests {
    use super::{Control, Holder, HolderLocalKey};
    use std::{
        collections::HashMap,
        fmt::Debug,
        sync::RwLock,
        thread::{self, ThreadId},
        time::Duration,
    };

    #[derive(Debug, Clone, PartialEq)]
    struct Foo(String);

    type Data = (u32, Foo);

    type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

    thread_local! {
        static MY_TL: Holder<Data, AccValue> = Holder::new();
    }

    fn op(data: Data, acc: &mut AccValue, tid: &ThreadId) {
        println!(
            "`op` called from {:?} with data {:?}",
            thread::current().id(),
            data
        );

        acc.entry(tid.clone()).or_insert_with(|| HashMap::new());
        let (k, v) = data;
        acc.get_mut(tid).unwrap().insert(k, v.clone());
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

    fn send_tl_data(k: u32, v: Foo, control: &Control<Data, AccValue>) {
        MY_TL.ensure_initialized(control);
        MY_TL.send_data((k, v)).unwrap();
    }

    const NTHREADS: usize = 5;

    #[test]
    fn test() {
        let mut control = Control::new(|| HashMap::new(), op, op_r);

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

                            send_tl_data(1, Foo("a".to_owned() + &si), &control);
                            send_tl_data(2, Foo("b".to_owned() + &si), &control);
                        }
                    })
                })
                .collect::<Vec<_>>();

            {
                thread::sleep(Duration::from_millis(50));
                let spawned_tids = spawned_tids.try_read().unwrap();
                println!("spawned_tid={:?}", spawned_tids);
            }

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
                    let tid_i = spawned_tids[i].clone();
                    (tid_i, map_i)
                })
                .collect::<Vec<_>>();
            let map = maps.into_iter().collect::<HashMap<_, _>>();

            {
                let acc = control.drain_tls().unwrap();
                assert!(acc.eq(&map), "Accumulator check: acc={acc:?}, map={map:?}");
            }
        }
    }
}
