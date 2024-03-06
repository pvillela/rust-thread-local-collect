//! Example usage of [`thread_local_collect::channeled`].

use std::{
    collections::HashMap,
    fmt::Debug,
    ops::Deref,
    sync::RwLock,
    thread::{self, ThreadId},
    time::Duration,
};
use thread_local_collect::channeled::{Control, Holder, HolderLocalKey};

#[derive(Debug, Clone, PartialEq)]
struct Foo(String);

type Data = (u32, Foo);

type AccumulatorMap = HashMap<ThreadId, HashMap<u32, Foo>>;

thread_local! {
    static MY_FOO_MAP: Holder<Data> = Holder::new();
}

fn send_tl_data(k: u32, v: Foo, control: &Control<Data, AccumulatorMap>) {
    MY_FOO_MAP.ensure_initialized(control);
    MY_FOO_MAP.send_data((k, v));
}

fn op(data: Data, acc: &mut AccumulatorMap, tid: &ThreadId) {
    println!(
        "`op` called from {:?} with data {:?}",
        thread::current().id(),
        data
    );

    acc.entry(tid.clone()).or_insert_with(|| HashMap::new());
    let (k, v) = data;
    acc.get_mut(tid).unwrap().insert(k, v.clone());
}

fn main() {
    let control = Control::new(HashMap::new(), op);

    let main_tid = thread::current().id();
    println!("main_tid={:?}", main_tid);
    let spawned_tid = RwLock::new(thread::current().id());

    {
        send_tl_data(1, Foo("a".to_owned()), &control);
        send_tl_data(2, Foo("b".to_owned()), &control);
        println!("after main thread inserts: {:?}", control.acc());
    }

    thread::sleep(Duration::from_millis(100));

    control.start_receiving_tls();

    thread::scope(|s| {
        let h = s.spawn(|| {
            let mut lock = spawned_tid.write().unwrap();
            *lock = thread::current().id();
            drop(lock);

            send_tl_data(1, Foo("aa".to_owned()), &control);

            thread::sleep(Duration::from_millis(200));

            send_tl_data(2, Foo("bb".to_owned()), &control);
        });

        thread::sleep(Duration::from_millis(50));

        let spawned_tid = spawned_tid.try_read().unwrap();
        println!("spawned_tid={:?}", spawned_tid);

        control.stop_receiving_tls();
        println!("after control.stop_receiving_tls(): {:?}", control.acc());

        h.join().unwrap();
    });

    {
        println!("before control.receive_tls(): {:?}", control.acc());

        // Drain channel.
        control.receive_tls();
        println!("after control.receive_tls(): {:?}", control.acc());

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
