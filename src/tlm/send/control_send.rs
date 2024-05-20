use super::super::common::{
    ControlG, CoreParam, CtrlParam, CtrlStateCore, CtrlStateParam, HldrData, HldrLink, HldrParam,
    New,
};
use std::{
    fmt::Debug,
    mem::take,
    sync::Arc,
    thread::{self, LocalKey, ThreadId},
};

pub struct ControlSendG<P, T, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,

    P: 'static,
{
    pub(super) control: ControlG<P>,
    /// Operation that combines the stored thead-local value with data sent from threads.
    #[allow(clippy::type_complexity)]
    op: Arc<dyn Fn(T, &mut U, ThreadId) + Send + Sync>,
    /// Produces a zero value of type `P::Dat`, which is needed to obtain consistent aggregation results.
    acc_zero: fn() -> U,
}

impl<P, T, U> ControlSendG<P, T, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,

    P::CtrlState: CtrlStateCore<P> + New<P::CtrlState, Arg = P::Acc>,
{
    pub fn new(
        tl: &'static LocalKey<P::Hldr>,
        // Produces a zero value of type `P::Dat`, which is needed to obtain consistent aggregation results.
        acc_zero: fn() -> U,
        // Operation that combines the stored thead-local value with data sent from threads.
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
                    // By the construction of acc_base above and assignment below, `acc0` will never be `None`.
                    if let Some(acc0) = acc0 {
                        *acc = Some(op_r(data, acc0));
                    }
                },
            ),
            acc_zero,
            op: Arc::new(op),
        }
    }
}

pub trait WithTakeTls<P, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,
{
    fn take_tls(control: &ControlG<P>);
}

impl<P, T, U> ControlSendG<P, T, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,

    Self: WithTakeTls<P, U>,
    P::CtrlState: CtrlStateCore<P>,
{
    pub fn drain_tls(&mut self) -> U {
        Self::take_tls(&self.control);
        let acc = self.control.take_acc(Some((self.acc_zero)()));
        acc.expect("accumulator is never None")
    }
}

impl<P, T, U> ControlSendG<P, T, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,

    P::CtrlState: CtrlStateCore<P>,
    P: CtrlParam<Ctrl = ControlG<P>>,
    P::Hldr: HldrLink<P> + HldrData<P>,
{
    pub fn send_data(&self, sent_data: T) {
        self.control
            .with_data_mut(|data| (self.op)(sent_data, data, thread::current().id()))
    }
}

impl<P, T, U> Clone for ControlSendG<P, T, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,
{
    fn clone(&self) -> Self {
        Self {
            control: self.control.clone(),
            op: self.op.clone(),
            acc_zero: self.acc_zero,
        }
    }
}

impl<P, T, U> Debug for ControlSendG<P, T, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,

    P::CtrlState: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("ControlSend({:?})", self.control))
    }
}

/// Comment out the manual Clone implementation above, deribe Clone, and see what happens below.
#[allow(unused)]
fn demonstrate_need_for_manual_clone<P, T, U>(x: ControlSendG<P, T, U>, y: &ControlSendG<P, T, U>)
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,
{
    let x1: ControlSendG<P, T, U> = x.clone();
    let y1: ControlSendG<P, T, U> = y.clone();
}
