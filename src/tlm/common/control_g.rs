//! This module implements [`ControlG`], a highly generic struct that controls the collection and accumulation of
//! thread-local values linked to this object.
//! The `Control`type alias in various modules is a specialization of this struct.

use super::common_traits::*;

use std::{
    fmt::Debug,
    marker::PhantomData,
    mem::replace,
    ops::Deref,
    sync::{Arc, Mutex, MutexGuard},
    thread::{LocalKey, ThreadId},
};

//=================
// Errora

pub(crate) const POISONED_CONTROL_MUTEX: &str = "poisoned control mutex";

//=================
// Core structs and impls

#[doc(hidden)]
/// Type used to discriminate default `impl`s. Could have used `()` instead.
#[derive(Debug)]
pub struct DefaultDiscr;

#[doc(hidden)]
/// Type used to discriminate `impl`s that use [`NodeParam`] and do not need to be further discriminated.
#[derive(Debug)]
pub struct WithNode;

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
pub struct CtrlStateG<P, D>
where
    P: CoreParam + SubStateParam,
{
    pub(crate) acc: P::Acc,
    pub(crate) s: P::SubState,
    _d: PhantomData<D>,
}

impl<P, D> CtrlStateG<P, D>
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

impl<P, D> New<Self> for CtrlStateG<P, D>
where
    P: CoreParam + SubStateParam,

    P::SubState: New<P::SubState, Arg = ()>,
{
    type Arg = P::Acc;

    fn new(acc_base: P::Acc) -> Self {
        Self {
            acc: acc_base,
            s: P::SubState::new(()),
            _d: PhantomData,
        }
    }
}

impl<P, D> WithAcc for CtrlStateG<P, D>
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

impl<P> CtrlStateCore<P> for CtrlStateG<P, DefaultDiscr>
where
    P: CoreParam + SubStateParam,
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

/// Highly generic struct that controls the collection and accumulation of thread-local values linked to this object.
/// Used to implement [`crate::tlm::joined::Control`], [`crate::tlm::probed::Control`], and
/// [`crate::tlm::simple_joined::Control`].
///
/// The trait bounds of type parameter `P` are used to customize `impl`s. The thread-local
/// values are of type `CoreParam::Dat` and the accumulated value is of type `CoreParam::Acc`.
pub struct ControlG<P>
where
    P: CoreParam + CtrlStateParam + HldrParam,

    P: 'static,
{
    /// Reference to thread-local
    pub(crate) tl: &'static LocalKey<P::Hldr>,
    /// Keeps track of linked thread-locals and accumulated value.
    pub(crate) state: Arc<Mutex<P::CtrlState>>,
    /// Constructs initial data for [`HolderG`].
    pub(crate) make_data: fn() -> P::Dat,
    /// Operation that combines data from thread-locals with accumulated value.
    #[allow(clippy::type_complexity)]
    pub(crate) op: Arc<dyn Fn(P::Dat, &mut P::Acc, ThreadId) + Send + Sync>,
}

impl<P> ControlG<P>
where
    P: CoreParam + CtrlStateParam + HldrParam,

    P::CtrlState: New<P::CtrlState, Arg = P::Acc>,
{
    /// Instantiates a *control* object.
    ///
    /// - `tl` - reference to thread-local static.
    /// - `acc_base` - initial value for accumulation.
    /// - `make_data` - constructs initial data for [`super::HolderG`].
    /// - `op` - operation that combines data from thread-locals with accumulated value.
    pub fn new(
        tl: &'static LocalKey<P::Hldr>,
        acc_base: P::Acc,
        make_data: fn() -> P::Dat,
        op: impl Fn(P::Dat, &mut P::Acc, ThreadId) + 'static + Send + Sync,
    ) -> Self {
        let state = P::CtrlState::new(acc_base);
        Self {
            tl,
            state: Arc::new(Mutex::new(state)),
            make_data,
            op: Arc::new(op),
        }
    }
}

impl<P> ControlG<P>
where
    P: CoreParam + CtrlStateParam + HldrParam,

    P::CtrlState: CtrlStateCore<P>,
{
    /// Acquires a lock on [`ControlG`]'s internal Mutex.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    pub(crate) fn lock(&self) -> MutexGuard<'_, P::CtrlState> {
        self.state.lock().expect(POISONED_CONTROL_MUTEX)
    }

    /// Returns a guard object that dereferences to `self`'s accumulated value. A lock is held during the guard's
    /// lifetime.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    pub fn acc(&self) -> impl Deref<Target = P::Acc> + '_ {
        AccGuardG::new(self.lock())
    }

    /// Provides access to `self`'s accumulated value.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    pub fn with_acc<V>(&self, f: impl FnOnce(&P::Acc) -> V) -> V {
        let acc = self.acc();
        f(&acc)
    }

    /// Returns a clone of `self`'s accumulated value.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    pub fn clone_acc(&self) -> P::Acc
    where
        P::Acc: Clone,
    {
        self.acc().clone()
    }

    /// Returns `self`'s accumulated value, using a value of the same type to replace
    /// the existing accumulated value.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    pub fn take_acc(&self, replacement: P::Acc) -> P::Acc {
        let mut lock = self.lock();
        let acc = lock.acc_mut();
        replace(acc, replacement)
    }
}

impl<P> Ctrl<P> for ControlG<P>
where
    P: CoreParam + CtrlStateParam + HldrParam,

    P::CtrlState: CtrlStateCore<P>,
{
    fn make_data(&self) -> P::Dat {
        (self.make_data)()
    }

    /// Used by [`super::HolderG`] to notify [`ControlG`] that the holder's data has been dropped.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    fn tl_data_dropped(&self, data: <P as CoreParam>::Dat, tid: ThreadId) {
        let mut lock = self.lock();
        lock.tl_data_dropped(self.op.deref(), data, tid);
    }
}

impl<P> ControlG<P>
where
    P: CoreParam + CtrlStateParam + HldrParam,

    P::CtrlState: CtrlStateCore<P>,
    P: CtrlParam<Ctrl = Self>,
    P::Hldr: HldrLink<P> + HldrData<P>,
{
    /// Invokes `f` on the held data.
    pub fn with_data<V>(&self, f: impl FnOnce(&P::Dat) -> V) -> V {
        self.tl.with(|h| {
            h.ensure_linked(self);
            h.with_data(f)
        })
    }

    /// Invokes `f` mutably on the held data.
    pub fn with_data_mut<V>(&self, f: impl FnOnce(&mut P::Dat) -> V) -> V {
        self.tl.with(|h| {
            h.ensure_linked(self);
            h.with_data_mut(f)
        })
    }
}

impl<P> CtrlNode<P> for ControlG<P>
where
    P: CoreParam + CtrlStateParam + HldrParam,

    P: NodeParam,
    P::CtrlState: CtrlStateWithNode<P>,
{
    /// Called by [`super::HolderG`] when a thread-local variable starts being used.
    ///
    /// # Panics
    /// If `self`'s mutex is poisoned.
    fn register_node(&self, node: P::Node, tid: ThreadId) {
        let mut lock = self.lock();
        lock.register_node(node, tid)
    }
}

impl<P> Clone for ControlG<P>
where
    P: CoreParam + CtrlStateParam + HldrParam,
{
    fn clone(&self) -> Self {
        Self {
            tl: self.tl,
            state: self.state.clone(),
            make_data: self.make_data,
            op: self.op.clone(),
        }
    }
}

impl<P> Debug for ControlG<P>
where
    P: CoreParam + CtrlStateParam + HldrParam,

    P::CtrlState: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Control({:?})", self.state))
    }
}
