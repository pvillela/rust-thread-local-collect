//! Example usage of inappropriate usage of [`thread_local_drop`].

use env_logger;
use std::{
    collections::HashMap,
    env::set_var,
    fmt::Debug,
    thread::{self, ThreadId},
    time::Duration,
};
use thread_local_drop::{
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
        let _h = s.spawn(|| {
            insert_tl_entry(1, Foo("aa".to_owned()), &control);
            print_tl("Spawned thread before sleep");
            thread::sleep(Duration::from_millis(200));

            // At this point, the control tmap is empty due to the timoing of the call to ensure_tls_dropped
            // below and the data has been set to None. The call below updates the data to Some of a
            // HashMap with the entry (2, "bb").
            insert_tl_entry(2, Foo("bb".to_owned()), &control);

            print_tl("Spawned thread after sleep and additional insert");
        });

        thread::sleep(Duration::from_millis(50));
        println!("Main thread after sleep: control={:?}", control);

        // Don't do this in production code. For demonstration purposes only.
        // Making this call before joining with `_h` is dangerous because there is a data race.
        unsafe { control.collect_all(&mut control.lock()) };

        println!(
            "After premature call to `ensure_tls_dropped`: control={:?}",
            control
        );

        // Implicit join at end of scope.
        // _h.join().unwrap();
    });

    println!("After spawned thread join: control={:?}", control);

    {
        let mut lock = control.lock();

        // SAFETY: Due to the implicit join above, we have a data race here. Therefore, the
        // address in `control`'s `tmap` associated with the spawned thread may point to an invalid memory chunk,
        // resulting in a sgementation fault when the address is dereferenced by `ensure_tls_dropped`.
        // In case the memory chunk pointed to by the address is valid, there may be additional issues further
        // below where the accumulator is printed.
        unsafe { control.collect_all(&mut lock) };

        println!(
            "After 2nd call to `ensure_tls_dropped`: control={:?}",
            control
        );

        // Due to the above-mentioned data race, if the address in question points to a valid memory chunk and
        // a segmentation fault doesn't occur above, then there can be 3 possibilities:
        // 1. The destructor of the Holder for the spawned thread is not holding control's Mutex lock and it has not
        //    completed execution, so the accumulated value does not reflect the second insert in the spawned thread.
        // 2. The destructor of the Holder for the spawned thread has already completed execution and the accumulated
        //    value reflects the second insert in the spawned thread.
        control.with_acc(&lock, |acc| println!("accumulated={:?}", acc));
    }
}
