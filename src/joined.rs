//! Support for ensuring that destructors are run on thread-local variables after the threads terminate,
//! as well as support for accumulating the thread-local values using a binary operation.

use crate::common::{ControlBase, ControlInner, ControlPart, ControlS, HolderBase};
use std::{
    cell::{Ref, RefCell, RefMut},
    sync::{Arc, Mutex, MutexGuard},
    thread::{LocalKey, ThreadId},
};

pub type Control<T, U> = ControlS<T, U, U>;

// #[derive(Debug)]
pub struct Holder<T, U>
where
    T: 'static,
{
    data: RefCell<Option<T>>,
    control: RefCell<Option<Control<T, U>>>,
    make_data: fn() -> T,
}

pub trait HolderLocalKey<T, U> {
    /// Establishes link with control.
    fn init_control(&'static self, control: &Control<T, U>);

    fn init_data(&'static self);

    fn ensure_initialized(&'static self, control: &Control<T, U>);

    /// Invokes `f` on data. Panics if data is [`None`].
    fn with_data<V>(&'static self, f: impl FnOnce(&T) -> V) -> V;

    /// Invokes `f` on data. Panics if data is [`None`].
    fn with_data_mut<V>(&'static self, f: impl FnOnce(&mut T) -> V) -> V;
}

impl<U> ControlInner<U> for U {
    fn acc(&mut self) -> &mut U {
        self
    }
}

impl<T, U> ControlBase for Control<T, U>
where
    T: 'static,
    U: 'static,
{
    fn ensure_tls_dropped(&self, _lock: &mut Self::Lock<'_>) {
        // no-op
    }

    fn deregister_thread(&self, _lock: &mut Self::Lock<'_>, _tid: &ThreadId) {
        // no-op
    }
}

impl<T, U> Control<T, U>
where
    T: 'static,
    U: 'static,
{
    pub fn new(acc_base: U, op: impl Fn(T, &mut U, &ThreadId) + 'static + Send + Sync) -> Self {
        Control {
            inner: Arc::new(Mutex::new(acc_base)),
            op: Arc::new(op),
        }
    }

    /// Acquires a lock for use by public `Control` methods that require its internal Mutex to be locked.
    ///
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub fn lock(&self) -> MutexGuard<'_, U> {
        ControlPart::lock(self)
    }

    /// Provides access to the accumulated value in the [Control] struct.
    ///
    /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub fn with_acc<V>(&self, lock: &mut MutexGuard<'_, U>, f: impl FnOnce(&U) -> V) -> V {
        ControlPart::with_acc(self, lock, f)
    }

    /// Returns the accumulated value in the [Control] struct, using a value of the same type to replace
    /// the existing accumulated value.
    ///
    /// The [`lock`](Self::lock) method can be used to obtain the `lock` argument.
    /// An cquired lock can be used with multiple method calls and droped after the last call.
    /// As with any lock, the caller should ensure the lock is dropped as soon as it is no longer needed.
    pub fn take_acc(&self, lock: &mut MutexGuard<'_, U>, replacement: U) -> U {
        ControlPart::take_acc(self, lock, replacement)
    }

    pub fn ensure_tls_dropped(&self, lock: &mut MutexGuard<'_, U>) {
        ControlBase::ensure_tls_dropped(self, lock)
    }
}

impl<T, U: 'static> HolderBase for Holder<T, U> {
    type Dat = T;
    type Ctrl = Control<T, U>;
    type Guard<'a> = RefMut<'a, Option<T>>;

    fn control(&self) -> Ref<'_, Option<Self::Ctrl>> {
        self.control.borrow()
    }

    fn make_data(&self) -> T {
        (self.make_data)()
    }

    fn data_guard(&self) -> Self::Guard<'_> {
        self.data.borrow_mut()
    }

    fn init_control(&self, control: &Self::Ctrl) {
        let mut ctrl = self.control.borrow_mut();
        *ctrl = Some(control.clone());
    }
}

impl<T: Clone, U: 'static + Clone> Holder<T, U> {
    pub const fn new(make_data: fn() -> T) -> Self {
        Self {
            data: RefCell::new(None),
            control: RefCell::new(None),
            make_data,
        }
    }

    pub fn data_guard(&self) -> RefMut<'_, Option<T>> {
        HolderBase::data_guard(self)
    }

    /// Establishes link with control.
    pub fn init_control(&self, control: &Control<T, U>) {
        HolderBase::init_control(self, control)
    }

    pub fn init_data(&self) {
        HolderBase::init_data(self)
    }

    pub fn ensure_initialized(&self, control: &Control<T, U>) {
        HolderBase::ensure_initialized(self, &control)
    }

    /// Invokes `f` on data. Panics if data is [`None`].
    pub fn with_data<V>(&self, f: impl FnOnce(&T) -> V) -> V {
        HolderBase::with_data(self, f)
    }

    /// Invokes `f` on data. Panics if data is [`None`].
    pub fn with_data_mut<V>(&self, f: impl FnOnce(&mut T) -> V) -> V {
        HolderBase::with_data_mut(self, f)
    }
}

impl<T: Clone, U: Clone> HolderLocalKey<T, U> for LocalKey<Holder<T, U>> {
    /// Establishes link with control.
    fn init_control(&'static self, control: &Control<T, U>) {
        self.with(|h| h.init_control(&control))
    }

    fn init_data(&'static self) {
        self.with(|h| h.init_data())
    }

    fn ensure_initialized(&'static self, control: &Control<T, U>) {
        self.with(|h| h.ensure_initialized(&control))
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

                            let mut lock = spawned_tids.try_write().unwrap();
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
                let acc = &lock;
                assert!(acc.eq(&map), "Accumulator check: acc={acc:?}, map={map:?}");
            }
        }
    }
}
