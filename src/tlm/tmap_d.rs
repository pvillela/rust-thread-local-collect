//! Defines a struct containing a map from thread IDs to thread-local values for use as
//! the sub-state of [`ControlG`]'s state.

use std::{collections::HashMap, thread::ThreadId};

use super::common::*;

//=================
// Control sub-state struct with a thread map.

/// Struct containing a map from thread IDs to thread-local values, used by the specialization of
/// [`CtrlStateG`] for module [`super::probed`].
/// Also used as the `D` discriminant parameter for [`CtrlStateG`] impls.
#[derive(Debug)]
pub struct TmapD<P>
where
    P: NodeParam,
{
    pub(crate) tmap: HashMap<ThreadId, P::Node>,
}

impl<P> New<Self> for TmapD<P>
where
    P: NodeParam,

    P: CoreParam,
{
    type Arg = ();

    fn new(_: ()) -> Self {
        Self {
            tmap: HashMap::new(),
        }
    }
}

impl<P> CtrlStateCore<P> for CtrlStateG<P, TmapD<P>>
where
    P: NodeParam,

    P: CoreParam + SubStateParam<SubState = TmapD<P>>,
{
    fn tl_data_dropped(
        &mut self,
        op: &(dyn Fn(P::Dat, &mut P::Acc, ThreadId) + Send + Sync),
        data: P::Dat,
        tid: ThreadId,
    ) {
        self.s.tmap.remove(&tid);
        let acc = self.acc_mut_priv();
        op(data, acc, tid);
    }
}

impl<P> CtrlStateWithNode<P> for CtrlStateG<P, TmapD<P>>
where
    P: NodeParam,

    P: CoreParam + SubStateParam<SubState = TmapD<P>>,
{
    fn register_node(&mut self, node: <P as NodeParam>::Node, tid: ThreadId) {
        self.s.tmap.insert(tid, node);
    }
}
