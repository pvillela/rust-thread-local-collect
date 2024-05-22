use std::{collections::HashMap, thread::ThreadId};

#[derive(Debug, Clone)]
pub struct Foo(pub String);

pub(crate) mod nosend {
    pub use super::Foo;
    use super::*;

    pub type Data = HashMap<i32, Foo>;

    pub type AccValue = HashMap<ThreadId, HashMap<i32, Foo>>;

    pub fn op(data: Data, acc: &mut AccValue, tid: ThreadId) {
        acc.entry(tid).or_default();
        for (k, v) in data {
            acc.get_mut(&tid).unwrap().insert(k, v.clone());
        }
    }
}

pub(crate) mod send {
    pub use super::Foo;
    use super::*;

    pub type Data = (i32, Foo);

    pub type AccValue = HashMap<ThreadId, HashMap<i32, Foo>>;

    pub fn op(data: Data, acc: &mut AccValue, tid: ThreadId) {
        acc.entry(tid).or_default();
        let (k, v) = data;
        acc.get_mut(&tid).unwrap().insert(k, v.clone());
    }

    pub fn op_r(acc1: AccValue, acc2: AccValue) -> AccValue {
        let mut acc = acc1;
        acc2.into_iter().for_each(|(k, v)| {
            acc.insert(k, v);
        });
        acc
    }
}
