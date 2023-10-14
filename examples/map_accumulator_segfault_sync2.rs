//! This example reliably crashes with a setmentation fault or similar panic.

#[allow(unused)]
use env_logger;
#[allow(unused)]
use std::env::set_var;

use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{Mutex, OnceLock},
    thread::{self, ThreadId},
    time::Duration,
};
use thread_local_drop::{Control, Holder};

#[derive(Debug, Clone)]
struct Foo(String);

type Data = HashMap<u32, Foo>;

type AccumulatorMap = HashMap<ThreadId, HashMap<u32, Foo>>;

static ALLOW_UPDATES: Mutex<()> = Mutex::new(());

thread_local! {
    static MY_FOO_MAP: Holder<Data, AccumulatorMap> = Holder::new(HashMap::new);
}

static CONTROL: OnceLock<Control<HashMap<u32, Foo>, AccumulatorMap>> = OnceLock::new();

fn control() -> &'static Control<HashMap<u32, Foo>, AccumulatorMap> {
    CONTROL.get_or_init(|| Control::new(HashMap::new(), op))
}

fn insert_tl_entry(k: u32, v: Foo, control: &Control<Data, AccumulatorMap>) {
    let _lock = ALLOW_UPDATES.lock().unwrap();
    control.with_tl_mut(&MY_FOO_MAP, |data| {
        data.insert(k, v);
    });
}

fn op(data: HashMap<u32, Foo>, acc: &mut AccumulatorMap, tid: &ThreadId) {
    acc.entry(tid.clone()).or_insert_with(|| HashMap::new());
    for (k, v) in data {
        acc.get_mut(tid).unwrap().insert(k, v.clone());
    }
}

fn main() {
    // Set env variable value below to "trace" to see debug logs emitted by the library.
    // set_var("RUST_LOG", "trace");
    // _ = env_logger::try_init();

    const N_THREADS: u32 = 100;
    const N_REPEATS1: u32 = 100;
    const N_REPEATS2: u32 = 100;
    const N_REPEATS: u32 = N_REPEATS1 + N_REPEATS2;
    const SLEEP_MILLIS_THREAD: u64 = 1;
    const SLEEP_MILLIS_MAIN: u64 = 10;

    let f = || {
        let hs = (0..N_THREADS)
            .map(|_| {
                thread::spawn(move || {
                    for j in 0..N_REPEATS {
                        let v = Foo("a".to_owned() + &j.to_string());
                        // println!("{:?}: {j}->{:?}", thread::current().id(), v);
                        insert_tl_entry(j, v, control());
                        thread::sleep(Duration::from_millis(SLEEP_MILLIS_THREAD));
                    }
                })
            })
            .collect::<Vec<_>>();

        hs.into_iter().for_each(|h| h.join().unwrap());
    };

    let h = thread::spawn(f);

    for k in 0..N_REPEATS1 {
        thread::sleep(Duration::from_millis(SLEEP_MILLIS_MAIN));
        let _allow_updates = ALLOW_UPDATES.lock().unwrap();
        let mut control_lock = control().lock();
        control().ensure_tls_dropped(&mut control_lock);
        let acc = control().take_acc(&mut control_lock, HashMap::new());
        let len = format!("{:?}", acc).len();
        println!("k={k},len={len}; ");
    }

    h.join().unwrap();

    for k in 0..N_REPEATS2 {
        thread::sleep(Duration::from_millis(SLEEP_MILLIS_MAIN));
        let _allow_updates = ALLOW_UPDATES.lock().unwrap();
        let mut control_lock = control().lock();
        control().ensure_tls_dropped(&mut control_lock);
        let acc = control().take_acc(&mut control_lock, HashMap::new());
        let len = format!("{:?}", acc).len();
        println!("k={k},len={len}; ");
    }

    println!("The End")
}
