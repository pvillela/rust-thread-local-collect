use super::common::{
    ControlG, CoreParam, CtrlStateCore, CtrlStateParam, CtrlStateWithNode, DefaultDiscr,
    GDataParam, GuardedData, Hldr, HldrParam, HolderG, New, NodeParam, WithNode,
};
use std::{
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
}

pub struct ControlSendG<P, D>
where
    P: CoreParam + CtrlStateParam + HldrParam + SentDataParam,
    P::Hldr: Hldr,

    P: 'static,
{
    pub(crate) control: ControlG<P, D>,
    /// Produces a zero value of type `P::Dat`, which is needed to obtain consistent aggregation results.
    acc_zero: Arc<dyn Fn() -> P::Acc + Send + Sync>,
    /// Operation that combines the stored thead-local value with data sent from threads.
    #[allow(clippy::type_complexity)]
    op: Arc<dyn Fn(P::SDat, &mut P::Dat, ThreadId) + Send + Sync>,
}

impl<P, D> ControlSendG<P, D>
where
    P: CoreParam + CtrlStateParam + HldrParam + SentDataParam,
    P::Hldr: Hldr,

    P::CtrlState: CtrlStateCore<P> + New<P::CtrlState, Arg = P::Acc>,
{
    pub(crate) fn new_priv(
        tl: &'static LocalKey<P::Hldr>,
        acc_zero: impl Fn() -> P::Acc + 'static + Send + Sync,
        op: impl Fn(P::SDat, &mut P::Dat, ThreadId) + 'static + Send + Sync,
        op_inner: impl Fn(P::Dat, &mut P::Acc, ThreadId) + 'static + Send + Sync,
    ) -> Self {
        Self {
            control: ControlG::new(tl, acc_zero(), op_inner),
            acc_zero: Arc::new(acc_zero),
            op: Arc::new(op),
        }
    }
}

impl<P, D> ControlSendG<P, D>
where
    P: CoreParam + CtrlStateParam + HldrParam + SentDataParam,
    P::Hldr: Hldr,

    P: CtrlStateParam,
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
    P: CoreParam + CtrlStateParam + HldrParam<Hldr = HolderG<P, DefaultDiscr>> + SentDataParam,

    P: CtrlStateParam + GDataParam,
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
    P: CoreParam + CtrlStateParam + HldrParam<Hldr = HolderG<P, WithNode>> + SentDataParam,

    P: NodeParam<NodeFnArg = ControlG<P, WithNode>>,
    P: CtrlStateParam + GDataParam,
    P::CtrlState: CtrlStateWithNode<P>,
    P::GData: GuardedData<P::Dat, Arg = P::Dat>,
{
    pub fn send_data(&self, sent_data: P::SDat) {
        self.control
            .with_data_mut(|data| (self.op)(sent_data, data, thread::current().id()))
    }
}
