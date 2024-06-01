//! Benchmark for [`thread_local_collect::tlcr::joined`].

use super::{bench, BenchTarget, Wrap, NENTRIES, NTHREADS};
use criterion::black_box;
use std::{collections::HashMap, fmt::Debug, ops::Deref, thread::ThreadId};
use thread_local_collect::tlcr::joined::Control;

mod map_bench {
    pub use super::super::map_data::send::{op, op_r, AccValue, Data, Foo};
    use super::*;

    impl BenchTarget<Data, AccValue> for Control<AccValue> {
        fn add_value(&self, t_idx: i32, i_idx: i32) {
            let si = black_box(t_idx.to_string());
            self.send_data((i_idx, Foo("a".to_owned() + &si)), op);
        }

        fn acc(&mut self) -> impl Deref<Target = AccValue> {
            let acc = self.drain_tls().unwrap();
            assert!(acc.len() == NTHREADS as usize);
            Wrap(acc)
        }
    }
}

mod i32_bench {
    pub use super::super::i32_data::send::{op, op_r, AccValue, Data};
    use super::*;

    impl BenchTarget<Data, AccValue> for Control<AccValue> {
        fn add_value(&self, t_idx: i32, i_idx: i32) {
            self.send_data(t_idx * i_idx, op);
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

pub fn tlcr_joined_map_bench() {
    use map_bench::*;
    let control = Control::new(HashMap::new, op_r);
    bench(control);
}

pub fn tlcr_joined_i32_bench() {
    use i32_bench::*;
    let control = Control::new(|| 0, op_r);
    bench(control);
}
