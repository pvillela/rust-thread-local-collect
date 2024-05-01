//! Benchmark for [`thread_local_collect::joined`].

use super::{bench, BenchTarget, NENTRIES, NTHREADS};
use criterion::black_box;
use std::{collections::HashMap, fmt::Debug, ops::Deref, thread::ThreadId};
use thread_local_collect::joined::{Control, Holder};

mod map_bench {
    use super::*;

    #[derive(Debug, Clone)]
    pub struct Foo(String);

    type Data = HashMap<u32, Foo>;

    type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

    thread_local! {
        static MY_TL: Holder<Data, AccValue> = Holder::new(HashMap::new);
    }

    fn insert_tl_entry(k: u32, v: Foo, control: &Control<Data, AccValue>) {
        control.with_data_mut(|data| {
            data.insert(k, v);
        });
    }

    pub fn op(data: Data, acc: &mut AccValue, tid: ThreadId) {
        acc.entry(tid).or_default();
        for (k, v) in data {
            acc.get_mut(&tid).unwrap().insert(k, v.clone());
        }
    }

    impl BenchTarget<Data, AccValue> for Control<Data, AccValue> {
        fn add_value(&self, t_idx: u32, i_idx: u32) {
            let si = black_box(t_idx.to_string());
            insert_tl_entry(i_idx, Foo("a".to_owned() + &si), self);
        }

        fn acc(&mut self) -> impl Deref<Target = AccValue> {
            let acc = Self::acc(self);
            assert!(acc.len() == NTHREADS as usize);
            acc
        }
    }

    pub(super) fn control() -> Control<Data, AccValue> {
        Control::new(&MY_TL, HashMap::new(), op)
    }
}

mod u32_bench {
    use super::*;

    type Data = u32;

    type AccValue = u32;

    thread_local! {
        static MY_TL: Holder<Data, AccValue> = Holder::new(||0);
    }

    fn update_tl(value: Data, control: &Control<Data, AccValue>) {
        control.with_data_mut(|data| {
            *data += value;
        });
    }

    pub fn op(data: Data, acc: &mut AccValue, _tid: ThreadId) {
        *acc += data;
    }

    impl BenchTarget<Data, AccValue> for Control<Data, AccValue> {
        fn add_value(&self, t_idx: u32, i_idx: u32) {
            update_tl(t_idx * i_idx, self);
        }

        fn acc(&mut self) -> impl Deref<Target = AccValue> {
            let acc = Self::acc(self);
            assert_eq!(
                *acc,
                NTHREADS * (NTHREADS - 1) / 2 * NENTRIES * (NENTRIES - 1) / 2
            );
            acc
        }
    }

    pub(super) fn control() -> Control<Data, AccValue> {
        Control::new(&MY_TL, 0, op)
    }
}

pub fn joined_map_bench() {
    use map_bench::*;
    bench(control());
}

pub fn joined_u32_bench() {
    use u32_bench::*;
    bench(control());
}
