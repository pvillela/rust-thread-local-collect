use criterion::black_box;
use std::{fmt::Debug, ops::Deref, thread};

pub const NTHREADS: i32 = 100;
pub const NENTRIES: i32 = 10;

pub trait BenchTarget<T, U> {
    fn add_value(&self, t_idx: i32, i_idx: i32);
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

pub struct Wrap<S>(pub S);

impl<S> Deref for Wrap<S> {
    type Target = S;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
