//! Provides a wrapper for [`crate::tlm::common::ControlG`] to support a `send_data` and `drain_tls` API
//! similar to that of [`crate::tlcr`] submodules.

use super::super::common::{
    ControlG, CoreParam, CtrlParam, CtrlStateCore, CtrlStateParam, HldrData, HldrLink, HldrParam,
    New,
};
use std::{
    fmt::Debug,
    mem::take,
    thread::{self, LocalKey, ThreadId},
};

/// Wrapper of [`crate::tlm::common::ControlG`] that provides  a`send_data` and `drain_tls` API
/// similar to that of [`crate::tlcr`] submodules. Used to implement [`super::joined::Control`],
/// [`super::probed::Control`], and [`super::simple_joined::Control`].
pub struct ControlSendG<P, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,

    P: 'static,
{
    /// Inner control object.
    pub(super) control: ControlG<P>,
    /// Produces a zero value of type `P::Dat`, which is needed to obtain consistent aggregation results.
    acc_zero: fn() -> U,
}

impl<P, U> ControlSendG<P, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,

    P::CtrlState: CtrlStateCore<P> + New<P::CtrlState, Arg = P::Acc>,
{
    /// Instantiates a [`ControlSendG`] object.
    ///
    /// - `tl` - reference to thread-local static.
    /// - `acc_zero` - produces a zero value of type `U`, which is needed to obtain consistent aggregation results.
    /// - `op` - operation that combines the accumulated value with data sent from threads.
    /// - `op_r` - binary operation that reduces two accumulated values into one.
    pub fn new(
        tl: &'static LocalKey<P::Hldr>,
        acc_zero: fn() -> U,
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
        }
    }
}

#[doc(hidden)]
/// Defines `take_tls` interface provided by different [`ControlSendG`] specializations.
pub trait WithTakeTls<P, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,
{
    /// Ensures thread-local values have been collected and aggregated.
    fn take_tls(control: &ControlG<P>);
}

impl<P, U> ControlSendG<P, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,

    Self: WithTakeTls<P, U>,
    P::CtrlState: CtrlStateCore<P>,
{
    /// Returns the accumulation of the thread-local values, replacing the state of `self` with
    /// the zero value of the accumulator.
    pub fn drain_tls(&mut self) -> U {
        Self::take_tls(&self.control);
        let acc = self.control.take_acc(Some((self.acc_zero)()));
        acc.expect("accumulator is never None")
    }
}

impl<P, U> ControlSendG<P, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,

    P::CtrlState: CtrlStateCore<P>,
    P: CtrlParam<Ctrl = ControlG<P>>,
    P::Hldr: HldrLink<P> + HldrData<P>,
{
    /// Called from a thread to access the thread's local accumulated value.
    pub fn with_local_acc<V>(&self, f: impl FnOnce(&U) -> V) -> V {
        self.control.with_data(f)
    }

    /// Called from a thread to mutably access the thread's local accumulated value.
    pub fn with_local_acc_mut<V>(&self, f: impl FnOnce(&mut U) -> V) -> V {
        self.control.with_data_mut(f)
    }

    /// Called from a thread to aggregate data with aggregation operation `op`.
    pub fn send_data<T>(&self, data: T, op: impl FnOnce(T, &mut U, ThreadId)) {
        self.with_local_acc_mut(|acc| op(data, acc, thread::current().id()))
    }
}

impl<P, U> Clone for ControlSendG<P, U>
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,
{
    fn clone(&self) -> Self {
        Self {
            control: self.control.clone(),
            acc_zero: self.acc_zero,
        }
    }
}

impl<P, U> Debug for ControlSendG<P, U>
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
fn demonstrate_need_for_manual_clone<P, U>(x: ControlSendG<P, U>, y: &ControlSendG<P, U>)
where
    P: CoreParam<Acc = Option<U>, Dat = U> + CtrlStateParam + HldrParam,
{
    let x1: ControlSendG<P, U> = x.clone();
    let y1: ControlSendG<P, U> = y.clone();
}
