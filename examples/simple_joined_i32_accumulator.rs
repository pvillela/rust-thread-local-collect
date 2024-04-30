//! Simple example usage of [`thread_local_collect::simple_joined`].

use std::{
    ops::Deref,
    thread::{self, ThreadId},
};
use thread_local_collect::simple_joined::{Control, Holder};

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
        });
        h.join().unwrap();
    });

    {
        // Different ways to print the accumulated value

        println!("accumulated={}", control.acc().deref());

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
