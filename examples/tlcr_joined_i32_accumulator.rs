//! Simple example usage of [`thread_local_collect::tlcr::joined`].

use std::thread::{self, ThreadId};
use thread_local_collect::tlcr::joined::Control;

// Define your data type, e.g.:
type Data = i32;

// Define your accumulated value type.
type AccValue = i32;

// Define your accumulation operation.
fn op(data: Data, acc: &mut AccValue, _: ThreadId) {
    *acc += data;
}

// Define your accumulor reduction operation.
fn op_r(acc1: AccValue, acc2: AccValue) -> AccValue {
    acc1 + acc2
}

const NTHREADS: i32 = 5;

fn main() {
    let mut control = Control::new(|| 0, op, op_r);

    thread::scope(|s| {
        let hs = (0..NTHREADS)
            .map(|i| {
                let control = control.clone();
                s.spawn({
                    move || {
                        control.send_data(i);
                    }
                })
            })
            .collect::<Vec<_>>();

        hs.into_iter().for_each(|h| h.join().unwrap());
    });

    // Drain thread-locals.
    let acc = control.drain_tls().unwrap();

    // Print the accumulated value
    println!("accumulated={acc}");
}
