//! Demonstrates race condition in [thread_local_collect::joined::Control::take_tls].

use thread_local_collect::{
    common::HolderLocalKey,
    joined::{Control, Holder},
};

use std::{
    collections::HashMap,
    fmt::Debug,
    ops::Deref,
    sync::RwLock,
    thread::{self, ThreadId},
    time::Duration,
};

#[derive(Debug, Clone, PartialEq)]
struct Foo(String);

type Data = HashMap<u32, Foo>;

type AccumulatorMap = HashMap<ThreadId, HashMap<u32, Foo>>;

thread_local! {
    static MY_FOO_MAP: Holder<Data, AccumulatorMap> = Holder::new(HashMap::new);
}

fn insert_tl_entry(k: u32, v: Foo, control: &Control<Data, AccumulatorMap>) {
    MY_FOO_MAP.ensure_linked(control);
    MY_FOO_MAP.with_data_mut(|data| data.insert(k, v)).unwrap();
}

fn op(data: HashMap<u32, Foo>, acc: &mut AccumulatorMap, tid: ThreadId) {
    acc.entry(tid).or_default();
    for (k, v) in data {
        acc.get_mut(&tid).unwrap().insert(k, v.clone());
    }
}

fn assert_tl(other: &Data, msg: &str) {
    MY_FOO_MAP
        .with_data(|map| {
            assert_eq!(map, other, "{msg}");
        })
        .unwrap();
}

/// Demonstrates race condition in [thread_local_collect::joined::Control::take_tls]
fn own_thread_and_implicit_joins() {
    let control = Control::new(HashMap::new(), op);

    let own_tid = thread::current().id();
    println!("main_tid={:?}", own_tid);
    let spawned_tids = RwLock::new(Vec::<ThreadId>::new());

    {
        insert_tl_entry(1, Foo("a".to_owned()), &control);
        insert_tl_entry(2, Foo("b".to_owned()), &control);

        let other = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
        assert_tl(&other, "After main thread inserts");
    }

    thread::sleep(Duration::from_millis(100));

    let map_own = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
    let mut map = HashMap::from([(own_tid, map_own)]);

    for i in 0..100 {
        let value1 = Foo("a".to_owned() + &i.to_string());
        let value2 = Foo("a".to_owned() + &i.to_string());
        let map_i = &HashMap::from([(1, value1.clone()), (2, value2.clone())]);

        thread::scope(|s| {
            let _h = s.spawn(|| {
                let spawned_tid = thread::current().id();
                let mut lock = spawned_tids.write().unwrap();
                lock.push(spawned_tid);
                drop(lock);

                insert_tl_entry(1, value1.clone(), &control);
                let other = HashMap::from([(1, value1)]);
                assert_tl(&other, "Before 1st insert");

                insert_tl_entry(2, value2, &control);
                assert_tl(map_i, "Before 2nd insert");
            });
        });

        {
            let lock = spawned_tids.read().unwrap();
            let spawned_tid = lock.last().unwrap();
            map.insert(*spawned_tid, map_i.clone());

            // Safety: NOT called after all other threads explicitly joined.
            unsafe { control.take_tls() };

            let acc = control.acc();
            assert_eq!(acc.deref(), &map, "Accumulator check on iteration {}", i);
        }
    }
}

fn main() {
    own_thread_and_implicit_joins();
}
