//! This example attempts to use an unsafe race condition in [`thread_local_collect::joined_old::Control::take_tls`]
//! to create a segmentation fault, but it doesn't succeed

use std::{
    hint::black_box,
    mem::replace,
    thread::{self, ThreadId},
    time::Duration,
};
use thread_local_collect::joined_old::{Control, Holder, HolderLocalKey};

#[derive(Debug)]
struct Foo(String);

impl Foo {
    fn new() -> Self {
        Foo(String::new())
    }
}

type Data = Option<Foo>;

type AccValue = Option<Foo>;

thread_local! {
    static MY_TL: Holder<Data, AccValue> = Holder::new(||Some(Foo::new()));
}

fn update_tl(value: Data, control: &Control<Data, AccValue>) {
    MY_TL.ensure_linked(control);
    MY_TL.with_data_mut(|data| replace(data, value));
}

fn op(data: Data, acc: &mut AccValue, _tid: ThreadId) {
    *acc = data;
}

/// This example attempts to use an unsafe race condition to create a segmentation fault,
/// but it doesn't succeed
fn try_segfault1() {
    let control: Control<Data, AccValue> = Control::new(Some(Foo::new()), op);

    const N_THREADS: u32 = 6;
    const N_REPEATS1: u32 = 1_000_000;
    const N_REPEATS2: u32 = 1_000_000;

    let f = || {
        let _hs = (0..N_THREADS)
            .map(|_| {
                let control = control.clone();
                thread::spawn(move || {
                    for j in 0..N_REPEATS1 {
                        update_tl(Some(Foo(j.to_string())), &control);
                        // thread::sleep(Duration::from_micros(SLEEP_MICROS_THREAD));
                    }
                })
            })
            .collect::<Vec<_>>();
    };

    thread::scope(|s| {
        let _h = s.spawn(f);

        thread::sleep(Duration::from_micros(1));

        for k in 0..N_REPEATS2 {
            unsafe { control.take_tls() };

            control.with_acc(|acc| black_box(format!("{:?}", acc)));

            if k % 1000 == 0 {
                println!();
                print!("{k}--")
            }
            print!("{}", k % 10);
        }
    });

    println!("The End")
}

fn main() {
    try_segfault1();
}
