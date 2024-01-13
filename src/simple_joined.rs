//! Support for ensuring that destructors are run on thread-local variables after the threads terminate,
//! as well as support for accumulating the thread-local values using a binary operation.

pub use crate::common::HolderLocalKey;
use crate::common::{ControlS, ControlStateS, HolderS};
use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
    thread::{LocalKey, ThreadId},
};

pub type TrivialState<T, U> = ControlStateS<T, U, ()>;

impl<T, U> TrivialState<T, U> {
    fn ensure_tls_dropped(_state: &mut Self, _op: &(dyn Fn(T, &mut U, &ThreadId) + Send + Sync)) {}
}

pub type Control<T, U> = ControlS<TrivialState<T, U>>;

impl<T, U> Control<T, U>
where
    T: 'static,
    U: 'static,
{
    pub fn new(acc_base: U, op: impl Fn(T, &mut U, &ThreadId) + 'static + Send + Sync) -> Self {
        Control {
            state: Arc::new(Mutex::new(TrivialState::new(
                acc_base,
                TrivialState::ensure_tls_dropped,
            ))),
            op: Arc::new(op),
        }
    }
}

pub type Holder<T, U> = HolderS<RefCell<Option<T>>, TrivialState<T, U>>;

impl<T, U: 'static> Holder<T, U> {
    /// Creates a new `Holder` instance with a function to initialize the data.
    ///
    /// The `make_data` function will be called lazily when data is first accessed to initialize
    /// the inner data value.
    pub fn new(make_data: fn() -> T) -> Self {
        Self {
            data: RefCell::new(None),
            control: RefCell::new(None),
            make_data,
        }
    }
}

impl<T, U> HolderLocalKey<T, Control<T, U>> for LocalKey<Holder<T, U>> {
    /// Establishes link with control.
    fn init_control(&'static self, control: &Control<T, U>) {
        self.with(|h| h.init_control(&control, ()))
    }

    fn init_data(&'static self) {
        self.with(|h| h.init_data())
    }

    fn ensure_initialized(&'static self, control: &Control<T, U>) {
        self.with(|h| h.ensure_initialized(&control, ()))
    }

    /// Invokes `f` on data. Panics if data is [`None`].
    fn with_data<V>(&'static self, f: impl FnOnce(&T) -> V) -> V {
        self.with(|h| h.with_data(f))
    }

    /// Invokes `f` on data. Panics if data is [`None`].
    fn with_data_mut<V>(&'static self, f: impl FnOnce(&mut T) -> V) -> V {
        self.with(|h| h.with_data_mut(f))
    }
}

#[cfg(test)]
mod tests {
    use crate::common::ControlState;

    use super::{Control, Holder, HolderLocalKey};

    use std::{
        collections::HashMap,
        fmt::Debug,
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

    #[test]
    fn test_all() {
        let control = Control::new(HashMap::new(), op);
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
                            assert_tl(&other, "After 1st insert");

                            insert_tl_entry(2, Foo("b".to_owned() + &si), control);

                            let other = HashMap::from([
                                (1, Foo("a".to_owned() + &si)),
                                (2, Foo("b".to_owned() + &si)),
                            ]);
                            assert_tl(&other, "After 2nd insert");
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
            let map = HashMap::from([
                (spawned_tids[0].clone(), map_0),
                (spawned_tids[1].clone(), map_1),
            ]);

            {
                let lock = control.lock();
                let acc = lock.acc();
                assert!(acc.eq(&map), "Accumulator check: acc={acc:?}, map={map:?}");
            }
        }
    }
}
