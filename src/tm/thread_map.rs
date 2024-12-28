use std::{
    cell::UnsafeCell,
    collections::HashMap,
    error::Error,
    fmt::{Debug, Display},
    mem::take,
    ops::DerefMut,
    sync::{PoisonError, RwLock},
    thread::{self, ThreadId},
};

/// Wrapper to enable cell to be used as value in `HashMap`.
struct UnsafeSyncCell<V>(UnsafeCell<V>);

/// SAFETY:
/// An instance is only accessed privately by [`ThreadMap`], in two ways:
/// - Under a [`ThreadMap`] instance read lock, always in the same thread.
/// - Under a [`ThreadMap`] instance write lock, from an arbitrary thread.
unsafe impl<V> Sync for UnsafeSyncCell<V> {}

impl<V: Debug> Debug for UnsafeSyncCell<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{:?}", unsafe { &*self.0.get() }))
    }
}

/// Error emitted when a [`ThreadMap`] method attempts to acquire its internal lock and the lock is poisoned.
#[derive(Debug)]
pub struct ThreadMapLockError;

impl Display for ThreadMapLockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (self as &dyn Debug).fmt(f)
    }
}

impl Error for ThreadMapLockError {}

impl<T> From<PoisonError<T>> for ThreadMapLockError {
    fn from(_: PoisonError<T>) -> Self {
        Self
    }
}

/// This type encapsulates the association of [`ThreadId`]s to values of type `V`. It is a simple and easy-to-use alternative
/// to the [`std::thread_local`] macro and the [`thread_local`](https://crates.io/crates/thread_local) crate.
#[derive(Debug)]
pub struct ThreadMap<V> {
    state: RwLock<HashMap<ThreadId, UnsafeSyncCell<V>>>,
    value_init: fn() -> V,
}

impl<V> ThreadMap<V> {
    /// Creates a new [`ThreadMap`] instance, with `value_init` used to create the initial value for each thread.
    pub fn new(value_init: fn() -> V) -> Self {
        Self {
            state: RwLock::new(HashMap::new()),
            value_init,
        }
    }

    /// Invokes `f` mutably on the value associated with the [`ThreadId`] of the current thread and returns the invocation result.
    /// If there is no value associated with the current thread then the `value_init` argument of [`Self::new`] is used
    /// to instantiate an initial associated value before `f` is applied.
    pub fn with_mut<W>(&self, f: impl FnOnce(&mut V) -> W) -> W {
        let lock = self.state.read().expect("unable to get read lock");
        let tid = thread::current().id();
        match lock.get(&tid) {
            Some(c) => {
                let v = c.0.get();
                // SAFETY: call below is always done in the thread with `ThreadId` `tid`, under an instance-level read lock.
                // all other access to the cell is done under an instance-level write lock.
                let rv = unsafe { &mut *v };
                f(rv)
            }
            None => {
                // Drop read lock and acquire write lock.
                drop(lock);
                let mut lock = self.state.write().expect("unable to get write lock");
                let mut v0 = (self.value_init)();
                let w = f(&mut v0);
                lock.insert(tid, UnsafeSyncCell(UnsafeCell::new(v0)));
                w
            }
        }
    }

    /// Invokes `f` on the value associated with the [`ThreadId`] of the current thread and returns the invocation result.
    /// If there is no value associated with the current thread then the `value_init` argument of [`Self::new`] is used
    /// to instantiate an initial associated value before `f` is applied.
    pub fn with<W>(&self, f: impl FnOnce(&V) -> W) -> W {
        let g = |v: &mut V| f(v);
        self.with_mut(g)
    }

    /// Returns a [`HashMap`] with the values associated with each [`ThreadId`] key and clears `self`'s state.
    ///
    /// # Errors
    /// - [`ThreadMapLockError`] if the internal lock is poisoned.
    pub fn drain(&self) -> Result<HashMap<ThreadId, V>, ThreadMapLockError> {
        let mut lock = self.state.write()?;
        let rmap = lock.deref_mut();
        let tmap = take(rmap);
        let map = tmap
            .into_iter()
            .map(|(k, v)| (k, v.0.into_inner()))
            .collect::<HashMap<_, _>>();
        Ok(map)
    }

    /// Folds every association in `self` into an accumulator (with initial value `z`) by applying an operation `f`,
    /// returning the final result.
    ///
    /// # Errors
    /// - [`ThreadMapLockError`] if the internal lock is poisoned.
    pub fn fold<W>(
        &self,
        z: W,
        f: impl FnMut(W, (ThreadId, &V)) -> W,
    ) -> Result<W, ThreadMapLockError> {
        let w = self
            .state
            .write()?
            .iter()
            .map(|(tid, c)| {
                let v = c.0.get();
                // SAFETY: call below is always done under an instance-level write lock.
                (*tid, unsafe { &*v })
            })
            .fold(z, f);
        Ok(w)
    }

    #[allow(unused)]
    /// Folds every value in `self` into an accumulator (with initial value `z`) by applying an operation `f`,
    /// returning the final result.
    ///
    /// # Errors
    /// - [`ThreadMapLockError`] if the internal lock is poisoned.
    pub fn fold_values<W>(
        &self,
        z: W,
        mut f: impl FnMut(W, &V) -> W,
    ) -> Result<W, ThreadMapLockError> {
        self.fold(z, |w, (_, v)| f(w, v))
    }

    #[allow(unused)]
    /// Returns a [`HashMap`] with clones of the values associated with each [`ThreadId`] key at the time the probe
    /// was executed.
    ///
    /// # Errors
    /// - [`ThreadMapLockError`] if the internal lock is poisoned.
    pub fn probe(&self) -> Result<HashMap<ThreadId, V>, ThreadMapLockError>
    where
        V: Clone,
    {
        let z = HashMap::<ThreadId, V>::new();
        self.fold(z, |mut w, (tid, v)| {
            w.insert(tid, v.clone());
            w
        })
    }
}

#[cfg(test)]
mod test {
    use std::{
        collections::HashMap,
        thread::{self},
        time::Duration,
    };

    use super::ThreadMap;

    const NTHREADS: i32 = 20;
    const NITER: i32 = 10;
    const SLEEP_MICROS: u64 = 10;

    fn value_constr() -> (i32, i32) {
        (0, 0)
    }

    fn g((i0, v0): &mut (i32, i32), i: i32) {
        *i0 = i;
        *v0 += i;
    }

    #[test]
    fn test() {
        let tm = ThreadMap::new(value_constr);

        thread::scope(|s| {
            let tm = &tm;
            for i in 0..NTHREADS {
                let f = move |p: &mut (i32, i32)| g(p, i);
                s.spawn(move || {
                    for _ in 0..NITER {
                        thread::sleep(Duration::from_micros(SLEEP_MICROS));
                        tm.with_mut(f)
                    }
                });
            }

            let probed = tm.probe().unwrap().into_values().collect::<HashMap<_, _>>();
            println!("probed={probed:?}");

            let f = move |p: &mut (i32, i32)| g(p, NTHREADS);
            for _ in 0..NITER {
                tm.with_mut(f)
            }

            let probed = tm.probe().unwrap().into_values().collect::<HashMap<_, _>>();
            println!("probed={probed:?}");
        });

        let expected = (0..=NTHREADS)
            .map(|i| (i, i * NITER))
            .collect::<HashMap<_, _>>();

        let probed = tm.probe().unwrap().into_values().collect::<HashMap<_, _>>();
        println!("probed={probed:?}");

        assert_eq!(expected, probed);

        let dumped = tm.drain().unwrap().into_values().collect::<HashMap<_, _>>();

        assert_eq!(expected, dumped);
    }
}
