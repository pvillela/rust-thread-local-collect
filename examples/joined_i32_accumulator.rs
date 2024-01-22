//! Simple example usage of [`thread_local_collect`].

use std::thread::{self, ThreadId};
use thread_local_collect::{
    joined::{Control, Holder},
    HolderLocalKey,
};

// Define your data type, e.g.:
type Data = i32;

// Define your accumulated value type. It can be `()` if you don't need an accumulator.
type AccValue = i32;

// Define your thread-local:
thread_local! {
    static MY_TL: Holder<Data, AccValue> = Holder::new(|| 0);
}

// Define your accumulation operation.
// You can use the closure `|_, _, _| ()` inline in the `Control` constructor if you don't need an accumulator.
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
        s.spawn(|| {
            update_tl(10, &control);
        });
    });

    {
        // Acquire `control`'s lock.
        let mut lock = control.lock();

        // SAFETY: Call this after all other threads registered with `control` have been joined.
        unsafe { control.take_tls(&mut lock) };

        control.with_acc(&lock, |acc| println!("accumulated={}", acc));
    }
}
