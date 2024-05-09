use super::common::{
    ControlG, CoreParam, CtrlStateCore, CtrlStateParam, GDataParam, GuardedData, HolderG, New,
    NoNode, UseCtrlStateGDefault,
};
use std::{
    sync::Arc,
    thread::{self, LocalKey, ThreadId},
};

pub trait SentDataParam {
    type SDat;
}

pub struct ControlSendG<P, D>
where
    P: CoreParam + GDataParam + CtrlStateParam + 'static + SentDataParam,
    P::Dat: 'static,
    P::GData: GuardedData<P::Dat, Arg = P::Dat> + 'static,
    P::CtrlState: CtrlStateCore<P>,
    D: 'static,
{
    pub(crate) control: ControlG<P, D>,
    /// Produces a zero value of type `P::Dat`, which is needed to obtain consistent aggregation results.
    acc_zero: Arc<dyn Fn() -> P::Acc + Send + Sync>,
    /// Operation that combines the stored thead-local value with data sent from threads.
    #[allow(clippy::type_complexity)]
    op: Arc<dyn Fn(P::SDat, &mut P::Dat, ThreadId) + Send + Sync>,
    /// Binary operation that combines thread-local stored value with accumulated value.
    op_r: Arc<dyn Fn(P::Dat, P::Acc) -> P::Acc + Send + Sync>,
}

impl<P, D> ControlSendG<P, D>
where
    P: CoreParam + GDataParam + CtrlStateParam + 'static + SentDataParam,
    P::Dat: 'static,
    P::GData: GuardedData<P::Dat, Arg = P::Dat> + 'static,
    P::CtrlState: CtrlStateCore<P>,
    D: 'static,
{
    pub fn drain_tls(&mut self) -> Result<P::Acc, String> {
        if Arc::strong_count(&self.control.state) > 0 {
            Err("DrainTlsError::NoThreadLocalsUsed".to_owned())
        } else {
            let acc = self.control.take_acc((self.acc_zero)());
            Ok(acc)
        }
    }
}

impl<P> ControlSendG<P, NoNode>
where
    P: CoreParam + GDataParam + CtrlStateParam + 'static + SentDataParam + UseCtrlStateGDefault,
    P::Dat: 'static,
    P::GData: GuardedData<P::Dat, Arg = P::Dat> + 'static,
    P::CtrlState: CtrlStateCore<P>,
{
    pub fn send_data(&self, sent_data: P::SDat) {
        self.control
            .with_data_mut(|data| (self.op)(sent_data, data, thread::current().id()))
    }
}
impl<P> ControlSendG<P, NoNode>
where
    P: CoreParam + GDataParam + CtrlStateParam + 'static + SentDataParam + UseCtrlStateGDefault,
    P::Dat: 'static,
    P::GData: GuardedData<P::Dat, Arg = P::Dat> + 'static,
    P::CtrlState: CtrlStateCore<P> + New<P::CtrlState, Arg = P::Acc>,
{
    pub(crate) fn new_priv(
        tl: &'static LocalKey<HolderG<P, NoNode>>,
        acc_zero: impl Fn() -> P::Acc + 'static + Send + Sync,
        op: impl Fn(P::SDat, &mut P::Dat, ThreadId) + 'static + Send + Sync,
        op_inner: impl Fn(P::Dat, &mut P::Acc, ThreadId) + 'static + Send + Sync,
        op_r: impl Fn(P::Dat, P::Acc) -> P::Acc + 'static + Send + Sync,
    ) -> Self {
        Self {
            control: ControlG::new(tl, acc_zero(), op_inner),
            acc_zero: Arc::new(acc_zero),
            op: Arc::new(op),
            op_r: Arc::new(op_r),
        }
    }
}
