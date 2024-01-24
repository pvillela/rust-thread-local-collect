use std::{
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    fmt::Debug,
    mem::replace,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, ThreadId},
};

pub(crate) trait Param {
    type Dat;
    type Acc;
    type Node;
}

// #[derive(Debug)]
pub(crate) struct AccGuardS<'a, P: Param> {
    guard: MutexGuard<'a, ControlStateS<P>>,
}

impl<'a, P: Param> AccGuardS<'a, P> {
    pub(crate) fn new(lock: MutexGuard<'a, ControlStateS<P>>) -> Self {
        AccGuardS { guard: lock }
    }
}

impl<P: Param> Deref for AccGuardS<'_, P> {
    type Target = P::Acc;

    fn deref(&self) -> &Self::Target {
        self.guard.acc()
    }
}

#[derive(Debug)]
pub(crate) struct ControlStateS<P: Param> {
    pub(crate) acc: P::Acc,
    pub(crate) tmap: HashMap<ThreadId, P::Node>,
}

impl<P: Param> ControlStateS<P> {
    fn acc(&self) -> &P::Acc {
        &self.acc
    }

    fn acc_mut(&mut self) -> &mut P::Acc {
        &mut self.acc
    }

    fn register_node(&mut self, node: P::Node, tid: &ThreadId) {
        self.tmap.insert(tid.clone(), node);
    }

    /// Used by a `Holder` to notify its `Control` that the holder's data has been dropped.
    ///
    /// The `data` argument is defined mostly for the beefit of module [`crate::simple_joined`]
    /// and would have been unnecessary if the framework implemented only modules
    /// [`crate::joined`] and [`crate::probed`]. Having said that, the `data` argument makes
    /// the generic implementation simpler and uniform across all modules. Without the `data`
    /// argument, the implementations for [`crate::joined`] and [`crate::probed`] would be
    /// different from each other and a bit more complex.
    fn tl_data_dropped(
        &mut self,
        op: &(dyn Fn(P::Dat, &mut P::Acc, &ThreadId) + Send + Sync),
        data: Option<P::Dat>,
        tid: &ThreadId,
    ) {
        self.tmap.remove(tid);
        if let Some(data) = data {
            let acc = self.acc_mut();
            op(data, acc, tid);
        }
    }
}

impl<P: Param> ControlStateS<P> {
    pub(crate) fn new(acc: P::Acc) -> Self {
        Self {
            acc,
            tmap: HashMap::new(),
        }
    }
}

/// Controls the destruction of thread-local values registered with it.
/// Such values of type `T` must be held in thread-locals of type [`Holder<T>`].
/// `U` is the type of the accumulated value resulting from an initial base value and
/// the application of an operation to each thread-local value and the current accumulated
/// value upon dropping of each thread-local value. (See [`new`](Control::new) method.)
pub(crate) struct ControlS<P: Param> {
    /// Keeps track of registered threads and accumulated value.
    pub(crate) state: Arc<Mutex<ControlStateS<P>>>,
    /// Binary operation that combines data from thread-locals with accumulated value.
    #[allow(clippy::type_complexity)]
    pub(crate) op: Arc<dyn Fn(P::Dat, &mut P::Acc, &ThreadId) + Send + Sync>,
}

/// Locking functionality underlying [`ControlC`].
impl<P: Param> ControlS<P> {
    /// Acquires a lock for use by public `Control` methods that require its internal Mutex to be locked.
    ///
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub(crate) fn lock(&self) -> MutexGuard<'_, ControlStateS<P>> {
        self.state.lock().unwrap()
    }

    pub(crate) fn acc(&self) -> AccGuardS<'_, P> {
        AccGuardS::new(self.lock())
    }

    /// Provides access to the accumulated value in the [Control] struct.
    pub(crate) fn with_acc<V>(&self, f: impl FnOnce(&P::Acc) -> V) -> V {
        let acc = self.acc();
        f(&acc)
    }

    /// Returns the accumulated value in the [Control] struct, using a value of the same type to replace
    /// the existing accumulated value.
    pub(crate) fn take_acc(&self, replacement: P::Acc) -> P::Acc {
        let mut lock = self.lock();
        let acc = lock.acc_mut();
        replace(acc, replacement)
    }

    fn register_node(&self, node: P::Node, tid: &ThreadId) {
        let mut lock = self.lock();
        lock.register_node(node, tid)
    }

    /// Used by a `Holder` to notify its `Control` that the holder's data has been dropped.
    fn tl_data_dropped(&self, data: Option<P::Dat>, tid: &ThreadId) {
        let mut lock = self.lock();
        lock.tl_data_dropped(self.op.deref(), data, tid);
    }
}

pub(crate) trait GuardedData<S: 'static> {
    type Guard<'a>: DerefMut<Target = S> + 'a
    where
        Self: 'a;

    fn guard<'a>(&'a self) -> Self::Guard<'a>;
}

impl<S: 'static> GuardedData<S> for RefCell<S> {
    type Guard<'a> = RefMut<'a, S>;

    fn guard<'a>(&'a self) -> Self::Guard<'a> {
        self.borrow_mut()
    }
}

impl<S: 'static> GuardedData<S> for Arc<Mutex<S>> {
    type Guard<'a> = MutexGuard<'a, S>;

    fn guard<'a>(&'a self) -> Self::Guard<'a> {
        self.lock().unwrap()
    }
}

/// Holds thead-local data to enable registering it with [`Control`].
pub(crate) struct HolderS<GData, P>
where
    GData: GuardedData<Option<P::Dat>> + 'static,
    P: Param,
    P::Dat: 'static,
{
    pub(crate) data: GData,
    pub(crate) control: RefCell<Option<ControlS<P>>>,
    pub(crate) make_data: fn() -> P::Dat,
}

/// Common trait supporting different `Holder` implementations.
impl<GData, P> HolderS<GData, P>
where
    GData: GuardedData<Option<P::Dat>> + 'static,
    P: Param,
{
    fn control(&self) -> Ref<'_, Option<ControlS<P>>> {
        self.control.borrow()
    }

    fn make_data(&self) -> P::Dat {
        (self.make_data)()
    }

    fn data_guard(&self) -> GData::Guard<'_> {
        self.data.guard()
    }

    /// Establishes link with control.
    pub(crate) fn init_control(&self, control: &ControlS<P>, node: P::Node) {
        let mut ctrl_ref = self.control.borrow_mut();
        *ctrl_ref = Some(control.clone());
        control.register_node(node, &thread::current().id())
    }

    pub(crate) fn init_data(&self) {
        let mut guard = self.data_guard();
        if guard.is_none() {
            *guard = Some(self.make_data());
        }
    }

    pub(crate) fn ensure_initialized(&self, control: &ControlS<P>, node: P::Node) {
        if self.control().as_ref().is_none() {
            self.init_control(control, node);
        }

        if self.data_guard().is_none() {
            self.init_data();
        }
    }

    fn drop_data(&self) {
        let mut data_guard = self.data_guard();
        let data = data_guard.take();
        let control: Ref<'_, Option<ControlS<P>>> = self.control();
        if control.is_none() {
            return;
        }
        let control = Ref::map(control, |x| x.as_ref().unwrap());
        control.tl_data_dropped(data, &thread::current().id());
    }

    /// Invokes `f` on data. Panics if data is [`None`].
    pub(crate) fn with_data<V>(&self, f: impl FnOnce(&P::Dat) -> V) -> V {
        let guard = self.data_guard();
        // f(guard.unwrap()) // instead of 2 lines below
        let data: Option<&P::Dat> = guard.as_ref();
        f(data.unwrap())
    }

    /// Invokes `f` on data. Panics if data is [`None`].
    pub(crate) fn with_data_mut<V>(&self, f: impl FnOnce(&mut P::Dat) -> V) -> V {
        let mut guard = self.data_guard();
        // f(guard.unwrap_mut()) // instead of 2 lines below
        let data: Option<&mut P::Dat> = guard.as_mut();
        f(data.unwrap())
    }
}

/// Provides convenient access to `Holder` functionality across the different framework modules
/// ([`crate::joined`], [`crate::simple_joined`], [`crate::probed`]).
///
/// - `Ctrl` is the module-specific Control type.
/// - References to `Holder` in methods refer to the module-specific Holder type.
pub trait HolderLocalKey<T, Ctrl> {
    /// Establishes link with control.
    fn init_control(&'static self, control: &Ctrl);

    /// Initializes `Holder` data.
    fn init_data(&'static self);

    /// Ensures `Holder` is properly initialized (both data and control link) by initializing it if not.
    fn ensure_initialized(&'static self, control: &Ctrl);

    /// Invokes `f` on `Holder` data. Panics if data is not initialized.
    fn with_data<V>(&'static self, f: impl FnOnce(&T) -> V) -> V;

    /// Invokes `f` on `Holder` data. Panics if data is not initialized.
    fn with_data_mut<V>(&'static self, f: impl FnOnce(&mut T) -> V) -> V;
}

impl<P: Param> Clone for ControlS<P> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            op: self.op.clone(),
        }
    }
}

impl<P> Debug for ControlS<P>
where
    P: Param + Debug,
    P::Acc: Debug,
    P::Node: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Control({:?})", self.state))
    }
}

impl<GData, P> Debug for HolderS<GData, P>
where
    GData: GuardedData<Option<P::Dat>> + Debug,
    P: Param + Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Holder{{data: {:?}}}", &self.data))
    }
}

impl<GData, P> Drop for HolderS<GData, P>
where
    P: Param,
    GData: GuardedData<Option<P::Dat>> + 'static,
{
    /// Ensures the held data, if any, is deregistered from the associated [`Control`] instance
    /// and the control instance's accumulation operation is invoked with the held data.
    fn drop(&mut self) {
        self.drop_data()
    }
}
