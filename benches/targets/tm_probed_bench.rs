//! Benchmark for [`thread_local_collect::tm::probed`].

use super::{bench, BenchTarget, NENTRIES, NTHREADS};
use std::{collections::HashMap, fmt::Debug, ops::Deref, thread::ThreadId};
use thread_local_collect::tm::probed::Control;

mod map_bench {
    pub use super::super::map_data::nosend::{op, AccValue, Data, Foo};
    use super::*;

    fn insert_tl_entry(k: i32, v: Foo, control: &Control<Data, AccValue>) {
        control.with_data_mut(|data| {
            data.insert(k, v);
        });
    }

    impl BenchTarget<Data, AccValue> for Control<Data, AccValue> {
        fn add_value(&self, t_idx: i32, i_idx: i32) {
            let sti = t_idx.to_string();
            insert_tl_entry(i_idx, Foo("a".to_owned() + &sti), self);
        }

        fn acc(&mut self) -> impl Deref<Target = AccValue> {
            self.take_tls(); // must call this to populate `acc`
            let acc = Self::acc(self);
            assert_eq!(acc.len(), NTHREADS as usize);
            acc
        }
    }

    pub(super) fn control() -> Control<Data, AccValue> {
        Control::new(HashMap::new(), HashMap::new, op)
    }
}

mod i32_bench {
    pub use super::super::i32_data::nosend::{op, AccValue, Data};
    use super::*;

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
            self.take_tls(); // must call this to populate `acc`
            let acc = Self::acc(self);
            assert_eq!(
                *acc,
                NTHREADS * (NTHREADS - 1) / 2 * NENTRIES * (NENTRIES - 1) / 2
            );
            acc
        }
    }

    pub(super) fn control() -> Control<Data, AccValue> {
        Control::new(0, || 0, op)
    }
}

pub fn tm_probed_map_bench() {
    use map_bench::*;
    bench(control());
}

pub fn tm_probed_i32_bench() {
    use i32_bench::*;
    bench(control());
}
