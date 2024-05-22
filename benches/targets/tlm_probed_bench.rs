//! Benchmark for [`thread_local_collect::tlm::probed`].

use super::{bench, BenchTarget, NENTRIES, NTHREADS};
use criterion::black_box;
use std::{collections::HashMap, fmt::Debug, ops::Deref, thread::ThreadId};
use thread_local_collect::tlm::probed::{Control, Holder};

mod map_bench {
    pub use super::super::map_data::nosend::{op, AccValue, Data, Foo};
    use super::*;

    thread_local! {
        static MY_TL: Holder<Data, AccValue> = Holder::new();
    }

    fn insert_tl_entry(k: i32, v: Foo, control: &Control<Data, AccValue>) {
        control.with_data_mut(|data| {
            data.insert(k, v);
        });
    }

    impl BenchTarget<Data, AccValue> for Control<Data, AccValue> {
        fn add_value(&self, t_idx: i32, i_idx: i32) {
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
        Control::new(&MY_TL, HashMap::new(), HashMap::new, op)
    }
}

mod i32_bench {
    pub use super::super::i32_data::nosend::{op, AccValue, Data};
    use super::*;

    thread_local! {
        static MY_TL: Holder<Data, AccValue> = Holder::new();
    }

    fn update_tl(value: Data, control: &Control<Data, AccValue>) {
        control.with_data_mut(|data| {
            *data += value;
        });
    }

    impl BenchTarget<Data, AccValue> for Control<Data, AccValue> {
        fn add_value(&self, t_idx: i32, i_idx: i32) {
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
        Control::new(&MY_TL, 0, || 0, op)
    }
}

pub fn tlm_probed_map_bench() {
    use map_bench::*;
    bench(control());
}

pub fn tlm_probed_i32_bench() {
    use i32_bench::*;
    bench(control());
}
