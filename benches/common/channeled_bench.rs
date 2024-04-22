//! Benchmark for [`thread_local_collect::simple_joined`].

use super::{bench, BenchTarget, NTHREADS};
use criterion::black_box;
use std::{collections::HashMap, fmt::Debug, ops::Deref, thread::ThreadId};
use thread_local_collect::channeled::{Control, Holder, HolderLocalKey};

#[derive(Debug, Clone)]
struct Foo(String);

type Data = (u32, Foo);

type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

thread_local! {
    static MY_TL: Holder<Data> = Holder::new();
}

fn send_tl_data(k: u32, v: Foo, control: &Control<Data, AccValue>) {
    MY_TL.ensure_linked(control);
    MY_TL.send_data((k, v)).unwrap();
}

fn op(data: Data, acc: &mut AccValue, tid: ThreadId) {
    acc.entry(tid).or_default();
    let (k, v) = data;
    acc.get_mut(&tid).unwrap().insert(k, v.clone());
}

struct BenchStruct(Control<Data, AccValue>);

impl BenchTarget<Data, AccValue> for BenchStruct {
    fn add_value(&self, t_idx: u32, i_idx: u32) {
        let si = black_box(t_idx.to_string());
        send_tl_data(i_idx, Foo("a".to_owned() + &si), &self.0);
    }

    fn acc(&mut self) -> impl Deref<Target = AccValue> {
        self.0.drain_tls();
        let acc = self.0.acc();
        assert!(acc.len() == NTHREADS as usize);
        acc
    }
}

pub fn channeled_bench() {
    let control = Control::new(HashMap::new(), op);
    // control.start_receiving_tls().unwrap(); // this significantly slows thigs down
    let target = BenchStruct(control);
    bench(target);
}
