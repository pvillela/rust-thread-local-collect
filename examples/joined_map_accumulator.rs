//! Example usage of [`thread_local_collect`].

use env_logger;
use std::{
    collections::HashMap,
    env::set_var,
    fmt::Debug,
    ops::Deref,
    thread::{self, ThreadId},
    time::Duration,
};
use thread_local_collect::{
    joined::{Control, Holder},
    HolderLocalKey,
};

#[derive(Debug, Clone)]
struct Foo(String);

type Data = HashMap<u32, Foo>;

type AccumulatorMap = HashMap<ThreadId, HashMap<u32, Foo>>;

thread_local! {
    static MY_FOO_MAP: Holder<Data, AccumulatorMap> = Holder::new(HashMap::new);
}

fn insert_tl_entry(k: u32, v: Foo, control: &Control<Data, AccumulatorMap>) {
    MY_FOO_MAP.ensure_initialized(control);
    MY_FOO_MAP.with_data_mut(|data| {
        data.insert(k, v);
    });
}

fn print_tl(prefix: &str) {
    MY_FOO_MAP.with(|r| {
        println!(
            "{}: local map for thread id={:?}: {:?}",
            prefix,
            thread::current().id(),
            r
        );
    });
}

fn op(data: HashMap<u32, Foo>, acc: &mut AccumulatorMap, tid: &ThreadId) {
    println!(
        "`op` called from {:?} with data {:?}",
        thread::current().id(),
        data
    );

    acc.entry(tid.clone()).or_insert_with(|| HashMap::new());
    for (k, v) in data {
        acc.get_mut(tid).unwrap().insert(k, v.clone());
    }
}

fn main() {
    // Set env variable value below to "trace" to see debug logs emitted by the library.
    set_var("RUST_LOG", "trace");
    _ = env_logger::try_init();

    let control = Control::new(HashMap::new(), op);

    insert_tl_entry(1, Foo("a".to_owned()), &control);
    insert_tl_entry(2, Foo("b".to_owned()), &control);
    print_tl("Main thread after inserts");

    thread::scope(|s| {
        s.spawn(|| {
            insert_tl_entry(1, Foo("aa".to_owned()), &control);
            print_tl("Spawned thread before sleep");
            thread::sleep(Duration::from_millis(200));
            insert_tl_entry(2, Foo("bb".to_owned()), &control);
            print_tl("Spawned thread after sleep and additional insert");
        });
    });

    println!("After spawned thread join: control={:?}", control);

    {
        // SAFETY: OK to call function below after all other threads have joined.
        unsafe { control.take_tls() };

        println!("After call to `take_tls`: control={:?}", control);

        // Print the accumulated value.
        control.with_acc(|acc| println!("accumulated={:?}", acc));

        // Another way to print the accumulated value.
        let acc = control.acc();
        println!("accumulated={:?}", acc.deref());
    }
}
