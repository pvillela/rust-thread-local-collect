use std::thread::ThreadId;

pub(crate) mod nosend {
    use super::*;

    pub type Data = i32;

    pub type AccValue = i32;

    pub fn op(data: Data, acc: &mut AccValue, _tid: ThreadId) {
        *acc += data;
    }
}

pub(crate) mod send {
    use super::*;

    pub type Data = i32;

    pub type AccValue = i32;

    pub fn op(data: Data, acc: &mut AccValue, _tid: ThreadId) {
        *acc += data;
    }

    pub fn op_r(acc1: AccValue, acc2: AccValue) -> AccValue {
        acc1 + acc2
    }
}
