//! This module supports the collection and aggregation of the values of a designated thread-local variable
//! across threads (see package [overview and core concepts](super)). The following features and constraints apply ...
//! - The designated thread-local variable may be defined and used in the thread responsible for
//! collection/aggregation.
//! - The values of linked thread-local variables are collected and aggregated into the [Control] object's
//! accumulated value when the thread-local variables are dropped following thread termination.
//! - The [`Control`] object's collection/aggregation function is UNSAFE unless executed after all participating
//! threads, other than the thread responsible for collection/aggregation, have
//! terminated and EXPLICITLY joined, directly or indirectly, into the thread responsible for collection.
//! - Implicit joins by scoped threads are NOT correctly handled as the aggregation relies on the destructors
//! of thread-local variables and such a destructor is not guaranteed to have executed at the point of the
//! implicit join of a scoped thread.
//!
//! ## Usage pattern
//!
//! Here's an outline of how this little framework can be used:
//!
//! ```rust
//! use std::{
//!     ops::Deref,
//!     thread::{self, ThreadId},
//! };
//! use thread_local_collect::joined::{Control, Holder, HolderLocalKey};
//!
//! // Define your data type, e.g.:
//! type Data = i32;
//!
//! // Define your accumulated value type.
//! type AccValue = i32;
//!
//! // Define your thread-local:
//! thread_local! {
//!     static MY_TL: Holder<Data, AccValue> = Holder::new(|| 0);
//! }
//!
//! // Define your accumulation operation.
//! fn op(data: Data, acc: &mut AccValue, _: ThreadId) {
//!     *acc += data;
//! }
//!
//! // Create a function to update the thread-local value:
//! fn update_tl(value: Data, control: &Control<Data, AccValue>) {
//!     MY_TL.ensure_linked(control);
//!     MY_TL.with_data_mut(|data| {
//!         *data = value;
//!     }).unwrap();
//! }
//!
//! fn main() {
//!     let control = Control::new(0, op);
//!
//!     update_tl(1, &control);
//!
//!     thread::scope(|s| {
//!         let h = s.spawn(|| {
//!             update_tl(10, &control);
//!         });
//!         h.join().unwrap();
//!     });
//!
//!     {
//!         // Take and accumulate the thread-local values.
//!         // SAFETY: Call this after all other threads registered with `control` have been joined.
//!         unsafe { control.take_tls() };
//!
//!         // Different ways to print the accumulated value
//!
//!         println!("accumulated={}", control.acc().deref());
//!
//!         let acc = control.acc();
//!         println!("accumulated={}", acc.deref());
//!         drop(acc);
//!
//!         control.with_acc(|acc| println!("accumulated={}", acc));
//!
//!         let acc = control.clone_acc();
//!         println!("accumulated={}", acc);
//!
//!         let acc = control.take_acc(0);
//!         println!("accumulated={}", acc);
//!     }
//! }
//! ```
//!
//! ## Other examples
//!
//! See another example at [`examples/joined_map_accumulator.rs`](https://github.com/pvillela/rust-thread-local-collect/blob/main/examples/joined_map_accumulator.rs).

use crate::common::{
    ControlG, CoreParam, CtrlStateG, CtrlStateParam, CtrlStateWithNode, GDataParam, HolderG,
    HolderNotLinkedError, New, NodeParam, SubStateParam, UseCtrlStateGDefault, WithNode,
};
use std::{
    cell::RefCell,
    marker::PhantomData,
    mem::replace,
    ops::DerefMut,
    thread::{self, LocalKey, ThreadId},
};

//=================
// Core implementation based on common module

/// Parameter bundle that enables specialization of the common generic structs for this module.
#[derive(Debug)]
pub struct P<T, U> {
    own_tl_addr: Option<usize>,
    tid: ThreadId,
    _t: PhantomData<T>,
    _u: PhantomData<U>,
}

impl<T, U> CoreParam for P<T, U> {
    type Dat = T;
    type Acc = U;
}

impl<T, U> NodeParam for P<T, U> {
    type Node = usize;
}

impl<T, U> SubStateParam for P<T, U> {
    type SubState = Self;
}

impl<T, U> UseCtrlStateGDefault for P<T, U> {}

impl<T, U> GDataParam for P<T, U> {
    type GData = RefCell<T>;
}

impl<T, U> New<P<T, U>> for P<T, U> {
    type Arg = ();

    fn new(_: ()) -> P<T, U> {
        Self {
            own_tl_addr: None,
            tid: thread::current().id(),
            _t: PhantomData,
            _u: PhantomData,
        }
    }
}

type CtrlState<T, U> = CtrlStateG<P<T, U>>;

impl<T, U> CtrlStateParam for P<T, U> {
    type CtrlState = CtrlState<T, U>;
}

impl<T, U> CtrlStateWithNode<P<T, U>> for CtrlState<T, U> {
    fn register_node(&mut self, node: usize, tid: ThreadId) {
        if tid == self.s.tid {
            self.s.own_tl_addr = Some(node);
        }
    }
}

fn addr_of_tl<H>(tl: &LocalKey<H>) -> usize {
    let tl_ptr: *const LocalKey<H> = tl;
    tl_ptr as usize
}

unsafe fn tl_from_addr<H>(addr: usize) -> &'static LocalKey<H> {
    &*(addr as *const LocalKey<H>)
}

/// Specialization of [`ControlG`] for this module.
/// Controls the collection and accumulation of thread-local values linked to this object.
///
/// `T` is the type of the thread-local values and `U` is the type of the accumulated value.
/// The data values are held in thread-locals of type [`Holder<T, U>`].
pub type Control<T, U> = ControlG<P<T, U>, WithNode>;

impl<T, U> Control<T, U>
where
    T: 'static,
    U: 'static,
{
    /// This method takes the value of the designated thread-local variable in the thread responsible for
    /// collection/aggregation, if that variable is used, and aggregates that value
    /// with this object's accumulator, replacing that value with the evaluation of the `make_data` function
    /// passed to [`Holder::new`].
    ///
    /// # Safety
    /// This function can be called safely provided that:
    /// - All threads other than the one where this method is called have terminaged and been EXPLICITLY joined,
    /// directly or indirectly.
    ///
    /// The above condition establishes a proper "happens-before" relationship for all explicitly joined threads.
    ///
    /// When called safely, as indicated above, all linked thread-local variables corresponding to explicitly
    /// joined threads will have been dropped (and their values will have been accumulated) at the time this method
    /// is called. At that point, the only possible remaining linked thread-local variable would be associated with
    /// the thread where this method was called, so there can be no race condition.
    ///
    /// Panics if `self`'s mutex is poisoned.
    pub unsafe fn take_tls(&self) {
        let mut guard = self.lock();
        // Need explicit deref_mut to avoid compilation error in for loop.
        let state = guard.deref_mut();
        if let Some(addr) = state.s.own_tl_addr {
            // Safety: provided that:
            // - All other threads have terminaged and been explicitly joined, directly or indirectly.
            //
            // The above condition establishes a proper "happens-before" relationship for all explicitly joined threads,
            // and the only possible remaining activity would be [`HolderG`] drop method execution on the thread that
            // calls this method. But that drop method can't be executed concurrently with this one.
            let tl: &LocalKey<Holder<T, U>> = tl_from_addr(addr);
            tl.with(|h| {
                let mut data_guard = h.data_guard();
                let data = replace(data_guard.deref_mut(), (h.make_data)());
                log::trace!("`take_tls`: executing `op`");
                (self.op)(data, &mut state.acc, thread::current().id());
            });
        }
    }

    fn node(&self) -> usize {
        addr_of_tl(self.tl)
    }

    pub fn with_data<V>(&self, f: impl FnOnce(&T) -> V) -> Result<V, HolderNotLinkedError> {
        self.with_data_node(f, Self::node)
    }

    pub fn with_data_mut<V>(&self, f: impl FnOnce(&mut T) -> V) -> Result<V, HolderNotLinkedError> {
        self.with_data_mut_node(f, Self::node)
    }
}

/// Specialization of [`HolderG`] for this module.
/// Holds thread-local data of type `T` and a smart pointer to a [`Control<T, U>`], enabling the linkage of
/// the held data with the control object.
pub type Holder<T, U> = HolderG<P<T, U>, WithNode>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Control, Holder};

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

    type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

    thread_local! {
        static MY_TL: Holder<Data, AccValue> = Holder::new(HashMap::new);
    }

    fn insert_tl_entry(k: u32, v: Foo, control: &Control<Data, AccValue>) {
        control.with_data_mut(|data| data.insert(k, v)).unwrap();
    }

    fn op(data: HashMap<u32, Foo>, acc: &mut AccValue, tid: ThreadId) {
        acc.entry(tid).or_default();
        for (k, v) in data {
            acc.get_mut(&tid).unwrap().insert(k, v.clone());
        }
    }

    fn assert_tl(other: &Data, msg: &str, control: &Control<Data, AccValue>) {
        control
            .with_data(|map| {
                assert_eq!(map, other, "{msg}");
            })
            .unwrap();
    }

    #[test]
    fn explicit_joins_no_take_tls() {
        // These are directly defined as references to prevent the move closure below from moving
        // `control` and `spawned_tids`values. The closure has to be `move` because it needs to own `i`.
        let control = &Control::new(&MY_TL, HashMap::new(), op);
        let spawned_tids = &RwLock::new(vec![thread::current().id(), thread::current().id()]);

        thread::scope(|s| {
            let hs = (0..2)
                .map(|i| {
                    s.spawn({
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

            {
                thread::sleep(Duration::from_millis(50));
                let spawned_tids = spawned_tids.try_read().unwrap();
                println!("spawned_tid={:?}", spawned_tids);
            }

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
                assert!(acc.eq(&map), "Accumulator check: acc={acc:?}, map={map:?}");
            }
        }
    }

    #[test]
    fn own_thread_and_explicit_joins() {
        let control = Control::new(&MY_TL, HashMap::new(), op);

        let own_tid = thread::current().id();
        println!("main_tid={:?}", own_tid);
        let spawned_tids = RwLock::new(Vec::<ThreadId>::new());

        {
            insert_tl_entry(1, Foo("a".to_owned()), &control);
            insert_tl_entry(2, Foo("b".to_owned()), &control);

            let other = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
            assert_tl(&other, "After main thread inserts", &control);
        }

        thread::sleep(Duration::from_millis(100));

        let map_own = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
        let mut map = HashMap::from([(own_tid, map_own)]);

        for i in 0..100 {
            let spawned_tids = &spawned_tids;
            let control = &control;
            let value1 = Foo("a".to_owned() + &i.to_string());
            let value2 = Foo("a".to_owned() + &i.to_string());
            let map_i = &HashMap::from([(1, value1.clone()), (2, value2.clone())]);

            thread::scope(|s| {
                let h = s.spawn(|| {
                    let spawned_tid = thread::current().id();
                    let mut lock = spawned_tids.write().unwrap();
                    lock.push(spawned_tid);
                    drop(lock);

                    insert_tl_entry(1, value1.clone(), control);
                    let other = HashMap::from([(1, value1)]);
                    assert_tl(&other, "After 1st insert", control);

                    insert_tl_entry(2, value2, control);
                    assert_tl(map_i, "After 2nd insert", control);
                });
                h.join().unwrap();
            });

            {
                let lock = spawned_tids.read().unwrap();
                let spawned_tid = lock.last().unwrap();
                map.insert(*spawned_tid, map_i.clone());

                // Safety: called after all other threads explicitly joined.
                unsafe { control.take_tls() };

                let acc = control.acc();
                assert_eq!(acc.deref(), &map, "Accumulator check on iteration {}", i);
            }
        }
    }
}
