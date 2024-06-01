//! Simple example usage of [`thread_local_collect::tlm::restr::probed`].

use std::{
    thread::{self, ThreadId},
    time::Duration,
};
use thread_local_collect::tlm::restr::probed::{Control, Holder};

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

fn main() {
    // Instantiate the control object.
    let mut control = Control::new(&MY_TL, acc_zero, op_r);

    // Send data to control from main thread if desired.
    control.send_data(1, op);

    let h = thread::spawn({
        // Clone control for use in the new thread.
        let control = control.clone();
        move || {
            control.send_data(10, op);
            thread::sleep(Duration::from_millis(10));
            control.send_data(20, op);
        }
    });

    // Wait for spawned thread to do some work.
    thread::sleep(Duration::from_millis(5));

    // Probe the thread-local values and get the accuulated value computed from
    // current thread-local values.
    let acc = control.probe_tls();
    println!("non-final accumulated from probe_tls(): {}", acc);

    h.join().unwrap();

    // Probe the thread-local variables and get the accuulated value computed from
    // final thread-local values.
    let acc = control.probe_tls();
    println!("final accumulated from probe_tls(): {}", acc);

    // Drain the final thread-local values.
    let acc = control.drain_tls();

    // Print the accumulated value
    println!("accumulated={acc}");
}
