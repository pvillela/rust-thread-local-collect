//! Simple example usage of [`thread_local_collect::tlcr::probed`].

use std::{
    thread::{self, ThreadId},
    time::Duration,
};
use thread_local_collect::tlcr::probed::Control;

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

fn main() {
    let mut control = Control::new(|| 0, op, op_r);

    control.send_data(1);

    thread::scope(|s| {
        let h = s.spawn(|| {
            control.send_data(10);
            thread::sleep(Duration::from_millis(10));
            control.send_data(20);
        });

        // Wait for spawned thread to do some work.
        thread::sleep(Duration::from_millis(5));

        // Probe the thread-local values and get the accuulated value computed from
        // current thread-local values.
        let acc = control.probe_tls().unwrap();
        println!("non-final accumulated from probe_tls(): {}", acc);

        h.join().unwrap();
    });

    // Probe the thread-local variables and get the accuulated value computed from
    // final thread-local values.
    let acc = control.probe_tls().unwrap();
    println!("final accumulated from probe_tls(): {}", acc);

    // Drain the final thread-local values.
    let acc = control.drain_tls().unwrap();

    // Print the accumulated value
    println!("accumulated={acc}");
}
