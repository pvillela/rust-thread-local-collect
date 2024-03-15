//! This module contains common traits and structs that are used by the other modules in this libray,
//! except for the [`crate::channeled`] mofule.

use std::{
    borrow::BorrowMut,
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    fmt::Debug,
    mem::replace,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, ThreadId},
};
use thiserror::Error;

//=================
// Errora

pub(crate) const POISONED_CONTROL_MUTEX: &str = "poisoned control mutex";
pub(crate) const POISONED_GUARDED_DATA_MUTEX: &str = "poisoned guarded data mutex";

/// Attempt to access `Holder` before it has been linked with `Control`.
#[derive(Error, Debug)]
#[error("attempt to access uninitialized Holder")]
pub struct HolderNotLinkedError;

//=================
// Param traits

/// Encapsulates the core types used by [`ControlG`], [`HolderG`], and their specializations.
pub trait CoreParam {
    /// Type of data held in thread-local variable.
    type Dat;
    /// Type of accumulated value.
    type Acc;
}

#[doc(hidden)]
/// Abstracts the type of the state of a [`ControlG`].
pub trait CtrlStateParam {
    type CtrlState;
}

#[doc(hidden)]
/// Abstracts the type of a sub-state of a [`CtrlStateG`].
pub trait SubStateParam {
    type SubState: New<Self::SubState, Arg = ()>;
}

#[doc(hidden)]
/// Abstracts the type of node used by structs and functions that depend on a node type.
/// The node type represents thread-local variable address or reference information that is held
/// in the state of a [`ControlG`].
pub trait NodeParam {
    type Node;
}

#[doc(hidden)]
/// Abstracts the type of [`GuardedData`] used by [`HolderG`].
pub trait GDataParam {
    type GData;
}

//=================
// Hidden non-param traits

#[doc(hidden)]
/// Used to tag an instantiation of [`CtrlStateG`] and enable it to inherit default [`CtrlStateG`] functionality.
pub trait UseCtrlStateGDefault {}

#[doc(hidden)]
/// Abstracts a type's ability to construct itself.
pub trait New<S> {
    type Arg;

    #[allow(clippy::new_ret_no_self)]
    fn new(arg: Self::Arg) -> S;
}

#[doc(hidden)]
/// Abstracts types that return the accumulator type.
pub trait WithAcc: New<Self, Arg = Self::Acc>
where
    Self: Sized,
{
    type Acc;

    /// Returns reference to accumulated value.
    fn acc(&self) -> &Self::Acc;

    /// Returns mutable reference to accumulated value.
    fn acc_mut(&mut self) -> &mut Self::Acc;
}

#[doc(hidden)]
/// Abstracts the core features of the state of a [`ControlG`].
pub trait CtrlStateCore<P>: WithAcc<Acc = P::Acc>
where
    Self: Sized,
    P: CoreParam,
{
    /// Invoked when thread-local [`HolderG`] is dropped to notify the control state and accumulate
    /// the thread-local value.
    ///
    // The `data` argument is not strictly necessary to support the implementation for state that uses
    // a node type as the data can be recovered from the corresponding node. However, the `data` argument makes
    // the generic implementation simpler and uniform across all modules. Without the `data`
    // argument, the implementations of this method for [`crate::joined`] and [`crate::probed`] would be
    // different from each other and a bit more complex.
    fn tl_data_dropped(
        &mut self,
        op: &(dyn Fn(P::Dat, &mut P::Acc, ThreadId) + Send + Sync),
        data: P::Dat,
        tid: ThreadId,
    );
}

#[doc(hidden)]
/// Abstracts additional features of the state of a [`ControlG`] that uses a [`NodeParam`].
pub trait CtrlStateWithNode<P>: CtrlStateCore<P>
where
    P: CoreParam + NodeParam,
{
    /// Registers a node with the control state.
    fn register_node(&mut self, node: P::Node, tid: ThreadId);
}

//=================
// Core structs and impls

/// Guard that dereferences to the accumulator type. A lock is held during the guard's lifetime.
pub struct AccGuardG<'a, S> {
    guard: MutexGuard<'a, S>,
}

impl<'a, S> AccGuardG<'a, S> {
    pub(crate) fn new(lock: MutexGuard<'a, S>) -> Self {
        Self { guard: lock }
    }
}

impl<S> Deref for AccGuardG<'_, S>
where
    S: WithAcc,
{
    type Target = S::Acc;

    fn deref(&self) -> &Self::Target {
        self.guard.acc()
    }
}

#[doc(hidden)]
/// Data structure that can be used as the state of a [`ControlG`].
#[derive(Debug)]
pub struct CtrlStateG<P>
where
    P: CoreParam + SubStateParam,
{
    pub(crate) acc: P::Acc,
    pub(crate) s: P::SubState,
}

impl<P> CtrlStateG<P>
where
    P: CoreParam + SubStateParam,
{
    fn acc(&self) -> &P::Acc {
        &self.acc
    }

    pub(crate) fn acc_mut(&mut self) -> &mut P::Acc {
        &mut self.acc
    }
}

impl<P> New<Self> for CtrlStateG<P>
where
    P: CoreParam + SubStateParam,
{
    type Arg = P::Acc;

    fn new(acc_base: P::Acc) -> Self {
        Self {
            acc: acc_base,
            s: P::SubState::new(()),
        }
    }
}

impl<P> WithAcc for CtrlStateG<P>
where
    P: CoreParam + SubStateParam + UseCtrlStateGDefault,
{
    type Acc = P::Acc;

    fn acc(&self) -> &P::Acc {
        CtrlStateG::acc(self)
    }

    fn acc_mut(&mut self) -> &mut P::Acc {
        CtrlStateG::acc_mut(self)
    }
}

impl<P> CtrlStateCore<P> for CtrlStateG<P>
where
    P: CoreParam + SubStateParam + UseCtrlStateGDefault,
{
    /// Indirectly used by [`HolderG`] to notify [`ControlG`] that the holder's data has been dropped.
    ///
    /// The `data` argument is not strictly necessary to support the implementation for state that uses
    /// a node type as the data can be recovered from the corresponding node. However, the `data` argument makes
    /// the generic implementation simpler and uniform across all modules. Without the `data`
    /// argument, the implementations of this method for [`crate::joined`] and [`crate::probed`] would be
    /// different from each other and a bit more complex.
    fn tl_data_dropped(
        &mut self,
        op: &(dyn Fn(P::Dat, &mut P::Acc, ThreadId) + Send + Sync),
        data: P::Dat,
        tid: ThreadId,
    ) {
        let acc = self.acc_mut();
        op(data, acc, tid);
    }
}

/// Controls the collection and accumulation of thread-local values linked to this object.
/// Such values, of type [`CoreParam::Dat`], must be held in thread-locals of type [`HolderG<P>`].
pub struct ControlG<P>
where
    P: CoreParam + CtrlStateParam,
{
    /// Keeps track of linked thread-locals and accumulated value.
    pub(crate) state: Arc<Mutex<P::CtrlState>>,
    /// Binary operation that combines data from thread-locals with accumulated value.
    #[allow(clippy::type_complexity)]
    pub(crate) op: Arc<dyn Fn(P::Dat, &mut P::Acc, ThreadId) + Send + Sync>,
}

impl<P> ControlG<P>
where
    P: CoreParam + CtrlStateParam,
    P::CtrlState: CtrlStateCore<P>,
{
    /// Instantiates a *control* object.
    pub fn new(
        acc_base: P::Acc,
        op: impl Fn(P::Dat, &mut P::Acc, ThreadId) + 'static + Send + Sync,
    ) -> Self {
        let state = P::CtrlState::new(acc_base);
        Self {
            state: Arc::new(Mutex::new(state)),
            op: Arc::new(op),
        }
    }

    /// Acquires a lock on [`ControlG`]'s internal Mutex.
    /// Panics if `self`'s mutex is poisoned.
    pub(crate) fn lock(&self) -> MutexGuard<'_, P::CtrlState> {
        self.state.lock().expect(POISONED_CONTROL_MUTEX)
    }

    /// Returns a guard object that dereferences to `self`'s accumulated value. A lock is held during the guard's
    /// lifetime.
    /// Panics if `self`'s mutex is poisoned.
    pub fn acc(&self) -> AccGuardG<'_, P::CtrlState> {
        AccGuardG::new(self.lock())
    }

    /// Provides access to `self`'s accumulated value.
    /// Panics if `self`'s mutex is poisoned.
    pub fn with_acc<V>(&self, f: impl FnOnce(&P::Acc) -> V) -> V {
        let acc = self.acc();
        f(&acc)
    }

    /// Returns a clone of `self`'s accumulated value.
    /// Panics if `self`'s mutex is poisoned.
    pub fn clone_acc(&self) -> P::Acc
    where
        P::Acc: Clone,
    {
        self.acc().clone()
    }

    /// Returns `self`'s accumulated value, using a value of the same type to replace
    /// the existing accumulated value.
    /// Panics if `self`'s mutex is poisoned.
    pub fn take_acc(&self, replacement: P::Acc) -> P::Acc {
        let mut lock = self.lock();
        let acc = lock.acc_mut();
        replace(acc, replacement)
    }

    /// Used by [`HolderG`] to notify [`ControlG`] that the holder's data has been dropped.
    /// Panics if `self`'s mutex is poisoned.
    fn tl_data_dropped(&self, data: P::Dat, tid: ThreadId) {
        let mut lock = self.lock();
        lock.tl_data_dropped(self.op.deref(), data, tid);
    }
}

impl<P> ControlG<P>
where
    P: CoreParam + NodeParam + CtrlStateParam,
    P::CtrlState: CtrlStateWithNode<P>,
{
    /// Called by [`HolderG`] when a thread-local variable starts being used.
    /// Panics if `self`'s mutex is poisoned.
    fn register_node(&self, node: P::Node, tid: ThreadId) {
        let mut lock = self.lock();
        lock.register_node(node, tid)
    }
}

impl<P> Clone for ControlG<P>
where
    P: CoreParam + CtrlStateParam,
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
    P: CoreParam + CtrlStateParam,
    P::CtrlState: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Control({:?})", self.state))
    }
}

#[doc(hidden)]
/// Abstraction of data wrappers used by [`HolderG`] specializations for different modules.
pub trait GuardedData<S: 'static>: New<Self>
where
    Self: Sized,
{
    type Guard<'a>: DerefMut<Target = S> + 'a
    where
        Self: 'a;

    fn guard(&self) -> Self::Guard<'_>;
}

impl<T> New<Self> for RefCell<T> {
    type Arg = T;

    fn new(t: Self::Arg) -> Self {
        RefCell::new(t)
    }
}

impl<T: 'static> GuardedData<T> for RefCell<T> {
    type Guard<'a> = RefMut<'a, T>;

    fn guard(&self) -> Self::Guard<'_> {
        self.borrow_mut()
    }
}

impl<T> New<Self> for Arc<Mutex<T>> {
    type Arg = T;

    fn new(t: Self::Arg) -> Self {
        Arc::new(Mutex::new(t))
    }
}

impl<T: 'static> GuardedData<T> for Arc<Mutex<T>> {
    type Guard<'a> = MutexGuard<'a, T>;

    fn guard(&self) -> Self::Guard<'_> {
        self.lock().expect(POISONED_GUARDED_DATA_MUTEX)
    }
}

/// Holds thread-local data and a smart pointer to a [`ControlG`], enabling the linkage of the held data
/// with the control object.
pub struct HolderG<P>
where
    P: CoreParam + GDataParam + CtrlStateParam,
    P::Dat: 'static,
    P::GData: GuardedData<P::Dat, Arg = P::Dat> + 'static,
    P::CtrlState: CtrlStateCore<P>,
{
    pub(crate) data: P::GData,
    pub(crate) control: RefCell<Option<ControlG<P>>>,
    pub(crate) make_data: fn() -> P::Dat,
}

impl<P> HolderG<P>
where
    P: CoreParam + GDataParam + CtrlStateParam,
    P::GData: GuardedData<P::Dat, Arg = P::Dat> + 'static,
    P::CtrlState: CtrlStateCore<P>,
{
    /// Instantiates a holder object. The `make_data` function produces the value used to initialize the
    /// held data.
    pub fn new(make_data: fn() -> P::Dat) -> Self {
        Self {
            data: P::GData::new(make_data()),
            control: RefCell::new(None),
            make_data,
        }
    }

    /// Returns reference to `control` field.
    pub(crate) fn control(&self) -> Ref<'_, Option<ControlG<P>>> {
        self.control.borrow()
    }

    /// Returns data guard for the held data.
    pub(crate) fn data_guard(&self) -> <P::GData as GuardedData<P::Dat>>::Guard<'_> {
        self.data.guard()
    }

    /// Invokes `f` on the held data.
    /// Returns an error if [`HolderG`] not linked with [`ControlG`].
    pub(crate) fn with_data<V>(
        &self,
        f: impl FnOnce(&P::Dat) -> V,
    ) -> Result<V, HolderNotLinkedError> {
        if self.control().deref().is_none() {
            return Err(HolderNotLinkedError);
        }
        let guard = self.data_guard();
        let res = f(&guard);
        Ok(res)
    }

    /// Invokes `f` mutably on the held data.
    /// Returns an error if [`HolderG`] not linked with [`ControlG`].
    pub(crate) fn with_data_mut<V>(
        &self,
        f: impl FnOnce(&mut P::Dat) -> V,
    ) -> Result<V, HolderNotLinkedError> {
        if self.control().deref().is_none() {
            return Err(HolderNotLinkedError);
        }
        let mut guard = self.data_guard();
        let res = f(&mut guard);
        Ok(res)
    }

    /// Used by [`Drop`] trait impl.
    fn drop_data(&self) {
        match self.control.borrow().deref() {
            None => (),
            Some(control) => {
                let mut data_guard = self.data_guard();
                let data = replace(data_guard.borrow_mut().deref_mut(), (self.make_data)());
                control.tl_data_dropped(data, thread::current().id());
            }
        }
    }
}

impl<P> HolderG<P>
where
    P: CoreParam + GDataParam + CtrlStateParam,
    P::Dat: 'static,
    P::GData: GuardedData<P::Dat, Arg = P::Dat> + 'static,
    P::CtrlState: CtrlStateCore<P>,
{
    /// Initializes the `control` field in [`HolderG`].
    fn link(&self, control: &ControlG<P>) {
        let mut ctrl_ref = self.control.borrow_mut();
        *ctrl_ref = Some(control.clone());
    }

    /// Ensures `self` is linkded with `control`.
    pub(crate) fn ensure_linked(&self, control: &ControlG<P>) {
        if self.control().as_ref().is_none() {
            self.link(control);
        }
    }
}

impl<P> HolderG<P>
where
    P: CoreParam + GDataParam + NodeParam + CtrlStateParam,
    P::Dat: 'static,
    P::GData: GuardedData<P::Dat, Arg = P::Dat> + 'static,
    P::CtrlState: CtrlStateWithNode<P>,
{
    /// Initializes the `control` field in [`HolderG`] when a node type is used.
    fn link_node(&self, control: &ControlG<P>, node: P::Node) {
        let mut ctrl_ref = self.control.borrow_mut();
        *ctrl_ref = Some(control.clone());
        control.register_node(node, thread::current().id())
    }

    /// Ensures `self` is linked to `control` when a node type is used.
    pub(crate) fn ensure_linked_node(&self, control: &ControlG<P>, node: P::Node) {
        if self.control().as_ref().is_none() {
            self.link_node(control, node);
        }
    }
}

impl<P> Debug for HolderG<P>
where
    P: CoreParam + GDataParam + CtrlStateParam + Debug,
    P::GData: GuardedData<P::Dat, Arg = P::Dat> + Debug + 'static,
    P::CtrlState: CtrlStateCore<P>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Holder{{data: {:?}}}", &self.data))
    }
}

impl<P> Drop for HolderG<P>
where
    P: CoreParam + GDataParam + CtrlStateParam,
    P::GData: GuardedData<P::Dat, Arg = P::Dat> + 'static,
    P::CtrlState: CtrlStateCore<P>,
{
    fn drop(&mut self) {
        self.drop_data()
    }
}

//=================
// Visible trait

/// Provides access to [`HolderG`] specializations of different framework modules
/// ([`crate::joined`], [`crate::simple_joined`], [`crate::probed`]).
pub trait HolderLocalKey<P>
where
    P: CoreParam + CtrlStateParam,
{
    /// Ensures [`HolderG`] instance is linked with `control``.
    fn ensure_linked(&'static self, control: &ControlG<P>);

    /// Invokes `f` on the held data.
    /// Returns an error if [`HolderG`] not linked with [`ControlG`].
    fn with_data<V>(&'static self, f: impl FnOnce(&P::Dat) -> V)
        -> Result<V, HolderNotLinkedError>;

    /// Invokes `f` mutably on the held data.
    /// Returns an error if [`HolderG`] not linked with [`ControlG`].
    fn with_data_mut<V>(
        &'static self,
        f: impl FnOnce(&mut P::Dat) -> V,
    ) -> Result<V, HolderNotLinkedError>;
}

//=================
// Control sub-state struct with a thread map.

/// Type used by the specialization of [`ControlG`] for module [`crate::probed`].
//  Holds the thread map used by the [`ControlG`] specializations for module [`crate::probed`].
//  Used also for module [`crate::joined_old`] which shows a previous more complex implementation of module
//  [`crate::joined`].
//  Also used to partially discriminate common [`ControlG`] functionality used by those modules.
#[derive(Debug)]
pub struct TmapD<P>
where
    P: NodeParam,
{
    pub(crate) tmap: HashMap<ThreadId, P::Node>,
}

impl<P> CoreParam for TmapD<P>
where
    P: CoreParam + NodeParam,
{
    type Acc = P::Acc;
    type Dat = P::Dat;
}

impl<P> NodeParam for TmapD<P>
where
    P: CoreParam + NodeParam,
{
    type Node = P::Node;
}

impl<P> CtrlStateParam for TmapD<P>
where
    P: CoreParam + NodeParam,
{
    type CtrlState = CtrlStateG<Self>;
}

impl<P> SubStateParam for TmapD<P>
where
    P: CoreParam + NodeParam,
{
    type SubState = Self;
}

impl<P> GDataParam for TmapD<P>
where
    P: CoreParam + NodeParam + GDataParam,
{
    type GData = P::GData;
}

impl<P> New<Self> for TmapD<P>
where
    P: NodeParam,
{
    type Arg = ();

    fn new(_: ()) -> Self {
        Self {
            tmap: HashMap::new(),
        }
    }
}

impl<P> WithAcc for CtrlStateG<TmapD<P>>
where
    P: CoreParam + NodeParam,
{
    type Acc = P::Acc;

    fn acc(&self) -> &P::Acc {
        CtrlStateG::acc(self)
    }

    fn acc_mut(&mut self) -> &mut P::Acc {
        CtrlStateG::acc_mut(self)
    }
}

impl<P> CtrlStateCore<TmapD<P>> for CtrlStateG<TmapD<P>>
where
    P: CoreParam + NodeParam,
{
    fn tl_data_dropped(
        &mut self,
        op: &(dyn Fn(P::Dat, &mut P::Acc, ThreadId) + Send + Sync),
        data: P::Dat,
        tid: ThreadId,
    ) {
        self.s.tmap.remove(&tid);
        let acc = self.acc_mut();
        op(data, acc, tid);
    }
}

impl<P> CtrlStateWithNode<TmapD<P>> for CtrlStateG<TmapD<P>>
where
    P: CoreParam + NodeParam,
{
    fn register_node(&mut self, node: <P as NodeParam>::Node, tid: ThreadId) {
        self.s.tmap.insert(tid, node);
    }
}
