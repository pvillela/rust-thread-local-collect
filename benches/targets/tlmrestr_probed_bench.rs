//! Benchmark for [`thread_local_collect::tlm::restr::probed`].

use super::{bench, BenchTarget, Wrap, NENTRIES, NTHREADS};
use std::{collections::HashMap, fmt::Debug, ops::Deref, thread::ThreadId};
use thread_local_collect::tlm::restr::probed::{Control, Holder};

mod map_bench {
    pub use super::super::map_data::send::{op, op_r, AccValue, Data, Foo};
    use super::*;

    thread_local! {
        static MY_TL: Holder<AccValue> = Holder::new();
    }

    impl BenchTarget<Data, AccValue> for Control<AccValue> {
        fn add_value(&self, t_idx: i32, i_idx: i32) {
            let sti = t_idx.to_string();
            self.aggregate_data((i_idx, Foo("a".to_owned() + &sti)), op);
        }

        fn acc(&mut self) -> impl Deref<Target = AccValue> {
            let acc = self.drain_tls();
            assert!(acc.len() == NTHREADS as usize);
            Wrap(acc)
        }
    }

    pub(super) fn control() -> Control<AccValue> {
        Control::new(&MY_TL, HashMap::new, op_r)
    }
}

mod i32_bench {
    pub use super::super::i32_data::send::{op, op_r, AccValue, Data};
    use super::*;

    thread_local! {
        static MY_TL: Holder<AccValue> = Holder::new();
    }

    impl BenchTarget<Data, AccValue> for Control<AccValue> {
        fn add_value(&self, t_idx: i32, i_idx: i32) {
            self.aggregate_data(t_idx * i_idx, op);
        }

        fn acc(&mut self) -> impl Deref<Target = AccValue> {
            let acc = self.drain_tls();
            assert_eq!(
                acc,
                NTHREADS * (NTHREADS - 1) / 2 * NENTRIES * (NENTRIES - 1) / 2
            );
            Wrap(acc)
        }
    }

    pub(super) fn control() -> Control<AccValue> {
        Control::new(&MY_TL, || 0, op_r)
    }
}

pub fn tlmrestr_probed_map_bench() {
    use map_bench::*;
    bench(control());
}

pub fn tlmrestr_probed_i32_bench() {
    use i32_bench::*;
    bench(control());
}
