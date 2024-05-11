//! Example usage of [`thread_local_collect::tlm::joined`].

use std::{
    collections::HashMap,
    env::set_var,
    fmt::Debug,
    thread::{self, ThreadId},
    time::Duration,
};
use thread_local_collect::tlm::joined::{ControlSend, HolderSend};

#[derive(Debug, Clone, PartialEq)]
struct Foo(String);

type Data = (u32, Foo);

type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

thread_local! {
    static MY_TL: HolderSend<AccValue> = HolderSend::new(HashMap::new);
}

fn op(data: Data, acc: &mut AccValue, tid: ThreadId) {
    println!(
        "`op` called from {:?} with data {:?}",
        thread::current().id(),
        data
    );

    acc.entry(tid).or_default();
    let (k, v) = data;
    acc.get_mut(&tid).unwrap().insert(k, v.clone());
}

fn op_r(acc1: AccValue, acc2: AccValue) -> AccValue {
    println!(
        "`op_r` called from {:?} with acc1={:?} and acc2={:?}",
        thread::current().id(),
        acc1,
        acc2
    );

    let mut acc = acc1;
    acc2.into_iter().for_each(|(k, v)| {
        acc.insert(k, v);
    });
    acc
}

fn main() {
    // Set env variable value below to "trace" to see debug logs emitted by the library.
    set_var("RUST_LOG", "trace");
    _ = env_logger::try_init();

    let mut control = ControlSend::new(&MY_TL, HashMap::new, op, op_r);

    control.send_data((1, Foo("a".to_owned())));
    control.send_data((2, Foo("b".to_owned())));

    thread::scope(|s| {
        let h = s.spawn(|| {
            control.send_data((1, Foo("aa".to_owned())));
            thread::sleep(Duration::from_millis(200));
            control.send_data((2, Foo("bb".to_owned())));
        });
        h.join().unwrap();
    });

    println!("After spawned thread join: control={:?}", control);

    // Get accumulated value.
    let acc = control.drain_tls();
    println!("accumulated from drain_tls={:?}", acc);

    println!("After call to `drain_tls`: control={:?}", control);
}
