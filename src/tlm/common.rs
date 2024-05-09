//! This module contains common traits and structs that are used by the other modules in this libray.
//! Module [`super::channeled`] implements the [core conepts](crate#core-concepts) directly and
//! makes minimal use of this module.

use std::{
    borrow::BorrowMut,
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    fmt::Debug,
    marker::PhantomData,
    mem::replace,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, LocalKey, ThreadId},
};

//=================
// Errora

pub(crate) const POISONED_CONTROL_MUTEX: &str = "poisoned control mutex";
pub(crate) const POISONED_GUARDED_DATA_MUTEX: &str = "poisoned guarded data mutex";

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
    // type StateArg;
}

#[doc(hidden)]
/// Abstracts the type of a sub-state of a [`CtrlStateG`].
pub trait SubStateParam {
    type SubState;
}

#[doc(hidden)]
/// Abstracts the type of node used by structs and functions that depend on a node type.
/// The node type represents thread-local variable address or reference information that is held
/// in the state of a [`ControlG`].
pub trait NodeParam {
    type Node;
}

// pub trait NodeFnParam {
//     type NodeFn;
// }

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
pub trait WithAcc
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
    // argument, the implementations of this method for [`super::joined`] and [`super::probed`] would be
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

pub trait WithNodeFn<P>
where
    P: NodeParam,
{
    type NodeFnArg;

    fn node_fn(arg: &Self::NodeFnArg) -> P::Node;
}

//=================
// Core structs and impls

/// Guard that dereferences to the accumulator type. A lock is held during the guard's lifetime.
struct AccGuardG<'a, S> {
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
    // pub(crate) state_arg: P::StateArg,
}

impl<P> CtrlStateG<P>
where
    P: CoreParam + SubStateParam,
{
    pub(crate) fn acc_priv(&self) -> &P::Acc {
        &self.acc
    }

    pub(crate) fn acc_mut_priv(&mut self) -> &mut P::Acc {
        &mut self.acc
    }
}

impl<P> New<Self> for CtrlStateG<P>
where
    P: CoreParam + SubStateParam,
    P::SubState: New<P::SubState, Arg = ()>,
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
    P: CoreParam + SubStateParam,
{
    type Acc = P::Acc;

    fn acc(&self) -> &P::Acc {
        CtrlStateG::acc_priv(self)
    }

    fn acc_mut(&mut self) -> &mut P::Acc {
        CtrlStateG::acc_mut_priv(self)
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
    /// argument, the implementations of this method for [`super::joined`] and [`super::probed`] would be
    /// different from each other and a bit more complex.
    fn tl_data_dropped(
        &mut self,
        op: &(dyn Fn(P::Dat, &mut P::Acc, ThreadId) + Send + Sync),
        data: P::Dat,
        tid: ThreadId,
    ) {
        let acc = self.acc_mut_priv();
        op(data, acc, tid);
    }
}

pub trait HldrParam {
    type Hldr;
}

pub trait Hldr {}

/// Controls the collection and accumulation of thread-local values linked to this object.
/// Such values, of type [`CoreParam::Dat`], must be held in thread-locals of type [`HolderG<P>`].
pub struct ControlG<P, D>
where
    P: CoreParam + CtrlStateParam + HldrParam,
    P::Hldr: Hldr,

    P: 'static,
{
    /// Reference to thread-local
    pub(crate) tl: &'static LocalKey<P::Hldr>,
    /// Keeps track of linked thread-locals and accumulated value.
    pub(crate) state: Arc<Mutex<P::CtrlState>>,
    /// Binary operation that combines data from thread-locals with accumulated value.
    #[allow(clippy::type_complexity)]
    pub(crate) op: Arc<dyn Fn(P::Dat, &mut P::Acc, ThreadId) + Send + Sync>,
    _d: PhantomData<D>,
}

impl<P, D> ControlG<P, D>
where
    P: CoreParam + CtrlStateParam + HldrParam,
    P::Hldr: Hldr,

    P::CtrlState: CtrlStateCore<P> + New<P::CtrlState, Arg = P::Acc>,
{
    /// Instantiates a *control* object.
    pub fn new(
        tl: &'static LocalKey<P::Hldr>,
        acc_base: P::Acc,
        op: impl Fn(P::Dat, &mut P::Acc, ThreadId) + 'static + Send + Sync,
    ) -> Self {
        let state = P::CtrlState::new(acc_base);
        Self {
            tl,
            state: Arc::new(Mutex::new(state)),
            op: Arc::new(op),
            _d: PhantomData,
        }
    }
}

impl<P, D> ControlG<P, D>
where
    P: CoreParam + CtrlStateParam + HldrParam,
    P::Hldr: Hldr,

    P::CtrlState: CtrlStateCore<P>,
{
    /// Acquires a lock on [`ControlG`]'s internal Mutex.
    /// Panics if `self`'s mutex is poisoned.
    pub(crate) fn lock(&self) -> MutexGuard<'_, P::CtrlState> {
        self.state.lock().expect(POISONED_CONTROL_MUTEX)
    }

    /// Returns a guard object that dereferences to `self`'s accumulated value. A lock is held during the guard's
    /// lifetime.
    /// Panics if `self`'s mutex is poisoned.
    pub fn acc(&self) -> impl Deref<Target = P::Acc> + '_ {
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

pub struct NoNode;

impl<P> ControlG<P, NoNode>
where
    P: CoreParam + CtrlStateParam + HldrParam<Hldr = HolderG<P, NoNode>>,

    P: CtrlStateParam + GDataParam,
    P::CtrlState: CtrlStateCore<P>,
    P::GData: GuardedData<P::Dat, Arg = P::Dat>,
{
    /// Invokes `f` on the held data.
    /// Returns an error if [`HolderG`] not linked with [`ControlG`].
    pub fn with_data<V>(&self, f: impl FnOnce(&P::Dat) -> V) -> V {
        self.tl.with(|h| {
            h.ensure_linked(self);
            h.with_data(f)
        })
    }

    /// Invokes `f` mutably on the held data.
    /// Returns an error if [`HolderG`] not linked with [`ControlG`].
    pub fn with_data_mut<V>(&self, f: impl FnOnce(&mut P::Dat) -> V) -> V {
        self.tl.with(|h| {
            h.ensure_linked(self);
            h.with_data_mut(f)
        })
    }
}

pub struct WithNode;

impl<P> ControlG<P, WithNode>
where
    P: CoreParam + CtrlStateParam + HldrParam,
    P::Hldr: Hldr,

    P: NodeParam,
    P::CtrlState: CtrlStateWithNode<P>,
{
    /// Called by [`HolderG`] when a thread-local variable starts being used.
    /// Panics if `self`'s mutex is poisoned.
    fn register_node(&self, node: P::Node, tid: ThreadId) {
        let mut lock = self.lock();
        lock.register_node(node, tid)
    }
}

impl<P> ControlG<P, WithNode>
where
    P: CoreParam + CtrlStateParam + HldrParam<Hldr = HolderG<P, WithNode>>,

    P: NodeParam + CtrlStateParam + GDataParam,
    P::CtrlState: CtrlStateCore<P>,
    P::GData: GuardedData<P::Dat, Arg = P::Dat>,
    P::CtrlState: CtrlStateWithNode<P>,
    Self: WithNodeFn<P, NodeFnArg = Self>,
{
    fn node(&self) -> P::Node {
        Self::node_fn(self)
    }

    /// Invokes `f` on the held data.
    /// Returns an error if [`HolderG`] not linked with [`ControlG`].
    pub fn with_data<V>(&self, f: impl FnOnce(&P::Dat) -> V) -> V {
        self.tl.with(|h| {
            h.ensure_linked(self, Self::node);
            h.with_data(f)
        })
    }

    /// Invokes `f` mutably on the held data.
    /// Returns an error if [`HolderG`] not linked with [`ControlG`].
    pub fn with_data_mut<V>(&self, f: impl FnOnce(&mut P::Dat) -> V) -> V {
        self.tl.with(|h| {
            h.ensure_linked(self, Self::node);
            h.with_data_mut(f)
        })
    }
}

impl<P, D> Clone for ControlG<P, D>
where
    P: CoreParam + CtrlStateParam + HldrParam,
    P::Hldr: Hldr,
{
    fn clone(&self) -> Self {
        Self {
            tl: self.tl,
            state: self.state.clone(),
            op: self.op.clone(),
            _d: PhantomData,
        }
    }
}

impl<P, D> Debug for ControlG<P, D>
where
    P: CoreParam + CtrlStateParam + HldrParam,
    P::Hldr: Hldr,

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
pub struct HolderG<P, D>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlStateParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = P::Dat>,
    P::CtrlState: CtrlStateCore<P>,
{
    pub(crate) data: P::GData,
    pub(crate) control: RefCell<Option<ControlG<P, D>>>,
    pub(crate) make_data: fn() -> P::Dat,
    _d: PhantomData<D>,
}

impl<P, D> Hldr for HolderG<P, D>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlStateParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = P::Dat>,
    P::CtrlState: CtrlStateCore<P>,
{
}

impl<P, D> HolderG<P, D>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlStateParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = P::Dat>,
    P::CtrlState: CtrlStateCore<P>,
{
    /// Instantiates a holder object. The `make_data` function produces the value used to initialize the
    /// held data.
    pub fn new(make_data: fn() -> P::Dat) -> Self {
        Self {
            data: P::GData::new(make_data()),
            control: RefCell::new(None),
            make_data,
            _d: PhantomData,
        }
    }

    /// Returns reference to `control` field.
    pub(crate) fn control(&self) -> Ref<'_, Option<ControlG<P, D>>> {
        self.control.borrow()
    }

    /// Returns data guard for the held data.
    pub(crate) fn data_guard(&self) -> <P::GData as GuardedData<P::Dat>>::Guard<'_> {
        self.data.guard()
    }

    /// Invokes `f` on the held data.
    /// Returns an error if [`HolderG`] not linked with [`ControlG`].
    pub(crate) fn with_data<V>(&self, f: impl FnOnce(&P::Dat) -> V) -> V {
        let guard = self.data_guard();
        f(&guard)
    }

    /// Invokes `f` mutably on the held data.
    /// Returns an error if [`HolderG`] not linked with [`ControlG`].
    pub(crate) fn with_data_mut<V>(&self, f: impl FnOnce(&mut P::Dat) -> V) -> V {
        let mut guard = self.data_guard();
        f(&mut guard)
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

impl<P> HolderG<P, NoNode>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlStateParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = P::Dat>,
    P::CtrlState: CtrlStateCore<P>,
{
    /// Initializes the `control` field in [`HolderG`].
    fn link(&self, control: &ControlG<P, NoNode>) {
        let mut ctrl_ref = self.control.borrow_mut();
        *ctrl_ref = Some(control.clone());
    }

    /// Ensures `self` is linkded with `control`.
    pub(crate) fn ensure_linked(&self, control: &ControlG<P, NoNode>) {
        if self.control().as_ref().is_none() {
            self.link(control);
        }
    }
}

impl<P> HolderG<P, WithNode>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlStateParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = P::Dat>,
    P::CtrlState: CtrlStateCore<P>,

    P: NodeParam,
    P::CtrlState: CtrlStateWithNode<P>,
{
    /// Initializes the `control` field in [`HolderG`] when a node type is used.
    fn link(&self, control: &ControlG<P, WithNode>, node: P::Node) {
        let mut ctrl_ref = self.control.borrow_mut();
        *ctrl_ref = Some(control.clone());
        control.register_node(node, thread::current().id())
    }

    /// Ensures `self` is linked to `control` when a node type is used.
    pub(crate) fn ensure_linked(
        &self,
        control: &ControlG<P, WithNode>,
        node: fn(&ControlG<P, WithNode>) -> P::Node,
    ) {
        if self.control().as_ref().is_none() {
            self.link(control, node(control));
        }
    }
}

impl<P, D> Debug for HolderG<P, D>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlStateParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = P::Dat>,
    P::CtrlState: CtrlStateCore<P>,

    P::GData: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Holder{{data: {:?}}}", &self.data))
    }
}

impl<P, D> Drop for HolderG<P, D>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlStateParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = P::Dat>,
    P::CtrlState: CtrlStateCore<P>,
{
    fn drop(&mut self) {
        self.drop_data()
    }
}

//=================
// Control sub-state struct with a thread map.

/// Type used by the specialization of [`ControlG`] for module [`super::probed`].
//  Holds the thread map used by the [`ControlG`] specializations for module [`super::probed`].
//  Used also for module [`super::joined_old`] which shows a previous more complex implementation of module
//  [`super::joined`].
//  Also used to partially discriminate common [`ControlG`] functionality used by those modules.
#[derive(Debug)]
pub struct TmapD<P>
where
    P: CoreParam + NodeParam + GDataParam,
{
    // pub(crate) node: fn(&P::GData) -> P::Node,
    pub(crate) tmap: HashMap<ThreadId, P::Node>,
}

impl<P> CoreParam for TmapD<P>
where
    P: CoreParam + NodeParam + GDataParam,
{
    type Acc = P::Acc;
    type Dat = P::Dat;
}

impl<P> NodeParam for TmapD<P>
where
    P: CoreParam + NodeParam + GDataParam,
{
    type Node = P::Node;
}

impl<P> CtrlStateParam for TmapD<P>
where
    P: CoreParam + NodeParam + GDataParam,
{
    type CtrlState = CtrlStateG<Self>;
    // type StateArg = fn(&P::GData) -> P::Node;
}

impl<P> SubStateParam for TmapD<P>
where
    P: CoreParam + NodeParam + GDataParam,
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
    P: CoreParam + NodeParam + GDataParam,
{
    type Arg = ();

    fn new(_: ()) -> Self {
        Self {
            tmap: HashMap::new(),
        }
    }
}

impl<P> CtrlStateCore<TmapD<P>> for CtrlStateG<TmapD<P>>
where
    P: CoreParam + NodeParam + GDataParam,
{
    fn tl_data_dropped(
        &mut self,
        op: &(dyn Fn(P::Dat, &mut P::Acc, ThreadId) + Send + Sync),
        data: P::Dat,
        tid: ThreadId,
    ) {
        self.s.tmap.remove(&tid);
        let acc = self.acc_mut_priv();
        op(data, acc, tid);
    }
}

impl<P> CtrlStateWithNode<TmapD<P>> for CtrlStateG<TmapD<P>>
where
    P: CoreParam + NodeParam + GDataParam,
{
    fn register_node(&mut self, node: <P as NodeParam>::Node, tid: ThreadId) {
        self.s.tmap.insert(tid, node);
    }
}
