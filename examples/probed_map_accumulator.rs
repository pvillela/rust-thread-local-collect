//! Example usage of [`thread_local_collect::probed`].

use std::{
    collections::HashMap,
    fmt::Debug,
    ops::Deref,
    sync::Mutex,
    thread::{self, ThreadId},
};
use thread_local_collect::probed::{Control, Holder, HolderLocalKey};
use thread_local_collect::test_support::ThreadGater;

#[derive(Debug, Clone, PartialEq)]
struct Foo(String);

type Data = HashMap<u32, Foo>;

type AccumulatorMap = HashMap<ThreadId, HashMap<u32, Foo>>;

thread_local! {
    static MY_FOO_MAP: Holder<Data, AccumulatorMap> = Holder::new(HashMap::new);
}

fn insert_tl_entry(k: u32, v: Foo, control: &Control<Data, AccumulatorMap>) {
    MY_FOO_MAP.ensure_initialized(control);
    MY_FOO_MAP.with_data_mut(|data| data.insert(k, v));
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

fn assert_tl(other: &Data, msg: &str) {
    MY_FOO_MAP.with_data(|map| {
        assert_eq!(map, other, "{msg}");
    });
}

fn main() {
    let control = Control::new(HashMap::new(), op);

    let own_tid = thread::current().id();
    println!("main_tid={:?}", own_tid);

    let main_thread_gater = ThreadGater::new();
    let spawned_thread_gater = ThreadGater::new();

    let expected_acc_mutex = Mutex::new(HashMap::new());

    {
        let control = &control;
        let value1 = Foo("aa".to_owned());
        let value2 = Foo("bb".to_owned());
        let value3 = Foo("cc".to_owned());
        let value4 = Foo("dd".to_owned());

        thread::scope(|s| {
            let h = s.spawn(|| {
                let spawned_tid = thread::current().id();

                main_thread_gater.wait_for(0);
                insert_tl_entry(1, value1.clone(), &control);
                let mut other = HashMap::from([(1, value1)]);
                assert_tl(&other, "After 1st insert");
                expected_acc_mutex
                    .try_lock()
                    .unwrap()
                    .insert(spawned_tid, other.clone());
                spawned_thread_gater.open(0);

                main_thread_gater.wait_for(1);
                insert_tl_entry(2, value2.clone(), &control);
                other.insert(2, value2);
                assert_tl(&other, "After 2nd insert");
                expected_acc_mutex
                    .try_lock()
                    .unwrap()
                    .insert(spawned_tid, other.clone());
                spawned_thread_gater.open(1);

                main_thread_gater.wait_for(2);
                insert_tl_entry(3, value3.clone(), &control);
                let mut other = HashMap::from([(3, value3)]);
                assert_tl(&other, "After take_tls and 3rd insert");
                {
                    let mut map = expected_acc_mutex.try_lock().unwrap();
                    op(other.clone(), &mut map, &spawned_tid);
                }
                spawned_thread_gater.open(2);

                main_thread_gater.wait_for(3);
                insert_tl_entry(4, value4.clone(), &control);
                other.insert(4, value4);
                assert_tl(&other, "After 4th insert");
                {
                    let mut map = expected_acc_mutex.try_lock().unwrap();
                    op(other.clone(), &mut map, &spawned_tid);
                }
                // spawned_thread_gater.open(3);
            });

            {
                insert_tl_entry(1, Foo("a".to_owned()), &control);
                insert_tl_entry(2, Foo("b".to_owned()), &control);
                let map_own = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
                assert_tl(&map_own, "After main thread inserts");

                let mut map = expected_acc_mutex.try_lock().unwrap();
                map.insert(own_tid, map_own);
                let acc = control.probe_tls();
                assert_eq!(
                    &acc,
                    map.deref(),
                    "Accumulator after main thread inserts and probe_tls"
                );
                main_thread_gater.open(0);
            }

            {
                spawned_thread_gater.wait_for(0);
                let map = expected_acc_mutex.try_lock().unwrap();
                let acc = control.probe_tls();
                assert_eq!(
                    &acc,
                    map.deref(),
                    "Accumulator after 1st spawned thread insert and probe_tls"
                );
                main_thread_gater.open(1);
            }

            {
                spawned_thread_gater.wait_for(1);
                let map = expected_acc_mutex.try_lock().unwrap();
                control.take_tls();
                let acc = control.acc();
                assert_eq!(
                    acc.deref(),
                    map.deref(),
                    "Accumulator after 2nd spawned thread insert and take_tls"
                );
                main_thread_gater.open(2);
            }

            {
                spawned_thread_gater.wait_for(2);
                let map = expected_acc_mutex.try_lock().unwrap();
                let acc = control.probe_tls();
                assert_eq!(
                    &acc,
                    map.deref(),
                    "Accumulator after 3rd spawned thread insert and probe_tls"
                );
                main_thread_gater.open(3);
            }

            {
                // done with thread gaters
                h.join().unwrap();

                let map = expected_acc_mutex.try_lock().unwrap();

                {
                    control.take_tls();
                    let acc = control.acc();
                    assert_eq!(
                        acc.deref(),
                        map.deref(),
                        "Accumulator after 4th spawned thread insert and take_tls"
                    );
                }

                {
                    control.take_tls();
                    let acc = control.acc();
                    assert_eq!(
                        acc.deref(),
                        map.deref(),
                        "Idempotency of control.take_tls()"
                    );
                }

                {
                    control.with_acc(|acc| {
                        assert_eq!(
                            acc,
                            map.deref(),
                            "Accumulator after 4th spawned thread, using control.with_acc()"
                        );
                    });
                }

                {
                    let acc = control.clone_acc();
                    assert_eq!(
                        &acc,
                        map.deref(),
                        "Accumulator after 4th spawned thread, using control.clone_acc()"
                    );
                }

                {
                    let acc = control.take_acc(HashMap::new());
                    assert_eq!(
                        &acc,
                        map.deref(),
                        "Accumulator after 4th spawned thread, using control.take_acc()"
                    );
                }

                {
                    control.with_acc(|acc| {
                        assert_eq!(acc, &HashMap::new(), "Accumulator after control.take_acc()");
                    });
                }
            }
        });
    }
}
