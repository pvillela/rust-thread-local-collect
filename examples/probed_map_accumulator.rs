//! Example usage of [`thread_local_collect::probed`].

use std::{
    collections::HashMap,
    fmt::Debug,
    ops::Deref,
    sync::Mutex,
    thread::{self, ThreadId},
};
use thread_local_collect::test_support::ThreadGater;
use thread_local_collect::{
    probed::{Control, Holder, HolderLocalKey},
    test_support::assert_eq_and_println,
};

#[derive(Debug, Clone, PartialEq)]
struct Foo(String);

type Data = HashMap<u32, Foo>;

type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

thread_local! {
    static MY_FOO_MAP: Holder<Data, AccValue> = Holder::new(HashMap::new);
}

fn insert_tl_entry(k: u32, v: Foo, control: &Control<Data, AccValue>) {
    MY_FOO_MAP.ensure_initialized(control);
    MY_FOO_MAP.with_data_mut(|data| data.insert(k, v));
}

fn op(data: HashMap<u32, Foo>, acc: &mut AccValue, tid: &ThreadId) {
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
        assert_eq_and_println(map, other, msg);
    });
}

fn main() {
    let control = Control::new(HashMap::new(), op);

    let main_tid = thread::current().id();
    println!("main_tid={:?}", main_tid);

    let main_thread_gater = ThreadGater::new("main");
    let spawned_thread_gater = ThreadGater::new("spawned");

    let expected_acc_mutex = Mutex::new(HashMap::new());

    let assert_acc = |acc: &AccValue, msg: &str| {
        let exp_guard = expected_acc_mutex.try_lock().unwrap();
        let exp = exp_guard.deref();

        assert_eq_and_println(acc, exp, msg);
    };

    thread::scope(|s| {
        let h = s.spawn(|| {
            let spawned_tid = thread::current().id();
            println!("spawned tid={:?}", spawned_tid);

            let mut my_map = HashMap::<u32, Foo>::new();

            let process_value =
                |gate: u8, k: u32, v: Foo, my_map: &mut HashMap<u32, Foo>, assert_tl_msg: &str| {
                    main_thread_gater.wait_for(gate);
                    insert_tl_entry(k, v.clone(), &control);
                    my_map.insert(k, v);
                    assert_tl(my_map, assert_tl_msg);

                    let mut exp = expected_acc_mutex.try_lock().unwrap();
                    op(my_map.clone(), &mut exp, &spawned_tid);

                    spawned_thread_gater.open(gate);
                };

            process_value(
                0,
                1,
                Foo("aa".to_owned()),
                &mut my_map,
                "After spawned thread 1st insert",
            );

            process_value(
                1,
                2,
                Foo("bb".to_owned()),
                &mut my_map,
                "After spawned thread 2nd insert",
            );

            my_map = HashMap::new();
            process_value(
                2,
                3,
                Foo("cc".to_owned()),
                &mut my_map,
                "After take_tls and spawned thread 3rd insert",
            );

            process_value(
                3,
                4,
                Foo("dd".to_owned()),
                &mut my_map,
                "After spawned thread 4th insert",
            );
        });

        {
            insert_tl_entry(1, Foo("a".to_owned()), &control);
            insert_tl_entry(2, Foo("b".to_owned()), &control);
            let my_map = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
            assert_tl(&my_map, "After main thread inserts");

            let mut map = expected_acc_mutex.try_lock().unwrap();
            map.insert(main_tid, my_map);
            let acc = control.probe_tls();
            assert_eq_and_println(
                &acc,
                map.deref(),
                "Accumulator after main thread inserts and probe_tls",
            );
            main_thread_gater.open(0);
        }

        {
            spawned_thread_gater.wait_for(0);
            let acc = control.probe_tls();
            assert_acc(
                &acc,
                "Accumulator after 1st spawned thread insert and probe_tls",
            );
            main_thread_gater.open(1);
        }

        {
            spawned_thread_gater.wait_for(1);
            control.take_tls();
            assert_acc(
                control.acc().deref(),
                "Accumulator after 2nd spawned thread insert and take_tls",
            );
            main_thread_gater.open(2);
        }

        {
            spawned_thread_gater.wait_for(2);
            let acc = control.probe_tls();
            assert_acc(
                &acc,
                "Accumulator after 3rd spawned thread insert and probe_tls",
            );
            main_thread_gater.open(3);
        }

        {
            // done with thread gaters
            h.join().unwrap();

            {
                control.take_tls();
                assert_acc(
                    control.acc().deref(),
                    "Accumulator after 4th spawned thread insert and take_tls",
                );
            }

            {
                control.take_tls();
                assert_acc(control.acc().deref(), "Idempotency of control.take_tls()");
            }

            {
                let acc = control.probe_tls();
                assert_acc(&acc, "After take_tls(), probe_tls() the same acc value");
            }

            {
                control.with_acc(|acc| {
                    assert_acc(
                        acc,
                        "Accumulator after 4th spawned thread insert, using control.with_acc()",
                    );
                });
            }

            {
                let acc = control.clone_acc();
                assert_acc(
                    &acc,
                    "Accumulator after 4th spawned thread insert, using control.clone_acc()",
                );
            }

            {
                let acc = control.take_acc(HashMap::new());
                assert_acc(
                    &acc,
                    "Accumulator after 4th spawned thread insert, using control.take_acc()",
                );
            }

            {
                control.with_acc(|acc| {
                    assert_eq_and_println(
                        acc,
                        &HashMap::new(),
                        "Accumulator after control.take_acc()",
                    );
                });
            }
        }
    });
}
