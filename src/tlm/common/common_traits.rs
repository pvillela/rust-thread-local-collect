//=================
// Param traits

use std::{ops::DerefMut, thread::ThreadId};

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
    type NodeFnArg;

    fn node_fn(arg: &Self::NodeFnArg) -> Self::Node;
}

// pub trait NodeFnParam {
//     type NodeFn;
// }

#[doc(hidden)]
/// Abstracts the type of [`GuardedData`] used by [`HolderG`].
pub trait GDataParam {
    type GData;
}

#[doc(hidden)]
/// Used in conjunction with [`Hldr`] to bstract the [`HolderG`] type to reduce circular dependencies between
/// [`ControlG`] and [`HolderG`].
pub trait HldrParam {
    type Hldr;
}

pub trait CtrlParam {
    type Ctrl;
}

//=================
// Hidden non-param traits

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

pub trait Ctrl<P>
where
    P: CoreParam,
{
    fn make_data(&self) -> P::Dat;
    fn tl_data_dropped(&self, data: P::Dat, tid: ThreadId);
}

pub trait CtrlNode<P>
where
    P: NodeParam,
{
    fn register_node(&self, node: P::Node, tid: ThreadId);
}

pub trait HldrLink<P>
where
    P: CoreParam + CtrlParam,
    P::Ctrl: Ctrl<P>,
{
    fn link(&self, control: &P::Ctrl);

    fn is_linked(&self) -> bool;

    fn ensure_linked(&self, control: &P::Ctrl) {
        if self.is_linked() {
            self.link(control);
        }
    }
}

pub trait HldrData<P>
where
    P: CoreParam,
{
    fn with_data<V>(&self, f: impl FnOnce(&P::Dat) -> V) -> V;

    fn with_data_mut<V>(&self, f: impl FnOnce(&mut P::Dat) -> V) -> V;
}

#[doc(hidden)]
/// Abstraction of data wrappers used by [`HolderG`] specializations for different modules.
pub trait GuardedData<T: 'static>: New<Self>
where
    Self: Sized,
{
    type Guard<'a>: DerefMut<Target = Option<T>> + 'a
    where
        Self: 'a;

    fn guard(&self) -> Self::Guard<'_>;
}
