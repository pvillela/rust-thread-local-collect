//! This module implemnets [`HolderG`], a highly generic struct that holds thread-local data and enables the
//! linkage of the held data with the control object of type [`super::ControlG`].
//! The `Holder`type alias in various modules is a specialization of this struct.

use super::{
    common_traits::*,
    control_g::{DefaultDiscr, WithNode},
};

use std::{
    cell::{Ref, RefCell, RefMut},
    fmt::Debug,
    marker::PhantomData,
    mem::take,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
    thread,
};

pub(crate) const POISONED_GUARDED_DATA_MUTEX: &str = "poisoned guarded data mutex";

impl<S> New<Self> for RefCell<S> {
    type Arg = S;

    fn new(t: Self::Arg) -> Self {
        RefCell::new(t)
    }
}

impl<T: 'static> GuardedData<T> for RefCell<Option<T>> {
    type Guard<'a> = RefMut<'a, Option<T>>;

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

impl<T: 'static> GuardedData<T> for Arc<Mutex<Option<T>>> {
    type Guard<'a> = MutexGuard<'a, Option<T>>;

    fn guard(&self) -> Self::Guard<'_> {
        let res = self.lock().expect(POISONED_GUARDED_DATA_MUTEX);
        res
    }
}

trait Unwrap<T> {
    fn unwrap(&self) -> &T;
    fn unwrap_mut(&mut self) -> &mut T;
}

const UNWRAP_MSG: &str = "must only be called after ensuring Holder is initialized";

impl<T, R> Unwrap<T> for R
where
    R: DerefMut<Target = Option<T>>,
{
    fn unwrap(&self) -> &T {
        self.as_ref().expect(UNWRAP_MSG)
    }

    fn unwrap_mut(&mut self) -> &mut T {
        self.as_mut().expect(UNWRAP_MSG)
    }
}

/// Highly generic struct that holds thread-local data and a smart pointer to a [`super::ControlG`], enabling the
/// linkage of the held data with the control object.
/// Used to implement [`crate::tlm::joined::Holder`], [`crate::tlm::probed::Holder`], and
/// [`crate::tlm::simple_joined::Holder`].
///
/// The trait bounds of type parameter `P` are used to customize `impl`s. The thread-local
/// values are of type `CoreParam::Dat` and the accumulated value is of type `CoreParam::Acc`.
///
/// Type parameter `D` is used as a discriminant to enable different implementations of the same method.
pub struct HolderG<P, D>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = Option<P::Dat>>,
    P::Ctrl: Ctrl<P>,
{
    pub(crate) data: P::GData,
    pub(crate) control: RefCell<Option<P::Ctrl>>,
    _d: PhantomData<D>,
}

impl<P, D> HolderG<P, D>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = Option<P::Dat>>,
    P::Ctrl: Ctrl<P>,
{
    /// Instantiates a holder object.
    pub fn new() -> Self {
        Self {
            data: P::GData::new(None),
            control: RefCell::new(None),
            _d: PhantomData,
        }
    }

    /// Returns reference to `control` field.
    pub(crate) fn control(&self) -> Ref<'_, Option<P::Ctrl>> {
        self.control.borrow()
    }

    fn is_linked(&self) -> bool {
        self.control().as_ref().is_some()
    }

    /// Returns data guard for the held data, ensuring the data is initialized.
    pub(crate) fn data_guard(&self) -> <P::GData as GuardedData<P::Dat>>::Guard<'_> {
        let mut guard = self.data.guard();
        if guard.is_none() {
            let control_guard = self.control();
            let control = control_guard
                .as_ref()
                .expect("holder must be linked to control");
            let data = guard.deref_mut();
            *data = Some(control.make_data());
        }
        guard
    }

    /// Used by [`Drop`] trait impl.
    fn drop_data(&self) {
        match self.control().deref() {
            None => (),
            Some(control) => {
                let mut data_guard = self.data_guard();
                let data = take(data_guard.deref_mut());
                if let Some(data) = data {
                    control.tl_data_dropped(data, thread::current().id())
                };
            }
        }
    }
}

impl<P, D> HldrData<P> for HolderG<P, D>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = Option<P::Dat>>,
    P::Ctrl: Ctrl<P>,
{
    /// Invokes `f` on the held data.
    /// Returns an error if [`HolderG`] not linked with [`super::ControlG`].
    fn with_data<V>(&self, f: impl FnOnce(&P::Dat) -> V) -> V {
        let guard = self.data_guard();
        f(guard.unwrap())
    }

    /// Invokes `f` mutably on the held data.
    /// Returns an error if [`HolderG`] not linked with [`super::ControlG`].
    fn with_data_mut<V>(&self, f: impl FnOnce(&mut P::Dat) -> V) -> V {
        let mut guard = self.data_guard();
        f(guard.unwrap_mut())
    }
}

impl<P> HldrLink<P> for HolderG<P, DefaultDiscr>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = Option<P::Dat>>,

    P::Ctrl: Ctrl<P> + Clone,
{
    /// Initializes the `control` field in [`HolderG`].
    fn link(&self, control: &P::Ctrl) {
        let mut ctrl_ref = self.control.borrow_mut();
        *ctrl_ref = Some(control.clone());
    }

    fn is_linked(&self) -> bool {
        Self::is_linked(self)
    }
}

impl<P> HldrLink<P> for HolderG<P, WithNode>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = Option<P::Dat>>,

    P: NodeParam<NodeFnArg = P::Ctrl>,
    P::Ctrl: Ctrl<P> + CtrlNode<P> + Clone,
{
    /// Initializes the `control` field in [`HolderG`] when a node type is used.
    fn link(&self, control: &P::Ctrl) {
        let mut ctrl_ref = self.control.borrow_mut();
        *ctrl_ref = Some(control.clone());
        control.register_node(P::node_fn(control), thread::current().id());
    }

    fn is_linked(&self) -> bool {
        Self::is_linked(self)
    }
}

impl<P, D> Debug for HolderG<P, D>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = Option<P::Dat>>,
    P::Ctrl: Ctrl<P>,

    P::GData: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Holder{{data: {:?}}}", &self.data))
    }
}

impl<P, D> Drop for HolderG<P, D>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = Option<P::Dat>>,
    P::Ctrl: Ctrl<P>,
{
    fn drop(&mut self) {
        self.drop_data()
    }
}
