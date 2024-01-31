use std::{
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    fmt::Debug,
    mem::replace,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, ThreadId},
};

pub trait Param {
    type Dat;
    type Acc;
    type Node;
    type GData;
    type Discr;
}

// #[derive(Debug)]
pub struct AccGuardG<'a, P: Param> {
    guard: MutexGuard<'a, ControlStateG<P>>,
}

impl<'a, P: Param> AccGuardG<'a, P> {
    pub(crate) fn new(lock: MutexGuard<'a, ControlStateG<P>>) -> Self {
        AccGuardG { guard: lock }
    }
}

impl<P: Param> Deref for AccGuardG<'_, P> {
    type Target = P::Acc;

    fn deref(&self) -> &Self::Target {
        self.guard.acc()
    }
}

#[derive(Debug)]
pub(crate) struct ControlStateG<P: Param> {
    pub(crate) acc: P::Acc,
    pub(crate) d: P::Discr,
}

impl<P: Param> ControlStateG<P> {
    fn acc(&self) -> &P::Acc {
        &self.acc
    }

    fn acc_mut(&mut self) -> &mut P::Acc {
        &mut self.acc
    }
}

#[derive(Debug)]
pub struct TmapD<P: Param> {
    pub(crate) tmap: HashMap<ThreadId, P::Node>,
}

impl<P: Param<Discr = TmapD<P>>> ControlStateG<P> {
    pub fn new(acc_base: P::Acc) -> Self {
        Self {
            acc: acc_base,
            d: TmapD {
                tmap: HashMap::new(),
            },
        }
    }

    fn register_node(&mut self, node: P::Node, tid: &ThreadId) {
        self.d.tmap.insert(tid.clone(), node);
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
        self.d.tmap.remove(tid);
        if let Some(data) = data {
            let acc = self.acc_mut();
            op(data, acc, tid);
        }
    }
}

/// Controls the destruction of thread-local values registered with it.
/// Such values of type `T` must be held in thread-locals of type [`Holder<T>`].
/// `U` is the type of the accumulated value resulting from an initial base value and
/// the application of an operation to each thread-local value and the current accumulated
/// value upon dropping of each thread-local value. (See [`new`](Control::new) method.)
pub struct ControlG<P: Param> {
    /// Keeps track of registered threads and accumulated value.
    pub(crate) state: Arc<Mutex<ControlStateG<P>>>,
    /// Binary operation that combines data from thread-locals with accumulated value.
    #[allow(clippy::type_complexity)]
    pub(crate) op: Arc<dyn Fn(P::Dat, &mut P::Acc, &ThreadId) + Send + Sync>,
}

/// Locking functionality underlying [`ControlC`].
impl<P: Param> ControlG<P> {
    /// Acquires a lock for use by public `Control` methods that require its internal Mutex to be locked.
    ///
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub(crate) fn lock(&self) -> MutexGuard<'_, ControlStateG<P>> {
        self.state.lock().unwrap()
    }

    pub fn acc(&self) -> AccGuardG<'_, P> {
        AccGuardG::new(self.lock())
    }

    /// Provides access to the accumulated value in the [Control] struct.
    pub fn with_acc<V>(&self, f: impl FnOnce(&P::Acc) -> V) -> V {
        let acc = self.acc();
        f(&acc)
    }

    /// Returns the accumulated value in the [Control] struct, using a value of the same type to replace
    /// the existing accumulated value.
    pub fn take_acc(&self, replacement: P::Acc) -> P::Acc {
        let mut lock = self.lock();
        let acc = lock.acc_mut();
        replace(acc, replacement)
    }
}

/// Locking functionality underlying [`ControlC`].
impl<P: Param<Discr = TmapD<P>>> ControlG<P> {
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

#[doc(hidden)]
pub trait GuardedData<S: 'static> {
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
pub struct HolderG<P>
where
    P: Param,
    P::Dat: 'static,
    P::GData: GuardedData<Option<P::Dat>> + 'static,
{
    pub(crate) data: P::GData,
    pub(crate) control: RefCell<Option<ControlG<P>>>,
    pub(crate) make_data: fn() -> P::Dat,
    pub(crate) init_control_fn: fn(this: &Self, control: &ControlG<P>, node: P::Node),
    pub(crate) drop_data_fn: fn(&Self),
}

/// Common trait supporting different `Holder` implementations.
impl<P> HolderG<P>
where
    P: Param,
    P::GData: GuardedData<Option<P::Dat>> + 'static,
{
    pub(crate) fn control(&self) -> Ref<'_, Option<ControlG<P>>> {
        self.control.borrow()
    }

    fn make_data(&self) -> P::Dat {
        (self.make_data)()
    }

    pub(crate) fn data_guard(&self) -> <P::GData as GuardedData<Option<P::Dat>>>::Guard<'_> {
        self.data.guard()
    }

    pub(crate) fn init_control(&self, control: &ControlG<P>, node: P::Node) {
        (self.init_control_fn)(self, control, node)
    }

    pub(crate) fn init_data(&self) {
        let mut guard = self.data_guard();
        if guard.is_none() {
            *guard = Some(self.make_data());
        }
    }

    pub(crate) fn ensure_initialized(&self, control: &ControlG<P>, node: P::Node) {
        if self.control().as_ref().is_none() {
            self.init_control(control, node);
        }

        if self.data_guard().is_none() {
            self.init_data();
        }
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

    fn drop_data(&self) {
        (self.drop_data_fn)(self)
    }
}

impl<P: Param<Discr = TmapD<P>>> HolderG<P>
where
    P: Param,
    P::GData: GuardedData<Option<P::Dat>> + 'static,
{
    /// Establishes link with control.
    pub(crate) fn init_control_fn(this: &Self, control: &ControlG<P>, node: P::Node) {
        let mut ctrl_ref = this.control.borrow_mut();
        *ctrl_ref = Some(control.clone());
        control.register_node(node, &thread::current().id())
    }

    pub(crate) fn drop_data_fn(this: &Self) {
        let mut data_guard = this.data_guard();
        let data = data_guard.take();
        let control = this.control();
        if control.is_none() {
            return;
        }
        let control = Ref::map(control, |x| x.as_ref().unwrap());
        control.tl_data_dropped(data, &thread::current().id());
    }
}

/// Provides access to `Holder` functionality across the different framework modules
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

impl<P: Param> Clone for ControlG<P> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            op: self.op.clone(),
        }
    }
}

impl<P> Debug for ControlG<P>
where
    P: Param + Debug,
    P::Acc: Debug,
    P::Node: Debug,
    P::Discr: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Control({:?})", self.state))
    }
}

impl<P> Debug for HolderG<P>
where
    P::GData: GuardedData<Option<P::Dat>> + Debug,
    P: Param + Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Holder{{data: {:?}}}", &self.data))
    }
}

impl<P> Drop for HolderG<P>
where
    P: Param,
    P::GData: GuardedData<Option<P::Dat>> + 'static,
{
    /// Ensures the held data, if any, is deregistered from the associated [`Control`] instance
    /// and the control instance's accumulation operation is invoked with the held data.
    fn drop(&mut self) {
        self.drop_data()
    }
}
