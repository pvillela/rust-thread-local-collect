//! Simple example usage of [`thread_local_collect::channeled`].

use std::{
    ops::Deref,
    thread::{self, ThreadId},
};
use thread_local_collect::channeled::{Control, Holder, HolderLocalKey};

// Define your data type, e.g.:
type Data = i32;

// Define your accumulated value type.
type AccValue = i32;

// Define your thread-local:
thread_local! {
    static MY_TL: Holder<Data> = Holder::new();
}

// Define your accumulation operation.
fn op(data: Data, acc: &mut AccValue, _: &ThreadId) {
    *acc += data;
}

// Create a function to send the thread-local value:
fn send_tl_data(value: Data, control: &Control<Data, AccValue>) {
    MY_TL.ensure_initialized(control);
    MY_TL.send_data(value);
}

fn main() {
    let control = Control::new(0, op);

    send_tl_data(1, &control);

    thread::scope(|s| {
        let h = s.spawn(|| {
            send_tl_data(10, &control);
        });
        h.join().unwrap();
    });

    {
        // Drain channel.
        control.receive_tls();

        // Different ways to print the accumulated value

        let acc = control.acc();
        println!("accumulated={}", acc.deref());
        drop(acc);

        control.with_acc(|acc| println!("accumulated={}", acc));

        let acc = control.clone_acc();
        println!("accumulated={}", acc);

        let acc = control.take_acc(0);
        println!("accumulated={}", acc);
    }
}
