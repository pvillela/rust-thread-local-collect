//! Simple example usage of [`thread_local_collect::joined`].

use std::{
    ops::Deref,
    thread::{self, ThreadId},
};
use thread_local_collect::joined::{Control, Holder, HolderLocalKey};

// Define your data type, e.g.:
type Data = i32;

// Define your accumulated value type.
type AccValue = i32;

// Define your thread-local:
thread_local! {
    static MY_TL: Holder<Data, AccValue> = Holder::new(|| 0);
}

// Define your accumulation operation.
fn op(data: Data, acc: &mut AccValue, _: &ThreadId) {
    *acc += data;
}

// Create a function to update the thread-local value:
fn update_tl(value: Data, control: &Control<Data, AccValue>) {
    MY_TL.ensure_initialized(control);
    MY_TL.with_data_mut(|data| {
        *data = value;
    });
}

fn main() {
    let control = Control::new(0, op);

    update_tl(1, &control);

    thread::scope(|s| {
        let h = s.spawn(|| {
            update_tl(10, &control);
        });
        h.join().unwrap();
    });

    {
        // Take and accumulate the thread-local values.
        // SAFETY: Call this after all other threads registered with `control` have been joined.
        unsafe { control.take_tls() };

        // Print the accumulated value.
        control.with_acc(|acc| println!("accumulated={}", acc));

        // Another way to print the accumulated value.
        let acc = control.acc();
        println!("accumulated={}", acc.deref());
    }
}
