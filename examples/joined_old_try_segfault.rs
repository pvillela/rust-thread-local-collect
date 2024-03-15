//! This example attempts to use an unsafe race condition in [`thread_local_collect::joined_old::Control::take_tls`]
//! to create a segmentation fault, but it doesn't succeed

use std::{
    collections::HashMap,
    fmt::Debug,
    hint::black_box,
    thread::{self, ThreadId},
    time::Duration,
};
use thread_local_collect::joined_old::{Control, Holder, HolderLocalKey};

#[derive(Debug, Clone, PartialEq)]
struct Foo(String);

type Data = HashMap<u32, Foo>;

type AccumulatorMap = HashMap<ThreadId, HashMap<u32, Foo>>;

thread_local! {
    static MY_FOO_MAP: Holder<Data, AccumulatorMap> = Holder::new(HashMap::new);
}

fn insert_tl_entry(k: u32, v: Foo, control: &Control<Data, AccumulatorMap>) {
    MY_FOO_MAP.ensure_linked(control);
    MY_FOO_MAP.with_data_mut(|data| data.insert(k, v));
}

fn op(data: HashMap<u32, Foo>, acc: &mut AccumulatorMap, tid: ThreadId) {
    acc.entry(tid).or_default();
    for (k, v) in data {
        acc.get_mut(&tid).unwrap().insert(k, v.clone());
    }
}

/// This example attempts to use an unsafe race condition to create a segmentation fault,
/// but it doesn't succeed
fn try_segfault() {
    let control: Control<HashMap<u32, Foo>, AccumulatorMap> = Control::new(HashMap::new(), op);

    const N_THREADS: u32 = 100;
    const N_REPEATS1: u32 = 1_000_000;
    const N_REPEATS2: u32 = 10000;
    const N_REPEATS: u32 = N_REPEATS1 + N_REPEATS2;

    let f = || {
        let _hs = (0..N_THREADS)
            .map(|_| {
                let control = control.clone();
                thread::spawn(move || {
                    for j in 0..N_REPEATS {
                        let v = Foo("a".to_owned() + &j.to_string());
                        insert_tl_entry(j % 10, v, &control);
                    }
                })
            })
            .collect::<Vec<_>>();
    };

    thread::scope(|s| {
        let _h = s.spawn(f);

        thread::sleep(Duration::from_micros(1));

        for k in 0..N_REPEATS1 {
            unsafe { control.take_tls() };

            control.with_acc(|acc| black_box(format!("{:?}", acc).len()));

            // println!("k={k},len={len}; ");
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
    try_segfault();
}
