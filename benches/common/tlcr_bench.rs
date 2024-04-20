//! Benchmark for [`thread_local_collect::tlcr`].

use super::{bench_fn, BenchTarget};
use criterion::black_box;
use std::{collections::HashMap, fmt::Debug, ops::Deref, thread::ThreadId};
use thread_local_collect::tlcr::Control;

#[derive(Debug, Clone)]
struct Foo(String);

type Data = (u32, Foo);

type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

fn op(data: Data, acc: &mut AccValue, tid: ThreadId) {
    acc.entry(tid).or_default();
    let (k, v) = data;
    acc.get_mut(&tid).unwrap().insert(k, v.clone());
}

fn op_r(acc1: AccValue, acc2: AccValue) -> AccValue {
    let mut acc = acc1;
    acc2.into_iter().for_each(|(k, v)| {
        acc.insert(k, v);
    });
    acc
}

struct BenchStruct(Control<Data, AccValue>);

struct Wrap<S>(S);

impl<S> Deref for Wrap<S> {
    type Target = S;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl BenchTarget<Data, AccValue> for BenchStruct {
    fn add_value(&self, t_idx: u32, i_idx: u32) {
        let si = black_box(t_idx.to_string());
        self.0.send_data((i_idx, Foo("a".to_owned() + &si)));
    }

    fn acc(&mut self) -> impl Deref<Target = AccValue> {
        Wrap(self.0.drain_tls().unwrap())
    }
}

pub fn tlcr_bench() {
    let control = Control::new(HashMap::new, op, op_r);
    let target = BenchStruct(control);
    let mut bench_fn = bench_fn(target);
    bench_fn();
}
