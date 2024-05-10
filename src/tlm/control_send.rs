use super::common::{
    ControlG, CoreParam, CtrlStateCore, CtrlStateParam, CtrlStateWithNode, DefaultDiscr,
    GDataParam, GuardedData, Hldr, HldrParam, HolderG, New, NodeParam, WithNode,
};
use std::{
    mem::replace,
    ops::Deref,
    sync::Arc,
    thread::{self, LocalKey, ThreadId},
};
use thiserror::Error;

/// Errors returned by [`Control::drain_tls`].
#[derive(Error, Debug)]
/// Method was called while some thread that sent a value for accumulation was still active.
#[error("method called while thread-locals were arctive")]
pub struct ActiveThreadLocalsError;

pub trait SentDataParam {
    type SDat;
    type AccDat;
}

pub struct ControlSendG<P, D>
where
    P: CoreParam<Acc = P::AccDat, Dat = P::AccDat> + CtrlStateParam + HldrParam + SentDataParam,
    P::Hldr: Hldr,

    P: 'static,
{
    control: ControlG<P, D>,
    /// Produces a zero value of type `P::Dat`, which is needed to obtain consistent aggregation results.
    acc_zero: Arc<dyn Fn() -> P::Acc + Send + Sync>,
    /// Operation that combines the stored thead-local value with data sent from threads.
    #[allow(clippy::type_complexity)]
    op: Arc<dyn Fn(P::SDat, &mut P::Dat, ThreadId) + Send + Sync>,
    // /// Binary operation that combines thread-local stored value with accumulated value.
    // op_r: Arc<dyn Fn(P::Dat, P::Acc) -> P::Acc + Send + Sync>,
}

impl<P, D> ControlSendG<P, D>
where
    P: CoreParam<Acc = P::AccDat, Dat = P::AccDat> + CtrlStateParam + HldrParam + SentDataParam,
    P::Hldr: Hldr,

    P::CtrlState: CtrlStateCore<P> + New<P::CtrlState, Arg = P::Acc>,
{
    pub fn new(
        tl: &'static LocalKey<P::Hldr>,
        acc_zero: impl Fn() -> P::Acc + 'static + Send + Sync + Clone,
        op: impl Fn(P::SDat, &mut P::Dat, ThreadId) + 'static + Send + Sync,
        op_r: impl Fn(P::Dat, P::Acc) -> P::Acc + 'static + Send + Sync,
    ) -> Self {
        // Clone below is typically very cheap because `acc_zero` is typically a function pointer.
        let acc_zero1 = acc_zero.clone();
        Self {
            control: ControlG::new(tl, acc_zero(), move |data: P::Dat, acc: &mut P::Acc, _| {
                let zero = acc_zero();
                let acc0 = replace(acc, zero);
                *acc = op_r(data, acc0);
            }),
            acc_zero: Arc::new(acc_zero1),
            op: Arc::new(op),
        }
    }
}

impl<P, D> ControlSendG<P, D>
where
    P: CoreParam<Acc = P::AccDat, Dat = P::AccDat> + CtrlStateParam + HldrParam + SentDataParam,
    P::Hldr: Hldr,

    P::CtrlState: CtrlStateCore<P>,
{
    pub fn drain_tls(&mut self) -> Result<P::Acc, ActiveThreadLocalsError> {
        if Arc::strong_count(&self.control.state) > 0 {
            Err(ActiveThreadLocalsError)
        } else {
            let acc = self.control.take_acc((self.acc_zero)());
            Ok(acc)
        }
    }
}

impl<P> ControlSendG<P, DefaultDiscr>
where
    P: CoreParam<Acc = P::AccDat, Dat = P::AccDat>
        + CtrlStateParam
        + HldrParam<Hldr = HolderG<P, DefaultDiscr>>
        + SentDataParam,

    P: GDataParam,
    P::CtrlState: CtrlStateCore<P>,
    P::GData: GuardedData<P::Dat, Arg = P::Dat>,
{
    pub fn send_data(&self, sent_data: P::SDat) {
        self.control
            .with_data_mut(|data| (self.op)(sent_data, data, thread::current().id()))
    }
}

impl<P> ControlSendG<P, WithNode>
where
    P: CoreParam<Acc = P::AccDat, Dat = P::AccDat>
        + CtrlStateParam
        + HldrParam<Hldr = HolderG<P, WithNode>>
        + SentDataParam,

    P: GDataParam,
    P: NodeParam<NodeFnArg = ControlG<P, WithNode>>,
    P::CtrlState: CtrlStateWithNode<P>,
    P::GData: GuardedData<P::Dat, Arg = P::Dat>,
{
    pub fn send_data(&self, sent_data: P::SDat) {
        self.control
            .with_data_mut(|data| (self.op)(sent_data, data, thread::current().id()))
    }
}

impl<P, D> Deref for ControlSendG<P, D>
where
    P: CoreParam<Acc = P::AccDat, Dat = P::AccDat> + CtrlStateParam + HldrParam + SentDataParam,
    P::Hldr: Hldr,
{
    type Target = ControlG<P, D>;

    fn deref(&self) -> &Self::Target {
        &self.control
    }
}

/// Can't use `#[derive(Clone)]` because it doesn't work due to `Deref` impl.
impl<P, D> Clone for ControlSendG<P, D>
where
    P: CoreParam<Acc = P::AccDat, Dat = P::AccDat> + CtrlStateParam + HldrParam + SentDataParam,
    P::Hldr: Hldr,
{
    fn clone(&self) -> Self {
        Self {
            control: self.control.clone(),
            acc_zero: self.acc_zero.clone(),
            op: self.op.clone(),
        }
    }
}

/// Comment out the manual clone implementation above and see what happens below.
#[allow(unused)]
fn demonstrate_need_for_manual_clone<P, D>(x: ControlSendG<P, D>, y: &ControlSendG<P, D>)
where
    P: CoreParam<Acc = P::AccDat, Dat = P::AccDat> + CtrlStateParam + HldrParam + SentDataParam,
    P::Hldr: Hldr,
{
    let x1: ControlSendG<P, D> = x.clone();
    let y1: ControlSendG<P, D> = y.clone();
}
