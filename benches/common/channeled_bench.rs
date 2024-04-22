//! Benchmark for [`thread_local_collect::simple_joined`].

use super::{bench, BenchTarget, NENTRIES, NTHREADS};
use criterion::black_box;
use std::{collections::HashMap, fmt::Debug, ops::Deref, thread::ThreadId};
use thread_local_collect::channeled::{Control, Holder, HolderLocalKey};

mod map_bench {
    use super::*;

    #[derive(Debug, Clone)]
    pub(crate) struct Foo(String);

    type Data = (u32, Foo);

    type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

    thread_local! {
        static MY_TL: Holder<Data> = Holder::new();
    }

    fn send_tl_data(k: u32, v: Foo, control: &Control<Data, AccValue>) {
        MY_TL.ensure_linked(control);
        MY_TL.send_data((k, v)).unwrap();
    }

    pub(super) fn op(data: Data, acc: &mut AccValue, tid: ThreadId) {
        acc.entry(tid).or_default();
        let (k, v) = data;
        acc.get_mut(&tid).unwrap().insert(k, v.clone());
    }

    impl BenchTarget<Data, AccValue> for Control<Data, AccValue> {
        fn add_value(&self, t_idx: u32, i_idx: u32) {
            let si = black_box(t_idx.to_string());
            send_tl_data(i_idx, Foo("a".to_owned() + &si), self);
        }

        fn acc(&mut self) -> impl Deref<Target = AccValue> {
            self.drain_tls();
            let acc = Self::acc(self);
            assert!(acc.len() == NTHREADS as usize);
            acc
        }
    }
}

mod u32_bench {
    use super::*;

    type Data = u32;

    type AccValue = u32;

    thread_local! {
        static MY_TL: Holder<Data> = Holder::new();
    }

    fn send_tl_data(value: u32, control: &Control<Data, AccValue>) {
        MY_TL.ensure_linked(control);
        MY_TL.send_data(value).unwrap();
    }

    pub(super) fn op(data: Data, acc: &mut AccValue, _tid: ThreadId) {
        *acc += data;
    }

    impl BenchTarget<Data, AccValue> for Control<Data, AccValue> {
        fn add_value(&self, t_idx: u32, i_idx: u32) {
            send_tl_data(t_idx * i_idx, self);
        }

        fn acc(&mut self) -> impl Deref<Target = AccValue> {
            self.drain_tls();
            let acc = Self::acc(self);
            assert_eq!(
                *acc,
                NTHREADS * (NTHREADS - 1) / 2 * NENTRIES * (NENTRIES - 1) / 2
            );
            acc
        }
    }
}

pub fn channeled_nobg_map_bench() {
    use map_bench::*;
    let control = Control::new(HashMap::new(), op);
    bench(control);
}

pub fn channeled_bg_map_bench() {
    use map_bench::*;
    let control = Control::new(HashMap::new(), op);
    control.start_receiving_tls().unwrap(); // this significantly slows thigs down
    bench(control);
}

pub fn channeled_nobg_u32_bench() {
    use u32_bench::*;
    let control = Control::new(0, op);
    bench(control);
}

pub fn channeled_bg_u32_bench() {
    use u32_bench::*;
    let control = Control::new(0, op);
    control.start_receiving_tls().unwrap(); // this significantly slows thigs down
    bench(control);
}
