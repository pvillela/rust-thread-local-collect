//! Example usage of [`thread_local_collect::channeled`].

use std::{
    collections::HashMap,
    fmt::Debug,
    ops::Deref,
    sync::Mutex,
    thread::{self, ThreadId},
    time::Duration,
};
use thread_local_collect::{
    channeled::{Control, Holder},
    test_support::{assert_eq_and_println, ThreadGater},
};

#[derive(Debug, Clone, PartialEq)]
struct Foo(String);

type Data = (u32, Foo);

type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

thread_local! {
    static MY_TL: Holder<Data> = Holder::new();
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

fn send_tl_data(k: u32, v: Foo, control: &Control<Data, AccValue>) {
    control.send_data((k, v));
}

#[test]
fn test() {
    main();
}

fn main() {
    let control = Control::new(&MY_TL, HashMap::new(), op);

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
                send_tl_data(k, v.clone(), &control);
                my_map.insert(k, v);
                expected_acc_mutex
                    .try_lock()
                    .unwrap()
                    .insert(spawned_tid, my_map.clone());
                // allow background receiving thread to receive above send
                thread::sleep(Duration::from_millis(10));
                spawned_thread_gater.open(gate);
            };

            process_value(0, 1, Foo("aa".to_owned()));
            process_value(1, 2, Foo("bb".to_owned()));
            process_value(2, 3, Foo("cc".to_owned()));
            process_value(3, 4, Foo("dd".to_owned()));
        });

        {
            control.start_receiving_tls().unwrap();
        }

        {
            send_tl_data(1, Foo("a".to_owned()), &control);
            send_tl_data(2, Foo("b".to_owned()), &control);
            let my_map = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);

            // Allow background receiving thread to receive above sends.
            thread::sleep(Duration::from_millis(10));

            expected_acc_mutex
                .try_lock()
                .unwrap()
                .insert(main_tid, my_map);
            assert_acc(control.acc().deref(), "Accumulator after main thread sends");
            main_thread_gater.open(0);
        }

        {
            spawned_thread_gater.wait_for(0);
            assert_acc(
                control.acc().deref(),
                "Accumulator after 1st spawned thread send",
            );

            {
                control.stop_receiving_tls();
                // Allow background receiving thread to process command.
                thread::sleep(Duration::from_millis(10));
            }

            main_thread_gater.open(1);
        }

        {
            spawned_thread_gater.wait_for(1);
            {
                let exp = expected_acc_mutex.try_lock().unwrap();
                let acc = control.acc();
                assert_ne!(
                    acc.deref(),
                    exp.deref(),
                    "Accumulator should not reflect 2nd spawned thread send",
                );
            }
            main_thread_gater.open(2);
        }

        {
            control.start_receiving_tls().unwrap();
            // Allow background receiving thread to process command.
            thread::sleep(Duration::from_millis(10));
        }

        {
            spawned_thread_gater.wait_for(2);
            assert_acc(
                control.acc().deref(),
                "Accumulator should reflect 2nd and 3rd spawned thread sends",
            );

            {
                control.stop_receiving_tls();
                // Allow background receiving thread to process command.
                thread::sleep(Duration::from_millis(10));
            }

            main_thread_gater.open(3);
        }

        {
            // Join spawned thread.
            h.join().unwrap();

            {
                let exp = expected_acc_mutex.try_lock().unwrap();
                let acc = control.acc();
                assert_ne!(
                    acc.deref(),
                    exp.deref(),
                    "Accumulator should not reflect 4th spawned thread send",
                );
            }

            control.drain_tls();

            assert_acc(
                control.acc().deref(),
                "Accumulator should reflect 4th spawned thread send",
            );
        }

        {
            {
                control.with_acc(|acc| {
                    assert_acc(
                        acc,
                        "Accumulator after spawned thread join, using control.with_acc()",
                    );
                });
            }

            {
                let acc = control.clone_acc();
                assert_acc(
                    &acc,
                    "Accumulator after spawned thread join, using control.clone_acc()",
                );
            }

            {
                let acc = control.take_acc(HashMap::new());
                assert_acc(
                    &acc,
                    "Accumulator after spawned thread join, using control.take_acc()",
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
