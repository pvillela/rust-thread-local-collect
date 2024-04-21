use criterion::black_box;
use std::{fmt::Debug, ops::Deref, thread};

pub const NTHREADS: u32 = 100;
pub const NENTRIES: u32 = 10;

pub trait BenchTarget<T, U> {
    fn add_value(&self, t_idx: u32, i_idx: u32);
    fn acc(&mut self) -> impl Deref<Target = U>;
}

pub fn bench<T, U: Debug>(mut target: impl BenchTarget<T, U> + Sync) {
    let mut bench = move || {
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
            black_box(&acc);
        }
    };
    bench()
}
