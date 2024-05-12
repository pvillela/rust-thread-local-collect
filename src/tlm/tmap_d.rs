use std::{collections::HashMap, thread::ThreadId};

use super::common::*;

//=================
// Control sub-state struct with a thread map.

/// Type used by the specialization of [`ControlG`] for module [`super::probed`].
//  Holds the thread map used by the [`ControlG`] specializations for module [`super::probed`].
//  Used also for module [`super::joined_old`] which shows a previous more complex implementation of module
//  [`super::joined`].
//  Also used to partially discriminate common [`ControlG`] functionality used by those modules.
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
