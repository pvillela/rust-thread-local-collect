use std::{
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    fmt::Debug,
    mem::replace,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, ThreadId},
};

pub trait ControlState {
    type Node;
    type Acc;
    type Dat;

    fn acc(&self) -> &Self::Acc;
    fn acc_mut(&mut self) -> &mut Self::Acc;
    fn register_node(&mut self, node: &Self::Node, tid: &ThreadId);
    fn deregister_thread(&mut self, tid: &ThreadId);
    fn ensure_tls_dropped(
        &mut self,
        op: Arc<dyn Fn(Self::Dat, &mut Self::Acc, &ThreadId) + Send + Sync>,
    );
}

#[derive(Debug)]
pub struct ControlStateS<T, U, Node> {
    pub(crate) acc: U,
    pub(crate) tmap: HashMap<ThreadId, Node>,
    ensure_tls_dropped: fn(state: &mut Self, op: Arc<dyn Fn(T, &mut U, &ThreadId) + Send + Sync>),
}

impl<T, U, Node> ControlStateS<T, U, Node> {
    pub(crate) fn new(
        acc: U,
        ensure_tls_dropped: fn(
            this: &mut Self,
            op: Arc<dyn Fn(T, &mut U, &ThreadId) + Send + Sync>,
        ),
    ) -> Self {
        Self {
            acc,
            tmap: HashMap::new(),
            ensure_tls_dropped,
        }
    }
}

impl<T, U, Node> ControlState for ControlStateS<T, U, Node>
where
    Node: Clone,
{
    type Acc = U;
    type Dat = T;
    type Node = Node;

    fn acc(&self) -> &U {
        &self.acc
    }

    fn acc_mut(&mut self) -> &mut U {
        &mut self.acc
    }

    fn register_node(&mut self, node: &Self::Node, tid: &ThreadId) {
        self.tmap.insert(tid.clone(), node.clone());
    }

    fn deregister_thread(&mut self, tid: &ThreadId) {
        self.tmap.remove(tid);
    }

    fn ensure_tls_dropped(&mut self, op: Arc<dyn Fn(T, &mut U, &ThreadId) + Send + Sync>) {
        (self.ensure_tls_dropped)(self, op)
    }
}

/// Controls the destruction of thread-local values registered with it.
/// Such values of type `T` must be held in thread-locals of type [`Holder<T>`].
/// `U` is the type of the accumulated value resulting from an initial base value and
/// the application of an operation to each thread-local value and the current accumulated
/// value upon dropping of each thread-local value. (See [`new`](Control::new) method.)
pub struct ControlS<T, State>
where
    State: ControlState,
{
    /// Keeps track of registered threads and accumulated value.
    pub(crate) state: Arc<Mutex<State>>,
    /// Binary operation that combines data from thread-locals with accumulated value.
    #[allow(clippy::type_complexity)]
    pub(crate) op: Arc<dyn Fn(T, &mut State::Acc, &ThreadId) + Send + Sync>,
}

/// Locking functionality underlying [`ControlC`].
impl<T, State> ControlS<T, State>
where
    State: ControlState<Dat = T>,
{
    fn op(&self, data: T, acc: &mut State::Acc, tid: &ThreadId) {
        (self.op)(data, acc, tid)
    }

    /// Acquires a lock for use by public `Control` methods that require its internal Mutex to be locked.
    ///
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub fn lock<'a>(&'a self) -> MutexGuard<'a, State> {
        self.state.lock().unwrap()
    }

    fn accumulate_tl(&self, lock: &mut MutexGuard<'_, State>, data: T, tid: &ThreadId) {
        let acc = lock.deref_mut().acc_mut();
        self.op(data, acc, tid);
    }

    /// Provides access to the accumulated value in the [Control] struct.
    ///
    /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub fn with_acc<V>(&self, lock: &MutexGuard<'_, State>, f: impl FnOnce(&State::Acc) -> V) -> V {
        let acc = lock.deref().acc();
        f(acc)
    }

    /// Returns the accumulated value in the [Control] struct, using a value of the same type to replace
    /// the existing accumulated value.
    ///
    /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub fn take_acc(
        &self,
        lock: &mut MutexGuard<'_, State>,
        replacement: State::Acc,
    ) -> State::Acc {
        let acc = lock.deref_mut().acc_mut();
        replace(acc, replacement)
    }

    fn register_node(&self, node: &State::Node, tid: &ThreadId) {
        let mut lock = self.lock();
        lock.register_node(node, tid)
    }

    /// Forces all registered thread-local values that have not already been dropped to be effectively dropped
    /// by replacing the [`Holder`] data with [`None`], and accumulates the values contained in those thread-locals.
    ///
    /// Should only be called from a thread (typically the main thread) under the following conditions:
    /// - All other threads that use this [`Control`] instance must have been directly or indirectly spawned
    ///   from this thread; ***and***
    /// - Any prior updates to holder values must have had a *happened before* relationship to this call;
    ///   ***and***
    /// - Any further updates to holder values must have a *happened after* relationship to this call.
    ///   
    /// In particular, the last two conditions are satisfied if the call to this method takes place after
    /// this thread joins (directly or indirectly) with all threads that have registered with this [`Control`]
    /// instance.
    ///
    /// These conditions ensure the absence of data races with a proper "happens-before" condition between any
    /// thread-local data updates and this call.
    ///
    /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub fn ensure_tls_dropped(&self, lock: &mut MutexGuard<'_, State>) {
        lock.ensure_tls_dropped(self.op.clone())
    }

    fn tl_data_dropped(&self, tid: &ThreadId, data: Option<T>) {
        let mut lock = self.lock();
        lock.deregister_thread(tid);
        if let Some(data) = data {
            self.accumulate_tl(&mut lock, data, tid);
        }
    }
}

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
pub struct HolderS<T, GData, CtrlState>
where
    T: 'static,
    GData: GuardedData<Option<T>> + 'static,
    CtrlState: ControlState<Dat = T>,
{
    pub(crate) data: GData,
    pub(crate) control: RefCell<Option<ControlS<T, CtrlState>>>,
    pub(crate) make_data: fn() -> T,
}

/// Common trait supporting different `Holder` implementations.
impl<T, GData, CtrlState> HolderS<T, GData, CtrlState>
where
    T: 'static,
    GData: GuardedData<Option<T>> + 'static,
    CtrlState: ControlState<Dat = T>,
{
    fn control(&self) -> Ref<'_, Option<ControlS<T, CtrlState>>> {
        self.control.borrow()
    }

    fn make_data(&self) -> T {
        (self.make_data)()
    }

    fn data_guard(&self) -> GData::Guard<'_> {
        self.data.guard()
    }

    /// Establishes link with control.
    pub(crate) fn init_control(&self, control: &ControlS<T, CtrlState>, node: &CtrlState::Node) {
        let mut ctrl_ref = self.control.borrow_mut();
        *ctrl_ref = Some(control.clone());
        control.register_node(node, &thread::current().id())
    }

    pub(crate) fn init_data(&self) {
        let mut guard = self.data_guard();
        let data = guard.deref_mut();
        if data.is_none() {
            *data = Some(self.make_data());
        }
    }

    pub(crate) fn ensure_initialized(
        &self,
        control: &ControlS<T, CtrlState>,
        node: &CtrlState::Node,
    ) {
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
        let control: Ref<'_, Option<ControlS<T, CtrlState>>> = self.control();
        if control.is_none() {
            return;
        }
        let control = Ref::map(control, |x| x.as_ref().unwrap());
        control.tl_data_dropped(&thread::current().id(), data);
    }

    /// Invokes `f` on data. Panics if data is [`None`].
    pub(crate) fn with_data<V>(&self, f: impl FnOnce(&T) -> V) -> V {
        let guard = self.data_guard();
        // f(guard.unwrap()) // instead of 2 lines below
        let data: Option<&T> = guard.as_ref();
        f(data.unwrap())
    }

    /// Invokes `f` on data. Panics if data is [`None`].
    pub(crate) fn with_data_mut<V>(&self, f: impl FnOnce(&mut T) -> V) -> V {
        let mut guard = self.data_guard();
        // f(guard.unwrap_mut()) // instead of 2 lines below
        let data: Option<&mut T> = guard.as_mut();
        f(data.unwrap())
    }
}

pub trait HolderLocalKey<T, Ctrl> {
    /// Establishes link with control.
    fn init_control(&'static self, control: &Ctrl);

    fn init_data(&'static self);

    fn ensure_initialized(&'static self, control: &Ctrl);

    /// Invokes `f` on data. Panics if data is [`None`].
    fn with_data<V>(&'static self, f: impl FnOnce(&T) -> V) -> V;

    /// Invokes `f` on data. Panics if data is [`None`].
    fn with_data_mut<V>(&'static self, f: impl FnOnce(&mut T) -> V) -> V;
}

impl<T, State> Clone for ControlS<T, State>
where
    State: ControlState,
{
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            op: self.op.clone(),
        }
    }
}

impl<T: Debug, State: Debug> Debug for ControlS<T, State>
where
    State: ControlState,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Control({:?})", self.state))
    }
}

impl<T, GData, CtrlState> Debug for HolderS<T, GData, CtrlState>
where
    T: Debug,
    GData: GuardedData<Option<T>> + Debug,
    CtrlState: ControlState<Dat = T>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Holder{{data: {:?}}}", &self.data))
    }
}

impl<T, GData, CtrlState> Drop for HolderS<T, GData, CtrlState>
where
    T: 'static,
    CtrlState: ControlState<Dat = T>,
    GData: GuardedData<Option<T>> + 'static,
{
    /// Ensures the held data, if any, is deregistered from the associated [`Control`] instance
    /// and the control instance's accumulation operation is invoked with the held data.
    fn drop(&mut self) {
        self.drop_data()
    }
}
