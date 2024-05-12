use super::{
    common_traits::*,
    control_g::{ControlG, DefaultDiscr, WithNode},
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

//=================
// Errora

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
        self.lock().expect(POISONED_GUARDED_DATA_MUTEX)
    }
}

trait Unwrap<T> {
    fn unwrap(&self) -> &T;
    fn unwrap_mut(&mut self) -> &mut T;
}

impl<T, R> Unwrap<T> for R
where
    R: DerefMut<Target = Option<T>>,
{
    fn unwrap(&self) -> &T {
        self.as_ref()
            .expect("must only be called after ensuring Holder is initialized")
    }

    fn unwrap_mut(&mut self) -> &mut T {
        self.as_mut()
            .expect("must only be called after ensuring Holder is initialized")
    }
}

/// Holds thread-local data and a smart pointer to a [`ControlG`], enabling the linkage of the held data
/// with the control object.
pub struct HolderG<P, D>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlStateParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = Option<P::Dat>>,
    P::CtrlState: CtrlStateCore<P>,
{
    pub(crate) data: P::GData,
    pub(crate) control: RefCell<Option<ControlG<P, D>>>,
    _d: PhantomData<D>,
}

impl<P, D> Hldr for HolderG<P, D>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlStateParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = Option<P::Dat>>,
    P::CtrlState: CtrlStateCore<P>,
{
}

impl<P, D> HolderG<P, D>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlStateParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = Option<P::Dat>>,
    P::CtrlState: CtrlStateCore<P>,
{
    /// Instantiates a holder object. The `make_data` function produces the value used to initialize the
    /// held data.
    pub fn new() -> Self {
        Self {
            data: P::GData::new(None),
            control: RefCell::new(None),
            _d: PhantomData,
        }
    }

    /// Returns reference to `control` field.
    pub(crate) fn control(&self) -> Ref<'_, Option<ControlG<P, D>>> {
        self.control.borrow()
    }

    /// Returns data guard for the held data, ensuring the data is initialized.
    pub(crate) fn data_guard(&self) -> <P::GData as GuardedData<P::Dat>>::Guard<'_> {
        let mut data_guard = self.data.guard();
        let data = data_guard.deref_mut();
        if data.is_none() {
            let control_ref = self.control();
            let control = control_ref
                .as_ref()
                .expect("only called after ensure_linked");
            *data = Some((control.make_data)());
        }
        data_guard
    }

    /// Invokes `f` on the held data.
    /// Returns an error if [`HolderG`] not linked with [`ControlG`].
    pub(crate) fn with_data<V>(&self, f: impl FnOnce(&P::Dat) -> V) -> V {
        let guard = self.data_guard();
        f(guard.unwrap())
    }

    /// Invokes `f` mutably on the held data.
    /// Returns an error if [`HolderG`] not linked with [`ControlG`].
    pub(crate) fn with_data_mut<V>(&self, f: impl FnOnce(&mut P::Dat) -> V) -> V {
        let mut guard = self.data_guard();
        f(guard.unwrap_mut())
    }

    /// Used by [`Drop`] trait impl.
    fn drop_data(&self) {
        match self.control().deref() {
            None => (),
            Some(control) => {
                let mut data_guard = self.data_guard();
                let data = take(data_guard.deref_mut());
                control.tl_data_dropped(
                    data.expect("Holder data guaranteed to be initialized on first use."),
                    thread::current().id(),
                );
            }
        }
    }
}

impl<P> HolderG<P, DefaultDiscr>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlStateParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = Option<P::Dat>>,
    P::CtrlState: CtrlStateCore<P>,
{
    /// Initializes the `control` field in [`HolderG`].
    fn link(&self, control: &ControlG<P, DefaultDiscr>) {
        let mut ctrl_ref = self.control.borrow_mut();
        *ctrl_ref = Some(control.clone());
    }

    /// Ensures `self` is linkded with `control`.
    pub(crate) fn ensure_linked(&self, control: &ControlG<P, DefaultDiscr>) {
        if self.control().as_ref().is_none() {
            self.link(control);
        }
    }
}

impl<P> HolderG<P, WithNode>
where
    P: CoreParam + GDataParam + HldrParam<Hldr = Self> + CtrlStateParam + 'static,
    P::GData: GuardedData<P::Dat, Arg = Option<P::Dat>>,
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
    P::GData: GuardedData<P::Dat, Arg = Option<P::Dat>>,
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
    P::GData: GuardedData<P::Dat, Arg = Option<P::Dat>>,
    P::CtrlState: CtrlStateCore<P>,
{
    fn drop(&mut self) {
        self.drop_data()
    }
}
