use criterion::black_box;
use std::{ops::Deref, thread};

pub fn fibonacci(n: u64) -> u64 {
    match n {
        0 => 1,
        1 => 1,
        n => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

pub const NTHREADS: u32 = 100;
pub const NENTRIES: u32 = 10;

pub trait BenchTarget<T, U> {
    fn add_value(&self, t_idx: u32, i_idx: u32);
    fn acc(&mut self) -> impl Deref<Target = U>;
}

pub fn bench_fn<T, U>(mut target: impl BenchTarget<T, U> + Sync) -> impl FnMut() {
    move || {
        {
            let target = &target;
            thread::scope(|s| {
                let hs = (0..NTHREADS)
                    .map(|i| {
                        s.spawn({
                            move || {
                                for j in 0..NENTRIES {
                                    target.add_value(i, j);
                                }
                            }
                        })
                    })
                    .collect::<Vec<_>>();

                hs.into_iter().for_each(|h| h.join().unwrap());
            });
        }

        {
            let acc = target.acc();
            black_box(acc);
        }
    }
}
