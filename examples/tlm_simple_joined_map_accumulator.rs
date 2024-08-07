//! Example usage of [`thread_local_collect::tlm::simple_joined`].

use thread_local_collect::{
    dev_support::assert_eq_and_println,
    tlm::simple_joined::{Control, Holder},
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

type Data = HashMap<i32, Foo>;

type AccumulatorMap = HashMap<ThreadId, HashMap<i32, Foo>>;

thread_local! {
    static MY_TL: Holder<Data, AccumulatorMap> = Holder::new();
}

fn insert_tl_entry(k: i32, v: Foo, control: &Control<Data, AccumulatorMap>) {
    control.with_data_mut(|data| data.insert(k, v));
}

fn op(data: HashMap<i32, Foo>, acc: &mut AccumulatorMap, tid: ThreadId) {
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

fn assert_tl(other: &Data, msg: &str, control: &Control<Data, AccumulatorMap>) {
    control.with_data(|map| {
        assert_eq!(map, other, "{msg}");
    });
}

#[test]
fn test() {
    main();
}

fn main() {
    let control = Control::new(&MY_TL, HashMap::new(), HashMap::new, op);
    let spawned_tids = RwLock::new(vec![thread::current().id(), thread::current().id()]);

    thread::scope(|s| {
        let hs = (0..2)
            .map(|i| {
                s.spawn({
                    // These are to prevent the move closure from moving `control` and `spawned_tids`.
                    // The closure has to be `move` because it needs to own `i`.
                    let control = &control;
                    let spawned_tids = &spawned_tids;

                    move || {
                        let si = i.to_string();

                        let mut lock = spawned_tids.write().unwrap();
                        lock[i] = thread::current().id();
                        drop(lock);

                        insert_tl_entry(1, Foo("a".to_owned() + &si), control);

                        let other = HashMap::from([(1, Foo("a".to_owned() + &si))]);
                        assert_tl(&other, "After 1st insert", control);

                        insert_tl_entry(2, Foo("b".to_owned() + &si), control);

                        let other = HashMap::from([
                            (1, Foo("a".to_owned() + &si)),
                            (2, Foo("b".to_owned() + &si)),
                        ]);
                        assert_tl(&other, "After 2nd insert", control);
                    }
                })
            })
            .collect::<Vec<_>>();

        thread::sleep(Duration::from_millis(50));

        let spawned_tids = spawned_tids.try_read().unwrap();
        println!("spawned_tid={:?}", spawned_tids);

        hs.into_iter().for_each(|h| h.join().unwrap());

        println!("after hs join: {:?}", control);
    });

    {
        let spawned_tids = spawned_tids.try_read().unwrap();
        let map_0 = HashMap::from([(1, Foo("a0".to_owned())), (2, Foo("b0".to_owned()))]);
        let map_1 = HashMap::from([(1, Foo("a1".to_owned())), (2, Foo("b1".to_owned()))]);
        let map = HashMap::from([(spawned_tids[0], map_0), (spawned_tids[1], map_1)]);

        {
            let guard = control.acc();
            let acc = guard.deref();
            assert_eq_and_println(acc, &map, "Accumulator check: acc={acc:?}, map={map:?}");
        }
    }
}
