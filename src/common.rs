use std::{
    cell::{Ref, RefCell},
    fmt::Debug,
    mem::replace,
    ops::DerefMut,
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, ThreadId},
};

pub(crate) trait ControlInner<U> {
    fn acc(&mut self) -> &mut U;
}

/// Locking functionality underlying [`ControlC`].
pub(crate) trait ControlPart {
    type Dat: 'static;
    type Acc: 'static;
    type Inner: ControlInner<Self::Acc>;
    type Lock<'a>: DerefMut<Target = Self::Inner> + 'a
    where
        Self: 'a;

    fn op(&self, data: Self::Dat, acc: &mut Self::Acc, tid: &ThreadId);

    /// Acquires a lock for use by public `Control` methods that require its internal Mutex to be locked.
    ///
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    fn lock(&self) -> Self::Lock<'_>;

    fn accumulate_tl(&self, lock: &mut Self::Lock<'_>, data: Self::Dat, tid: &ThreadId) {
        let acc = lock.deref_mut().acc();
        self.op(data, acc, tid);
    }

    /// Provides access to the accumulated value in the [Control] struct.
    ///
    /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    fn with_acc<V>(&self, lock: &mut Self::Lock<'_>, f: impl FnOnce(&Self::Acc) -> V) -> V {
        let acc = lock.deref_mut().acc();
        f(acc)
    }

    /// Returns the accumulated value in the [Control] struct, using a value of the same type to replace
    /// the existing accumulated value.
    ///
    /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    fn take_acc(&self, lock: &mut Self::Lock<'_>, replacement: Self::Acc) -> Self::Acc {
        let acc = lock.deref_mut().acc();
        replace(acc, replacement)
    }
}

/// Common trait supporting different `Control` implementations.
pub(crate) trait ControlBase: ControlPart {
    /// Forces all registered thread-local values that have not already been dropped to be effectively dropped
    /// by replacing the [`Holder`] data with [`None`], and accumulates the values contained in those thread-locals.
    ///
    /// Should only be called from a thread (typically the main thread) under the following conditions:
    /// - All other threads that use this [`Control`] instance must have been directly or indirectly spawned
    ///   from this thread; ***and***
    /// - Any prior updates to holder values must have had a *happened before* relationship to this call;
    ///   ***and***
    /// - Any further updates to holder values must have a *happened after* relationship to this call.
    ///   
    /// In particular, the last two conditions are satisfied if the call to this method takes place after
    /// this thread joins (directly or indirectly) with all threads that have registered with this [`Control`]
    /// instance.
    ///
    /// These conditions ensure the absence of data races with a proper "happens-before" condition between any
    /// thread-local data updates and this call.
    ///
    /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    fn ensure_tls_dropped(&self, lock: &mut Self::Lock<'_>);

    fn deregister_thread(&self, lock: &mut Self::Lock<'_>, tid: &ThreadId);

    fn tl_data_dropped(&self, tid: &ThreadId, data: Option<Self::Dat>) {
        let mut lock = self.lock();
        self.deregister_thread(&mut lock, tid);
        if let Some(data) = data {
            self.accumulate_tl(&mut lock, data, tid);
        }
    }
}

/// Controls the destruction of thread-local values registered with it.
/// Such values of type `T` must be held in thread-locals of type [`Holder<T>`].
/// `U` is the type of the accumulated value resulting from an initial base value and
/// the application of an operation to each thread-local value and the current accumulated
/// value upon dropping of each thread-local value. (See [`new`](Control::new) method.)
pub struct ControlS<T, U, Inner> {
    /// Keeps track of registered threads and accumulated value.
    pub(crate) inner: Arc<Mutex<Inner>>,
    /// Binary operation that combines data from thread-locals with accumulated value.
    #[allow(clippy::type_complexity)]
    pub(crate) op: Arc<dyn Fn(T, &mut U, &ThreadId) + Send + Sync>,
}

impl<T, U, Inner> ControlPart for ControlS<T, U, Inner>
where
    T: 'static,
    U: 'static,
    Inner: ControlInner<U> + 'static,
{
    type Dat = T;
    type Acc = U;
    type Inner = Inner;
    type Lock<'a> = MutexGuard<'a, Inner>;

    fn op(&self, data: T, acc: &mut Self::Acc, tid: &ThreadId) {
        (self.op)(data, acc, tid)
    }

    fn lock<'a>(&'a self) -> Self::Lock<'a> {
        self.inner.lock().unwrap()
    }
}

impl<T, U, Inner> Clone for ControlS<T, U, Inner> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            op: self.op.clone(),
        }
    }
}

impl<T: Debug, U: Debug, Inner: Debug> Debug for ControlS<T, U, Inner> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Control({:?})", self.inner))
    }
}

/// Common trait supporting different `Holder` implementations.
pub(crate) trait HolderBase {
    type Dat;
    type Ctrl: ControlBase<Dat = Self::Dat> + Clone;
    type Guard<'a>: DerefMut<Target = Option<Self::Dat>> + 'a
    where
        Self: 'a;

    fn control(&self) -> Ref<'_, Option<Self::Ctrl>>;

    fn data_guard<'a>(&'a self) -> Self::Guard<'a>;

    fn make_data(&self) -> Self::Dat;

    /// Establishes link with control.
    fn init_control(&self, control: &Self::Ctrl);

    fn init_data(&self) {
        let mut guard = self.data_guard();
        let data = guard.deref_mut();
        if data.is_none() {
            *data = Some(self.make_data());
        }
    }

    fn ensure_initialized(&self, control: &Self::Ctrl) {
        if self.control().as_ref().is_none() {
            self.init_control(control);
        }

        if self.data_guard().is_none() {
            self.init_data();
        }
    }

    fn drop_data(&self) {
        let mut data_guard = self.data_guard();
        let data = data_guard.take();
        let control: Ref<'_, Option<Self::Ctrl>> = self.control();
        if control.is_none() {
            return;
        }
        let control = Ref::map(control, |x| x.as_ref().unwrap());
        control.tl_data_dropped(&thread::current().id(), data);
    }

    /// Invokes `f` on data. Panics if data is [`None`].
    fn with_data<V>(&self, f: impl FnOnce(&Self::Dat) -> V) -> V {
        let guard = self.data_guard();
        // f(guard.unwrap()) // instead of 2 lines below
        let data: Option<&Self::Dat> = guard.as_ref();
        f(data.unwrap())
    }

    /// Invokes `f` on data. Panics if data is [`None`].
    fn with_data_mut<V>(&self, f: impl FnOnce(&mut Self::Dat) -> V) -> V {
        let mut guard = self.data_guard();
        // f(guard.unwrap_mut()) // instead of 2 lines below
        let data: Option<&mut Self::Dat> = guard.as_mut();
        f(data.unwrap())
    }
}

pub(crate) trait GuardedData<S: 'static> {
    type Guard<'a>: DerefMut<Target = S> + 'a
    where
        Self: 'a;

    fn guard<'a>(&'a self) -> Self::Guard<'a>;
}

/// Holds thead-local data to enable registering it with [`Control`].
pub(crate) struct HolderS<T, Ctrl, GData>
where
    T: 'static,
    Ctrl: ControlBase<Dat = T> + Clone + 'static,
    GData: GuardedData<Option<T>> + 'static,
{
    pub(crate) data: GData,
    pub(crate) control: RefCell<Option<Ctrl>>,
    pub(crate) make_data: fn() -> T,
}

impl<T, Ctrl, GData> HolderBase for HolderS<T, Ctrl, GData>
where
    T: 'static,
    Ctrl: ControlBase<Dat = T> + Clone + 'static,
    GData: GuardedData<Option<T>> + 'static,
{
    type Dat = T;
    type Ctrl = Ctrl;
    type Guard<'a> = GData::Guard<'a>;

    fn control(&self) -> Ref<'_, Option<Self::Ctrl>> {
        self.control.borrow()
    }

    fn make_data(&self) -> T {
        (self.make_data)()
    }

    fn data_guard(&self) -> Self::Guard<'_> {
        self.data.guard()
    }

    fn init_control(&self, control: &Self::Ctrl) {
        let mut ctrl = self.control.borrow_mut();
        *ctrl = Some(control.clone());
    }
}

impl<'a, T, Ctrl, GData> Debug for HolderS<T, Ctrl, GData>
where
    T: Debug,
    Ctrl: ControlBase<Dat = T> + Clone,
    GData: GuardedData<Option<T>> + Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Holder{{data: {:?}}}", &self.data))
    }
}

impl<T, Ctrl, GData> Drop for HolderS<T, Ctrl, GData>
where
    T: 'static,
    Ctrl: ControlBase<Dat = T> + Clone + 'static,
    GData: GuardedData<Option<T>> + 'static,
{
    /// Ensures the held data, if any, is deregistered from the associated [`Control`] instance
    /// and the control instance's accumulation operation is invoked with the held data.
    fn drop(&mut self) {
        self.drop_data()
    }
}

pub trait DerefMutOption<T>: DerefMut<Target = Option<T>> {
    fn unwrap(&self) -> &T;

    fn unwrap_mut(&mut self) -> &mut T;
}

impl<T, X> DerefMutOption<T> for X
where
    X: DerefMut<Target = Option<T>>,
{
    fn unwrap(&self) -> &T {
        self.as_ref().unwrap()
    }

    fn unwrap_mut(&mut self) -> &mut T {
        self.as_mut().unwrap()
    }
}
