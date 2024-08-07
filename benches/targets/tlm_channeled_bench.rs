//! Benchmark for [`thread_local_collect::tlm::channeled`].

use super::{bench, BenchTarget, NENTRIES, NTHREADS};
use std::{collections::HashMap, fmt::Debug, ops::Deref, thread::ThreadId};
use thread_local_collect::tlm::channeled::{Control, Holder};

mod map_bench {
    pub use super::super::map_data::send::{op, AccValue, Data, Foo};
    use super::*;

    thread_local! {
        static MY_TL: Holder<Data> = Holder::new();
    }

    impl BenchTarget<Data, AccValue> for Control<Data, AccValue> {
        fn add_value(&self, t_idx: i32, i_idx: i32) {
            let sti = t_idx.to_string();
            self.send_data((i_idx, Foo("a".to_owned() + &sti)));
        }

        fn acc(&mut self) -> impl Deref<Target = AccValue> {
            self.drain_tls();
            let acc = Self::acc(self);
            assert!(acc.len() == NTHREADS as usize);
            acc
        }
    }

    pub(super) fn control() -> Control<Data, AccValue> {
        Control::new(&MY_TL, HashMap::new(), op)
    }
}

mod i32_bench {
    pub use super::super::i32_data::send::{op, AccValue, Data};
    use super::*;

    thread_local! {
        static MY_TL: Holder<Data> = Holder::new();
    }

    impl BenchTarget<Data, AccValue> for Control<Data, AccValue> {
        fn add_value(&self, t_idx: i32, i_idx: i32) {
            self.send_data(t_idx * i_idx);
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

    pub(super) fn control() -> Control<Data, AccValue> {
        Control::new(&MY_TL, 0, op)
    }
}

pub fn tlm_channeled_nobg_map_bench() {
    use map_bench::*;
    bench(control());
}

pub fn tlm_channeled_bg_map_bench() {
    use map_bench::*;
    let control = control();
    control.start_receiving_tls().unwrap(); // this significantly slows thigs down
    bench(control);
}

pub fn tlm_channeled_nobg_i32_bench() {
    use i32_bench::*;
    bench(control());
}

pub fn tlm_channeled_bg_i32_bench() {
    use i32_bench::*;
    let control = control();
    control.start_receiving_tls().unwrap(); // this significantly slows thigs down
    bench(control);
}
