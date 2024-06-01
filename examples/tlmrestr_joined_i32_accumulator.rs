//! Simple example usage of [`thread_local_collect::tlm::restr::joined`].

use std::thread::{self, ThreadId};
use thread_local_collect::tlm::restr::joined::{Control, Holder};

// Define your data type, e.g.:
type Data = i32;

// Define your accumulated value type.
type AccValue = i32;

// Define your zero accumulated value function.
fn acc_zero() -> AccValue {
    0
}

// Define your accumulation operation.
fn op(data: Data, acc: &mut AccValue, _: ThreadId) {
    *acc += data;
}

// Define your accumulor reduction operation.
fn op_r(acc1: AccValue, acc2: AccValue) -> AccValue {
    acc1 + acc2
}

thread_local! {
    static MY_TL: Holder<AccValue> = Holder::new();
}

const NTHREADS: i32 = 5;

fn main() {
    // Instantiate the control object.
    let mut control = Control::new(&MY_TL, acc_zero, op_r);

    // Send data to control from main thread if desired.
    control.aggregate_data(100, op);

    let hs = (0..NTHREADS)
        .map(|i| {
            // Clone control for use in the new thread.
            let control = control.clone();
            thread::spawn({
                move || {
                    // Send data from thread to control object.
                    control.aggregate_data(i, op);
                }
            })
        })
        .collect::<Vec<_>>();

    // Join all threads.
    hs.into_iter().for_each(|h| h.join().unwrap());

    // Drain thread-local values.
    let acc = control.drain_tls();

    // Print the accumulated value
    println!("accumulated={acc}");
}
