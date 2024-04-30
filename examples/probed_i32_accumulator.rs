//! Simple example usage of [`thread_local_collect::probed`].

use std::{
    ops::Deref,
    thread::{self, ThreadId},
    time::Duration,
};
use thread_local_collect::probed::{Control, Holder};

// Define your data type, e.g.:
type Data = i32;

// Define your accumulated value type.
type AccValue = i32;

// Define your thread-local:
thread_local! {
    static MY_TL: Holder<Data, AccValue> = Holder::new(|| 0);
}

// Define your accumulation operation.
fn op(data: Data, acc: &mut AccValue, _: ThreadId) {
    *acc += data;
}

// Create a function to update the thread-local value:
fn update_tl(value: Data, control: &Control<Data, AccValue>) {
    control.with_data_mut(|data| {
        *data = value;
    });
}

fn main() {
    let control = Control::new(&MY_TL, 0, op);

    update_tl(1, &control);

    thread::scope(|s| {
        let h = s.spawn(|| {
            update_tl(10, &control);
            thread::sleep(Duration::from_millis(10));
            update_tl(20, &control);
        });

        {
            // Wait for spawned thread to do some work.
            thread::sleep(Duration::from_millis(5));

            // Probe the thread-local variables and get the accuulated value computed from
            // current thread-local values without updating the accumulated value in `control`.
            let acc = control.probe_tls();
            println!("non-final accumulated from probe_tls(): {}", acc);

            h.join().unwrap();

            // Probe the thread-local variables and get the accuulated value computed from
            // final thread-local values without updating the accumulated value in `control`.
            let acc = control.probe_tls();
            println!("final accumulated from probe_tls(): {}", acc);

            // Take the final thread-local values and accumulate them in `control`.
            control.take_tls();

            // Different ways to print the accumulated value in `control`.

            println!("final accumulated={}", control.acc().deref());

            let acc = control.acc();
            println!("final accumulated: {}", acc.deref());
            drop(acc);

            control.with_acc(|acc| println!("final accumulated: {}", acc));

            let acc = control.clone_acc();
            println!("final accumulated: {}", acc);

            let acc = control.probe_tls();
            println!("final accumulated from probe_tls(): {}", acc);

            let acc = control.take_acc(0);
            println!("final accumulated: {}", acc);
        }
    });
}
