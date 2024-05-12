use super::common::{
    ControlG, CoreParam, CtrlStateCore, CtrlStateParam, CtrlStateWithNode, DefaultDiscr,
    GDataParam, GuardedData, Hldr, HldrParam, HolderG, New, NodeParam, WithNode,
};
use std::{
    fmt::Debug,
    mem::take,
    sync::Arc,
    thread::{self, LocalKey, ThreadId},
};
use thiserror::Error;

/// Errors returned by [`ControlSendG::drain_tls`].
#[derive(Error, Debug)]
#[error("there is nothing accumulated")]
pub struct NothingAccumulatedError;

pub struct ControlSendG<P, D, T, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,
    P::Hldr: Hldr,

    P: 'static,
{
    control: ControlG<P, D>,
    /// Operation that combines the stored thead-local value with data sent from threads.
    #[allow(clippy::type_complexity)]
    op: Arc<dyn Fn(T, &mut U, ThreadId) + Send + Sync>,
}

impl<P, D, T, U> ControlSendG<P, D, T, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,
    P::Hldr: Hldr,

    P::CtrlState: CtrlStateCore<P> + New<P::CtrlState, Arg = P::Acc>,
{
    pub fn new(
        tl: &'static LocalKey<P::Hldr>,
        // Produces a zero value of type `P::Dat`, which is needed to obtain consistent aggregation results.
        acc_zero: fn() -> U,
        op: impl Fn(T, &mut U, ThreadId) + 'static + Send + Sync,
        op_r: impl Fn(U, U) -> U + 'static + Send + Sync,
    ) -> Self {
        Self {
            control: ControlG::new(
                tl,
                Some(acc_zero()),
                acc_zero,
                move |data: U, acc: &mut Option<U>, _| {
                    let acc0 = take(acc);
                    // By construction above and assignment below, `acc0` will never be `None`.
                    if let Some(acc0) = acc0 {
                        *acc = Some(op_r(data, acc0));
                    }
                },
            ),
            op: Arc::new(op),
        }
    }
}

pub trait WithTakeTls<P, D, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,
    P::Hldr: Hldr,
{
    fn take_tls(control: &ControlG<P, D>);
}

impl<P, D, T, U> ControlSendG<P, D, T, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,
    P::Hldr: Hldr,

    P::CtrlState: CtrlStateCore<P>,
    Self: WithTakeTls<P, D, U>,
{
    pub fn drain_tls(&mut self) -> Result<U, NothingAccumulatedError> {
        Self::take_tls(&self.control);
        let acc = self.control.take_acc(None);
        acc.ok_or(NothingAccumulatedError)
    }
}

impl<P, T, U> ControlSendG<P, DefaultDiscr, T, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U>
        + CtrlStateParam
        + HldrParam<Hldr = HolderG<P, DefaultDiscr>>,

    P: GDataParam,
    P::CtrlState: CtrlStateCore<P>,
    P::GData: GuardedData<U, Arg = Option<U>>,
    U: 'static,
{
    pub fn send_data(&self, sent_data: T) {
        self.control
            .with_data_mut(|data| (self.op)(sent_data, data, thread::current().id()))
    }
}

impl<P, T, U> ControlSendG<P, WithNode, T, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U>
        + CtrlStateParam
        + HldrParam<Hldr = HolderG<P, WithNode>>,

    P: GDataParam,
    P: NodeParam<NodeFnArg = ControlG<P, WithNode>>,
    P::CtrlState: CtrlStateWithNode<P>,
    P::GData: GuardedData<U, Arg = Option<U>>,
    U: 'static,
{
    pub fn send_data(&self, sent_data: T) {
        self.control
            .with_data_mut(|data| (self.op)(sent_data, data, thread::current().id()))
    }
}

impl<P, D, T, U> Clone for ControlSendG<P, D, T, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,
    P::Hldr: Hldr,
{
    fn clone(&self) -> Self {
        Self {
            control: self.control.clone(),
            op: self.op.clone(),
        }
    }
}

impl<P, D, T, U> Debug for ControlSendG<P, D, T, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,
    P::Hldr: Hldr,

    P::CtrlState: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("ControlSend({:?})", self.control))
    }
}

/// Comment out the manual clone implementation above and see what happens below.
#[allow(unused)]
fn demonstrate_need_for_manual_clone<P, D, T, U>(
    x: ControlSendG<P, D, T, U>,
    y: &ControlSendG<P, D, T, U>,
) where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,
    P::Hldr: Hldr,
{
    let x1: ControlSendG<P, D, T, U> = x.clone();
    let y1: ControlSendG<P, D, T, U> = y.clone();
}
