//! Example usage of [`thread_local_collect::tlcr::probed`].

use std::{
    collections::HashMap,
    fmt::Debug,
    ops::Deref,
    sync::Mutex,
    thread::{self, ThreadId},
};
use thread_local_collect::test_support::ThreadGater;
use thread_local_collect::{test_support::assert_eq_and_println, tlcr::probed::Control};

#[derive(Debug, Clone, PartialEq)]
struct Foo(String);

type Data = (u32, Foo);

type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

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

#[test]
fn test() {
    main();
}

fn main() {
    let mut control = Control::new(HashMap::new, op, op_r);

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

            let mut process_value = |gate: u8, k: u32, v: Foo| {
                main_thread_gater.wait_for(gate);
                control.send_data((k, v.clone()));
                my_map.insert(k, v);
                expected_acc_mutex
                    .try_lock()
                    .unwrap()
                    .insert(spawned_tid, my_map.clone());
                spawned_thread_gater.open(gate);
            };

            process_value(0, 1, Foo("aa".to_owned()));
            process_value(1, 2, Foo("bb".to_owned()));
            process_value(2, 3, Foo("cc".to_owned()));
            process_value(3, 4, Foo("dd".to_owned()));
        });

        {
            control.send_data((1, Foo("a".to_owned())));
            control.send_data((2, Foo("b".to_owned())));
            let my_map = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);

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
            let acc = control.probe_tls();
            assert_acc(
                &acc,
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
        }
    });

    {
        let acc = control.drain_tls().unwrap();
        assert_acc(
            &acc,
            "Accumulator after 4th spawned thread insert and drain_tls",
        );
    }
}
