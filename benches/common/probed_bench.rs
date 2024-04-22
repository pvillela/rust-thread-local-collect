//! Benchmark for [`thread_local_collect::simple_joined`].

use super::{bench, BenchTarget, NTHREADS};
use criterion::black_box;
use std::{collections::HashMap, fmt::Debug, ops::Deref, thread::ThreadId};
use thread_local_collect::probed::{Control, Holder, HolderLocalKey};

#[derive(Debug, Clone)]
struct Foo(String);

type Data = HashMap<u32, Foo>;

type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

thread_local! {
    static MY_FOO_MAP: Holder<Data, AccValue> = Holder::new(HashMap::new);
}

fn insert_tl_entry(k: u32, v: Foo, control: &Control<Data, AccValue>) {
    MY_FOO_MAP.ensure_linked(control);
    MY_FOO_MAP
        .with_data_mut(|data| {
            data.insert(k, v);
        })
        .unwrap();
}

fn op(data: Data, acc: &mut AccValue, tid: ThreadId) {
    acc.entry(tid).or_default();
    for (k, v) in data {
        acc.get_mut(&tid).unwrap().insert(k, v.clone());
    }
}

struct BenchStruct(Control<Data, AccValue>);

impl BenchTarget<Data, AccValue> for BenchStruct {
    fn add_value(&self, t_idx: u32, i_idx: u32) {
        let si = black_box(t_idx.to_string());
        insert_tl_entry(i_idx, Foo("a".to_owned() + &si), &self.0);
    }

    fn acc(&mut self) -> impl Deref<Target = AccValue> {
        let acc = self.0.acc();
        assert!(acc.len() == NTHREADS as usize);
        acc
    }
}

pub fn probed_bench() {
    let control = Control::new(HashMap::new(), op);
    let target = BenchStruct(control);
    bench(target);
}
