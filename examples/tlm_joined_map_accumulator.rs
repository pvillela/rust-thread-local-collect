//! Example usage of [`thread_local_collect::tlm::joined`].

use std::{
    collections::HashMap,
    env::set_var,
    fmt::Debug,
    ops::Deref,
    thread::{self, ThreadId},
    time::Duration,
};
use thread_local_collect::tlm::joined::{Control, Holder};

#[derive(Debug, Clone, PartialEq)]
struct Foo(String);

type Data = HashMap<i32, Foo>;

type AccValue = HashMap<ThreadId, HashMap<i32, Foo>>;

thread_local! {
    static MY_TL: Holder<Data, AccValue> = Holder::new();
}

fn insert_tl_entry(k: i32, v: Foo, control: &Control<Data, AccValue>) {
    control.with_data_mut(|data| {
        data.insert(k, v);
    });
}

fn print_tl(prefix: &str) {
    MY_TL.with(|r| {
        println!(
            "{}: local map for thread id={:?}: {:?}",
            prefix,
            thread::current().id(),
            r
        );
    });
}

fn op(data: Data, acc: &mut AccValue, tid: ThreadId) {
    println!(
        "`op` called from {:?} with data {:?}",
        thread::current().id(),
        data
    );

    acc.entry(tid).or_default();
    for (k, v) in data {
        acc.get_mut(&tid).unwrap().insert(k, v.clone());
    }
}

fn main() {
    // Set env variable value below to "trace" to see debug logs emitted by the library.
    set_var("RUST_LOG", "trace");
    _ = env_logger::try_init();

    let control = Control::new(&MY_TL, HashMap::new(), HashMap::new, op);

    insert_tl_entry(1, Foo("a".to_owned()), &control);
    insert_tl_entry(2, Foo("b".to_owned()), &control);
    print_tl("Main thread after inserts");

    thread::scope(|s| {
        let h = s.spawn(|| {
            insert_tl_entry(1, Foo("aa".to_owned()), &control);
            print_tl("Spawned thread before sleep");
            thread::sleep(Duration::from_millis(200));
            insert_tl_entry(2, Foo("bb".to_owned()), &control);
            print_tl("Spawned thread after sleep and additional insert");
        });
        h.join().unwrap();
    });

    println!("After spawned thread join: control={:?}", control);

    {
        // Take and accumulate the thread-local value from the main thread.
        control.take_own_tl();

        println!("After call to `take_tls`: control={:?}", control);

        // Different ways to print the accumulated value

        let acc = control.acc();
        println!("accumulated={:?}", acc.deref());
        drop(acc);

        control.with_acc(|acc| println!("accumulated={:?}", acc));

        let acc = control.clone_acc();
        println!("accumulated={:?}", acc);

        let acc = control.take_acc(HashMap::new());
        println!("accumulated={:?}", acc);
    }
}
