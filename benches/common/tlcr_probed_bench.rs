//! Benchmark for [`thread_local_collect::tlcr::joined`].

use super::{bench, BenchTarget, NENTRIES, NTHREADS};
use criterion::black_box;
use std::{collections::HashMap, fmt::Debug, ops::Deref, thread::ThreadId};
use thread_local_collect::tlcr::probed::Control;

mod map_bench {
    use super::*;

    #[derive(Debug, Clone)]
    pub(super) struct Foo(String);

    type Data = (u32, Foo);

    type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

    pub(super) fn op(data: Data, acc: &mut AccValue, tid: ThreadId) {
        acc.entry(tid).or_default();
        let (k, v) = data;
        acc.get_mut(&tid).unwrap().insert(k, v.clone());
    }

    pub(super) fn op_r(acc1: AccValue, acc2: AccValue) -> AccValue {
        let mut acc = acc1;
        acc2.into_iter().for_each(|(k, v)| {
            acc.insert(k, v);
        });
        acc
    }

    struct Wrap<S>(S);

    impl<S> Deref for Wrap<S> {
        type Target = S;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl BenchTarget<Data, AccValue> for Control<Data, AccValue> {
        fn add_value(&self, t_idx: u32, i_idx: u32) {
            let si = black_box(t_idx.to_string());
            self.send_data((i_idx, Foo("a".to_owned() + &si)));
        }

        fn acc(&mut self) -> impl Deref<Target = AccValue> {
            let acc = self.drain_tls().unwrap();
            assert!(acc.len() == NTHREADS as usize);
            Wrap(acc)
        }
    }
}

mod u32_bench {
    use super::*;

    type Data = u32;

    type AccValue = u32;

    pub(super) fn op(data: Data, acc: &mut AccValue, _tid: ThreadId) {
        *acc += data;
    }

    pub(super) fn op_r(acc1: AccValue, acc2: AccValue) -> AccValue {
        acc1 + acc2
    }

    struct Wrap<S>(S);

    impl<S> Deref for Wrap<S> {
        type Target = S;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl BenchTarget<Data, AccValue> for Control<Data, AccValue> {
        fn add_value(&self, t_idx: u32, i_idx: u32) {
            self.send_data(t_idx * i_idx);
        }

        fn acc(&mut self) -> impl Deref<Target = AccValue> {
            let acc = self.drain_tls().unwrap();
            assert_eq!(
                acc,
                NTHREADS * (NTHREADS - 1) / 2 * NENTRIES * (NENTRIES - 1) / 2
            );
            Wrap(acc)
        }
    }
}

pub fn tlcr_probed_map_bench() {
    use map_bench::*;
    let control = Control::new(HashMap::new, op, op_r);
    bench(control);
}

pub fn tlcr_probed_u32_bench() {
    use u32_bench::*;
    let control = Control::new(|| 0, op, op_r);
    bench(control);
}
