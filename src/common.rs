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
    #[doc(hidden)]
    /// Type of node in [`TmapD`] and [`NodeD`].
    type Node;
}

pub trait GDataParam {
    #[doc(hidden)]
    /// Guarded data type used by [`HolderG`].
    type GData;
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
pub struct AccGuardG<'a, P: CoreParam> {
    guard: MutexGuard<'a, ControlStateG<P>>,
}

impl<'a, P: CoreParam> AccGuardG<'a, P> {
    pub(crate) fn new(lock: MutexGuard<'a, ControlStateG<P>>) -> Self {
        AccGuardG { guard: lock }
    }
}

impl<P: CoreParam> Deref for AccGuardG<'_, P> {
    type Target = P::Acc;

    fn deref(&self) -> &Self::Target {
        self.guard.acc()
    }
}

/// Controls the collection and accumulation of thread-local values linked to it.
/// Such values, of type [`Param::Dat`], must be held in thread-locals of type [`HolderG`]`<P>`.
pub struct ControlG<P: CoreParam> {
    /// Keeps track of linked thread-locals and accumulated value.
    pub(crate) state: Arc<Mutex<ControlStateG<P>>>,
    /// Binary operation that combines data from thread-locals with accumulated value.
    #[allow(clippy::type_complexity)]
    pub(crate) op: Arc<dyn Fn(P::Dat, &mut P::Acc, &ThreadId) + Send + Sync>,
}

impl<P: CoreParam> ControlG<P> {
    /// Acquires a lock on [`ControlG`]'s internal Mutex.
    pub(crate) fn lock(&self) -> MutexGuard<'_, ControlStateG<P>> {
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
}

impl<P: CoreParam> Clone for ControlG<P> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            op: self.op.clone(),
        }
    }
}

impl<P> Debug for ControlG<P>
where
    P: CoreParam + Debug,
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
    P: CoreParam + GDataParam,
    P::Dat: 'static,
    P::GData: GuardedData<Option<P::Dat>> + 'static,
{
    pub(crate) data: P::GData,
    pub(crate) control: RefCell<Option<ControlG<P>>>,
    pub(crate) make_data: fn() -> P::Dat,
    pub(crate) init_control_fn: fn(this: &Self, control: &ControlG<P>, node: P::Node),
    pub(crate) drop_data_fn: fn(&Self),
}

impl<P> HolderG<P>
where
    P: CoreParam + GDataParam,
    P::GData: GuardedData<Option<P::Dat>> + 'static,
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

    /// Ensures `self` is initialized, both the held data and the `control` smart pointer.
    pub(crate) fn ensure_initialized(&self, control: &ControlG<P>, node: P::Node) {
        if self.control().as_ref().is_none() {
            self.init_control(control, node);
        }

        if self.data_guard().is_none() {
            self.init_data();
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
        (self.drop_data_fn)(self)
    }
}

impl<P> HolderG<P>
where
    P: CoreParam + GDataParam,
    P::GData: GuardedData<Option<P::Dat>> + 'static,
{
    /// Initializes the `control` field.
    pub(crate) fn init_control(&self, control: &ControlG<P>, node: P::Node) {
        (self.init_control_fn)(self, control, node)
    }
}

impl<P> Debug for HolderG<P>
where
    P: CoreParam + GDataParam + Debug,
    P::GData: GuardedData<Option<P::Dat>> + Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Holder{{data: {:?}}}", &self.data))
    }
}

impl<P> Drop for HolderG<P>
where
    P: CoreParam + GDataParam,
    P::GData: GuardedData<Option<P::Dat>> + 'static,
{
    fn drop(&mut self) {
        self.drop_data()
    }
}

/// Provides access to [`HolderG`] specializations of different framework modules
/// ([`crate::joined`], [`crate::simple_joined`], [`crate::probed`]).
pub trait HolderLocalKey<P: CoreParam> {
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
// Common items to support modules `probed` and `joined_bad`.

#[doc(hidden)]
/// Type used to hold the thread map used by the [`ControlG`] specializations for module [`crate::probed`].
/// Used also for module [`crate::joined_bad`] which shows a previous incorrect implementation of module
/// [`crate::joined`].
/// Also used to partially discriminate common [`ControlG`] functionality used by those modules.
#[derive(Debug)]
pub struct TmapD<P>
where
    P: CoreParam,
{
    pub(crate) tmap: HashMap<ThreadId, P::Node>,
}

impl<P: CoreParam> CoreParam for TmapD<P> {
    type Acc = P::Acc;
    type Dat = P::Dat;
    type Discr = Self;
    type Node = P::Node;
}

impl<P> GDataParam for TmapD<P>
where
    P: CoreParam + GDataParam,
{
    type GData = P::GData;
}

impl<P: CoreParam> ControlStateG<TmapD<P>> {
    pub fn new(acc_base: P::Acc) -> Self {
        Self {
            acc: acc_base,
            d0: TmapD {
                tmap: HashMap::new(),
            },
        }
    }

    /// Indirectly called by [`HolderG`] when a thread-local variable starts being used.
    fn register_node(&mut self, node: P::Node, tid: &ThreadId) {
        self.d0.tmap.insert(tid.clone(), node);
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

impl<P: CoreParam> ControlG<TmapD<P>> {
    /// Called by [`HolderG`] when a thread-local variable starts being used.
    fn register_node(&self, node: P::Node, tid: &ThreadId) {
        let mut lock = self.lock();
        lock.register_node(node, tid)
    }

    /// Used by [`HolderG`] to notify [`ControlG`] that the holder's data has been dropped.
    fn tl_data_dropped(&self, data: Option<P::Dat>, tid: &ThreadId) {
        let mut lock = self.lock();
        lock.tl_data_dropped(self.op.deref(), data, tid);
    }
}

impl<P> HolderG<TmapD<P>>
where
    P: CoreParam + GDataParam,
    P::GData: GuardedData<Option<P::Dat>> + 'static,
{
    /// Function to be used as a field in [`HolderG`].
    pub(crate) fn init_control_fn(this: &Self, control: &ControlG<TmapD<P>>, node: P::Node) {
        let mut ctrl_ref = this.control.borrow_mut();
        *ctrl_ref = Some(control.clone());
        control.register_node(node, &thread::current().id())
    }

    /// Function to be used as a field in [`HolderG`].
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

//=================
// Common items that support modules `joined` and `simple_joined`

#[doc(hidden)]
/// Type used to hold the node value used by the [`ControlG`] specializations for module [`crate::joined`] and
/// [`crate::simple_joined`].
/// Also used to partially discriminate common [`ControlG`] functionality used by those modules.
#[derive(Debug)]
pub struct NodeD<P> {
    pub(crate) d1: P,
}

impl<P: CoreParam> CoreParam for NodeD<P> {
    type Acc = P::Acc;
    type Dat = P::Dat;
    type Discr = Self;
    type Node = P::Node;
}

impl<P: GDataParam> GDataParam for NodeD<P> {
    type GData = P::GData;
}

#[doc(hidden)]
pub trait RegisterNode<P: CoreParam> {
    fn register_node(&mut self, node: P::Node, tid: &ThreadId);
}

impl<P: CoreParam> ControlStateG<NodeD<P>> {
    /// Indirectly called by [`HolderG`] when a thread-local variable starts being used.
    fn register_node(&mut self, node: P::Node, tid: &ThreadId)
    where
        P: RegisterNode<P>,
    {
        self.d0.d1.register_node(node, tid);
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

impl<P: CoreParam> ControlG<NodeD<P>> {
    /// Called by [`HolderG`] when a thread-local variable starts being used.
    fn register_node(&self, node: P::Node, tid: &ThreadId)
    where
        P: RegisterNode<P>,
    {
        let mut lock = self.lock();
        lock.register_node(node, tid)
    }

    /// Used by [`HolderG`] to notify [`ControlG`] that the holder's data has been dropped.
    fn tl_data_dropped(&self, data: Option<P::Dat>, tid: &ThreadId) {
        let mut lock = self.lock();
        lock.tl_data_dropped(self.op.deref(), data, tid);
    }
}

impl<P> HolderG<NodeD<P>>
where
    P: CoreParam + GDataParam + RegisterNode<P>,
    P::GData: GuardedData<Option<P::Dat>> + 'static,
{
    /// Function to be used as a field in [`HolderG`].
    pub(crate) fn init_control_fn(this: &Self, control: &ControlG<NodeD<P>>, node: P::Node) {
        let mut ctrl_ref = this.control.borrow_mut();
        *ctrl_ref = Some(control.clone());
        control.register_node(node, &thread::current().id())
    }

    /// Function to be used as a field in [`HolderG`].
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
