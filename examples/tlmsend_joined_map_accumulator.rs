//! Example usage of [`thread_local_collect::tlm::send::joined`].

use std::{
    collections::HashMap,
    fmt::Debug,
    sync::RwLock,
    thread::{self, ThreadId},
};
use thread_local_collect::{
    test_support::assert_eq_and_println,
    tlm::send::joined::{Control, Holder},
};

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

thread_local! {
    static MY_TL: Holder<AccValue> = Holder::new();
}

const NTHREADS: usize = 5;

#[test]
fn test() {
    main()
}

fn main() {
    let mut control = Control::new(&MY_TL, HashMap::new, op, op_r);

    {
        control.send_data((1, Foo("a".to_owned())));
        control.send_data((2, Foo("b".to_owned())));
    }

    let tid_own = thread::current().id();
    let map_own = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);

    // This is directly defined as a reference to prevent the move closure below from moving the
    // `spawned_tids` value. The closure has to be `move` because it needs to own `i`.
    let spawned_tids = &RwLock::new(vec![thread::current().id(); NTHREADS]);

    thread::scope(|s| {
        let hs = (0..NTHREADS)
            .map(|i| {
                let control = &control;
                s.spawn({
                    move || {
                        let si = i.to_string();

                        let mut lock = spawned_tids.write().unwrap();
                        lock[i] = thread::current().id();
                        drop(lock);

                        control.send_data((1, Foo("a".to_owned() + &si)));
                        control.send_data((2, Foo("b".to_owned() + &si)));
                    }
                })
            })
            .collect::<Vec<_>>();

        hs.into_iter().for_each(|h| h.join().unwrap());

        println!("after hs join: {:?}", control);
    });

    {
        let spawned_tids = spawned_tids.try_read().unwrap();
        let maps = (0..NTHREADS)
            .map(|i| {
                let map_i = HashMap::from([
                    (1, Foo("a".to_owned() + &i.to_string())),
                    (2, Foo("b".to_owned() + &i.to_string())),
                ]);
                let tid_i = spawned_tids[i];
                (tid_i, map_i)
            })
            .collect::<Vec<_>>();
        let mut map = maps.into_iter().collect::<HashMap<_, _>>();
        map.insert(tid_own, map_own);

        {
            let acc = control.drain_tls();
            assert_eq_and_println(&acc, &map, "Accumulator check");
        }

        // drain_tls again
        {
            let acc = control.drain_tls();
            assert_eq_and_println(&acc, &HashMap::new(), "empty accumulatore expected");
        }
    }
}
