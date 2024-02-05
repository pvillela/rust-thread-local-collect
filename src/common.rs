//! This module contains common traits and structs that are used by the other modules in this libray.

use std::{
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    fmt::Debug,
    mem::replace,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, ThreadId},
};

//=================
// Common items that support all modules except `channeled`

/// Encapsulates the types used by [`ControlG`], [`HolderG`], and their specializations.
pub trait CoreParam {
    /// Type of data held in thread-local variable.
    type Dat;
    /// Type of accumulated value.
    type Acc;
    /// Discriminant type used to enable different specializations of the generic data structures.
    type Discr;
}

#[doc(hidden)]
pub trait NodeParam {
    /// Type of node in [`TmapD`] and [`NodeD`].
    type Node;
}

#[doc(hidden)]
pub trait GDataParam {
    /// Guarded data type used by [`HolderG`].
    type GData;
}

#[doc(hidden)]
pub trait CtrlStateCore<P>
where
    P: CoreParam,
{
    fn acc(&self) -> &P::Acc;

    fn acc_mut(&mut self) -> &mut P::Acc;

    fn tl_data_dropped(
        &mut self,
        op: &(dyn Fn(P::Dat, &mut P::Acc, &ThreadId) + Send + Sync),
        data: Option<P::Dat>,
        tid: &ThreadId,
    );
}

#[doc(hidden)]
pub trait CtrlStateWithNode<P>: CtrlStateCore<P>
where
    P: CoreParam + NodeParam,
{
    fn register_node(&mut self, node: P::Node, tid: &ThreadId);
}

pub trait CtrlStateParam {
    type CtrlState;
}

/// Data structure that holds the state of a [`ControlG`].
#[derive(Debug)]
pub(crate) struct ControlStateG<P: CoreParam> {
    pub(crate) acc: P::Acc,
    pub(crate) d0: P::Discr,
}

impl<P: CoreParam> ControlStateG<P> {
    fn acc(&self) -> &P::Acc {
        &self.acc
    }

    pub(crate) fn acc_mut(&mut self) -> &mut P::Acc {
        &mut self.acc
    }
}

/// Guard that dereferences to [`Param::Acc`].
pub struct AccGuardG<'a, P>
where
    P: CoreParam + CtrlStateParam,
{
    guard: MutexGuard<'a, P::CtrlState>,
}

impl<'a, P> AccGuardG<'a, P>
where
    P: CoreParam + CtrlStateParam,
{
    pub(crate) fn new(lock: MutexGuard<'a, P::CtrlState>) -> Self {
        AccGuardG { guard: lock }
    }
}

impl<P> Deref for AccGuardG<'_, P>
where
    P: CoreParam + CtrlStateParam,
    P::CtrlState: CtrlStateCore<P>,
{
    type Target = P::Acc;

    fn deref(&self) -> &Self::Target {
        self.guard.acc()
    }
}

/// Controls the collection and accumulation of thread-local values linked to it.
/// Such values, of type [`Param::Dat`], must be held in thread-locals of type [`HolderG`]`<P>`.
pub struct ControlG<P>
where
    P: CoreParam + CtrlStateParam,
{
    /// Keeps track of linked thread-locals and accumulated value.
    pub(crate) state: Arc<Mutex<P::CtrlState>>,
    /// Binary operation that combines data from thread-locals with accumulated value.
    #[allow(clippy::type_complexity)]
    pub(crate) op: Arc<dyn Fn(P::Dat, &mut P::Acc, &ThreadId) + Send + Sync>,
}

impl<P: CoreParam> ControlG<P>
where
    P: CoreParam + CtrlStateParam,
    P::CtrlState: CtrlStateCore<P>,
{
    /// Acquires a lock on [`ControlG`]'s internal Mutex.
    pub(crate) fn lock(&self) -> MutexGuard<'_, P::CtrlState> {
        self.state.lock().unwrap()
    }

    /// Returns a guard object that dereferences to `self`'s accumulated value. A lock held is during the guard's
    /// lifetime.
    pub fn acc(&self) -> AccGuardG<'_, P> {
        AccGuardG::new(self.lock())
    }

    /// Provides access to [`self`]'s accumulated value.
    pub fn with_acc<V>(&self, f: impl FnOnce(&P::Acc) -> V) -> V {
        let acc = self.acc();
        f(&acc)
    }

    /// Returns a clone of [`self`]'s accumulated value.
    pub fn clone_acc(&self) -> P::Acc
    where
        P::Acc: Clone,
    {
        self.acc().clone()
    }

    /// Returns [`self`]'s accumulated value, using a value of the same type to replace
    /// the existing accumulated value.
    pub fn take_acc(&self, replacement: P::Acc) -> P::Acc {
        let mut lock = self.lock();
        let acc = lock.acc_mut();
        replace(acc, replacement)
    }

    /// Used by [`HolderG`] to notify [`ControlG`] that the holder's data has been dropped.
    fn tl_data_dropped(&self, data: Option<P::Dat>, tid: &ThreadId) {
        let mut lock = self.lock();
        lock.tl_data_dropped(self.op.deref(), data, tid);
    }
}

impl<P> Clone for ControlG<P>
where
    P: CoreParam + CtrlStateParam,
    P::CtrlState: CtrlStateCore<P>,
{
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            op: self.op.clone(),
        }
    }
}

impl<P> Debug for ControlG<P>
where
    P: CoreParam + CtrlStateParam + Debug,
    P::CtrlState: CtrlStateCore<P> + Debug,
    P::Acc: Debug,
    P::Discr: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Control({:?})", self.state))
    }
}

#[doc(hidden)]
/// Abstraction of data wrappers used by [`HolderG`] specializations for different modules.
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

/// Holds thead-local data and a smart pointer to a [`ControlG`], enabling the linkage of the held data
/// with the control object.
pub struct HolderG<P>
where
    P: CoreParam + GDataParam + CtrlStateParam,
    P::Dat: 'static,
    P::GData: GuardedData<Option<P::Dat>> + 'static,
    P::CtrlState: CtrlStateCore<P>,
{
    pub(crate) data: P::GData,
    pub(crate) control: RefCell<Option<ControlG<P>>>,
    pub(crate) make_data: fn() -> P::Dat,
}

impl<P> HolderG<P>
where
    P: CoreParam + GDataParam + CtrlStateParam,
    P::GData: GuardedData<Option<P::Dat>> + 'static,
    P::CtrlState: CtrlStateCore<P>,
{
    /// Returns reference to `control` field.
    pub(crate) fn control(&self) -> Ref<'_, Option<ControlG<P>>> {
        self.control.borrow()
    }

    /// Returns data guard for the held data.
    pub(crate) fn data_guard(&self) -> <P::GData as GuardedData<Option<P::Dat>>>::Guard<'_> {
        self.data.guard()
    }

    /// Initializes the held data.
    pub(crate) fn init_data(&self) {
        let mut guard = self.data_guard();
        if guard.is_none() {
            *guard = Some((self.make_data)());
        }
    }

    /// Invokes `f` on the held data. Panics if the data is not initialized.
    pub(crate) fn with_data<V>(&self, f: impl FnOnce(&P::Dat) -> V) -> V {
        let guard = self.data_guard();
        // f(guard.unwrap()) // instead of 2 lines below
        let data: Option<&P::Dat> = guard.as_ref();
        f(data.unwrap())
    }

    /// Invokes `f` mutably on the held data. Panics if the data is not initialized.
    pub(crate) fn with_data_mut<V>(&self, f: impl FnOnce(&mut P::Dat) -> V) -> V {
        let mut guard = self.data_guard();
        // f(guard.unwrap_mut()) // instead of 2 lines below
        let data: Option<&mut P::Dat> = guard.as_mut();
        f(data.unwrap())
    }

    /// Used by [`Drop`] trait impl.
    fn drop_data(&self) {
        let mut data_guard = self.data_guard();
        let data = data_guard.take();
        let control = self.control();
        if control.is_none() {
            return;
        }
        let control = Ref::map(control, |x| x.as_ref().unwrap());
        control.tl_data_dropped(data, &thread::current().id());
    }
}

impl<P> Debug for HolderG<P>
where
    P: CoreParam + GDataParam + CtrlStateParam + Debug,
    P::GData: GuardedData<Option<P::Dat>> + Debug,
    P::CtrlState: CtrlStateCore<P>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Holder{{data: {:?}}}", &self.data))
    }
}

impl<P> Drop for HolderG<P>
where
    P: CoreParam + GDataParam + CtrlStateParam,
    P::GData: GuardedData<Option<P::Dat>> + 'static,
    P::CtrlState: CtrlStateCore<P>,
{
    fn drop(&mut self) {
        self.drop_data()
    }
}

/// Provides access to [`HolderG`] specializations of different framework modules
/// ([`crate::joined`], [`crate::simple_joined`], [`crate::probed`]).
pub trait HolderLocalKey<P>
where
    P: CoreParam + CtrlStateParam,
{
    /// Establishes link with control.
    fn init_control(&'static self, control: &ControlG<P>);

    /// Initializes held data.
    fn init_data(&'static self);

    /// Ensures [`HolderG`] instance is properly initialized (both data and control link) by initializing it if not.
    fn ensure_initialized(&'static self, control: &ControlG<P>);

    /// Invokes `f` on the held data. Panics if the data is not initialized.
    fn with_data<V>(&'static self, f: impl FnOnce(&P::Dat) -> V) -> V;

    /// Invokes `f` mutably on the held data. Panics if the data is not initialized.
    fn with_data_mut<V>(&'static self, f: impl FnOnce(&mut P::Dat) -> V) -> V;
}

//=================
// Common items to support specific discriminants.

#[doc(hidden)]
/// Type used to hold the thread map used by the [`ControlG`] specializations for module [`crate::probed`].
/// Used also for module [`crate::joined_bad`] which shows a previous incorrect implementation of module
/// [`crate::joined`].
/// Also used to partially discriminate common [`ControlG`] functionality used by those modules.
#[derive(Debug)]
pub struct TmapD<P>
where
    P: CoreParam + NodeParam,
{
    pub(crate) tmap: HashMap<ThreadId, P::Node>,
}

impl<P> CoreParam for TmapD<P>
where
    P: CoreParam + NodeParam,
{
    type Acc = P::Acc;
    type Dat = P::Dat;
    type Discr = Self;
}

impl<P> GDataParam for TmapD<P>
where
    P: CoreParam + NodeParam + GDataParam,
{
    type GData = P::GData;
}

impl<P> ControlStateG<TmapD<P>>
where
    P: CoreParam + NodeParam,
{
    pub fn new(acc_base: P::Acc) -> Self {
        Self {
            acc: acc_base,
            d0: TmapD {
                tmap: HashMap::new(),
            },
        }
    }
}

impl<P> CtrlStateCore<P> for ControlStateG<TmapD<P>>
where
    P: CoreParam + NodeParam + CtrlStateParam,
{
    fn acc(&self) -> &<P as CoreParam>::Acc {
        ControlStateG::acc(&self)
    }

    fn acc_mut(&mut self) -> &mut <P as CoreParam>::Acc {
        ControlStateG::acc_mut(self)
    }

    /// Indirectly used by [`HolderG`] to notify [`ControlG`] that the holder's data has been dropped.
    ///
    /// The `data` argument is not strictly necessary to support the implementation as the data can be
    /// recovered from the `tmap`. However, the `data` argument makes
    /// the generic implementation simpler and uniform across all modules. Without the `data`
    /// argument, the implementations of this method for [`crate::joined`] and [`crate::probed`] would be
    /// different from each other and a bit more complex.
    fn tl_data_dropped(
        &mut self,
        op: &(dyn Fn(P::Dat, &mut P::Acc, &ThreadId) + Send + Sync),
        data: Option<P::Dat>,
        tid: &ThreadId,
    ) {
        self.d0.tmap.remove(tid);
        if let Some(data) = data {
            let acc = self.acc_mut();
            op(data, acc, tid);
        }
    }
}

impl<P> CtrlStateWithNode<P> for ControlStateG<TmapD<P>>
where
    P: CoreParam + NodeParam + CtrlStateParam,
{
    fn register_node(&mut self, node: <P as NodeParam>::Node, tid: &ThreadId) {
        self.d0.tmap.insert(tid.clone(), node);
    }
}

impl<P> ControlG<P>
where
    P: CoreParam + NodeParam + CtrlStateParam,
    P::CtrlState: CtrlStateWithNode<P>,
{
    /// Called by [`HolderG`] when a thread-local variable starts being used.
    fn register_node(&self, node: P::Node, tid: &ThreadId) {
        let mut lock = self.lock();
        lock.register_node(node, tid)
    }
}

impl<P> HolderG<P>
where
    P: CoreParam + GDataParam + NodeParam + CtrlStateParam,
    P::Dat: 'static,
    P::GData: GuardedData<Option<P::Dat>> + 'static,
    P::CtrlState: CtrlStateWithNode<P>,
{
    /// Function to be used as a field in [`HolderG`].
    pub(crate) fn init_control(&self, control: &ControlG<P>, node: P::Node) {
        let mut ctrl_ref = self.control.borrow_mut();
        *ctrl_ref = Some(control.clone());
        control.register_node(node, &thread::current().id())
    }
}

#[doc(hidden)]
/// Type used to hold the node value used by the [`ControlG`] specializations for module [`crate::joined`] and
/// [`crate::simple_joined`].
/// Also used to partially discriminate common [`ControlG`] functionality used by those modules.
#[derive(Debug)]
pub struct NoTmapD<P> {
    pub(crate) d1: P,
}

impl<P: CoreParam> CoreParam for NoTmapD<P> {
    type Acc = P::Acc;
    type Dat = P::Dat;
    type Discr = Self;
}

impl<P: GDataParam> GDataParam for NoTmapD<P> {
    type GData = P::GData;
}

impl<P> CtrlStateCore<P> for ControlStateG<NoTmapD<P>>
where
    P: CoreParam + NodeParam + CtrlStateParam,
{
    fn acc(&self) -> &<P as CoreParam>::Acc {
        ControlStateG::acc(&self)
    }

    fn acc_mut(&mut self) -> &mut <P as CoreParam>::Acc {
        ControlStateG::acc_mut(self)
    }

    /// Indirectly used by [`HolderG`] to notify [`ControlG`] that the holder's data has been dropped.
    ///
    /// The `data` argument is not strictly necessary to support the implementation as the data can be
    /// recovered from the `tmap`. However, the `data` argument makes
    /// the generic implementation simpler and uniform across all modules. Without the `data`
    /// argument, the implementations of this method for [`crate::joined`] and [`crate::probed`] would be
    /// different from each other and a bit more complex.
    fn tl_data_dropped(
        &mut self,
        op: &(dyn Fn(P::Dat, &mut P::Acc, &ThreadId) + Send + Sync),
        data: Option<P::Dat>,
        tid: &ThreadId,
    ) {
        if let Some(data) = data {
            let acc = self.acc_mut();
            op(data, acc, tid);
        }
    }
}
