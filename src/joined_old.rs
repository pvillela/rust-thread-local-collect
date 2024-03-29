//! This is an older version of [`crate::joined`] that unnecessarily registers all thread-local variables
//! on a map in the control state.
//!
//! This module supports the collection and aggregation of the values of a designated thread-local variable
//! across threads (see package [overview and core concepts](super)). The following features and constraints apply ...
//! - The designated thread-local variable may be defined and used in the thread responsible for
//! collection/aggregation.
//! - The values of linked thread-local variables are collected and aggregated into the [Control] object's
//! accumulated value when the thread-local variables are dropped following thread termination.
//! - The [`Control`] object's collection/aggregation function is UNSAFE unless executed after all participating
//! threads, other than the thread responsible for collection/aggregation, have
//! terminated and EXPLICITLY joined, directly or indirectly, into the thread responsible for collection.

pub use crate::common::HolderLocalKey;

use crate::common::{
    ControlG, CoreParam, GDataParam, HolderG, HolderNotLinkedError, NodeParam, SubStateParam, TmapD,
};
use std::{cell::RefCell, marker::PhantomData, mem::replace, ops::DerefMut, thread::LocalKey};

//=================
// Core implementation based on common module

#[doc(hidden)] // not needed here
/// Parameter bundle that enables specialization of the common generic structs for this module.
#[derive(Debug)]
pub struct P<T, U> {
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
    type SubState = TmapD<Self>;
}

impl<T, U> GDataParam for P<T, U> {
    type GData = RefCell<T>;
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
pub type Control<T, U> = ControlG<TmapD<P<T, U>>>;

#[doc(hidden)] // needed here in addition to module overall
impl<T, U> Control<T, U>
where
    T: 'static,
    U: 'static,
{
    /// This method takes the values of any remaining linked thread-local-variables and aggregates those values
    /// with this object's accumulator, replacing those values with the evaluation of the `make_data` function
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
        for (tid, addr) in state.s.tmap.iter() {
            log::trace!("executing `take_tls` for key={:?}", tid);
            // Safety: provided that:
            // - All other threads have terminaged and been explicitly joined, directly or indirectly.
            //
            // The above condition establishes a proper "happens-before" relationship for all explicitly joined threads,
            // and the only possible remaining activity would be [`HolderG`] drop method execution on the thread that
            // calls this method. But that drop method can't be executed concurrently with this one.
            let tl: &LocalKey<Holder<T, U>> = tl_from_addr(*addr);
            tl.with(|h| {
                let mut data_guard = h.data_guard();
                let data = replace(data_guard.deref_mut(), (h.make_data)());
                log::trace!("`take_tls`: executing `op`");
                (self.op)(data, &mut state.acc, *tid);
            });
        }
    }
}

/// Specialization of [`HolderG`] for this module.
/// Holds thread-local data of type `T` and a smart pointer to a [`Control<T, U>`], enabling the linkage of
/// the held data with the control object.
pub type Holder<T, U> = HolderG<TmapD<P<T, U>>>;

//=================
// Implementation of HolderLocalKey.

#[doc(hidden)] // needed here in addition to module overall
impl<T, U> HolderLocalKey<TmapD<P<T, U>>> for LocalKey<Holder<T, U>> {
    /// Ensures [`Holder`] is properly initialized by initializing it if not.
    fn ensure_linked(&'static self, control: &Control<T, U>) {
        self.with(|h| h.ensure_linked_node(control, addr_of_tl(self)))
    }

    /// Invokes `f` on [`Holder`] data. Panics if data is [`None`].
    fn with_data<V>(&'static self, f: impl FnOnce(&T) -> V) -> Result<V, HolderNotLinkedError> {
        self.with(|h| h.with_data(f))
    }

    /// Invokes `f` mutably on [`Holder`] data. Panics if data is [`None`].
    fn with_data_mut<V>(
        &'static self,
        f: impl FnOnce(&mut T) -> V,
    ) -> Result<V, HolderNotLinkedError> {
        self.with(|h| h.with_data_mut(f))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Control, Holder, HolderLocalKey};

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

    type AccumulatorMap = HashMap<ThreadId, HashMap<u32, Foo>>;

    thread_local! {
        static MY_FOO_MAP: Holder<Data, AccumulatorMap> = Holder::new(HashMap::new);
    }

    fn insert_tl_entry(k: u32, v: Foo, control: &Control<Data, AccumulatorMap>) {
        MY_FOO_MAP.ensure_linked(control);
        MY_FOO_MAP.with_data_mut(|data| data.insert(k, v)).unwrap();
    }

    fn op(data: HashMap<u32, Foo>, acc: &mut AccumulatorMap, tid: ThreadId) {
        acc.entry(tid).or_default();
        for (k, v) in data {
            acc.get_mut(&tid).unwrap().insert(k, v.clone());
        }
    }

    fn assert_tl(other: &Data, msg: &str) {
        MY_FOO_MAP
            .with_data(|map| {
                assert_eq!(map, other, "{msg}");
            })
            .unwrap();
    }

    #[test]
    fn explicit_joins_no_take_tls() {
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
        let control = Control::new(HashMap::new(), op);

        let own_tid = thread::current().id();
        println!("main_tid={:?}", own_tid);
        let spawned_tids = RwLock::new(Vec::<ThreadId>::new());

        {
            insert_tl_entry(1, Foo("a".to_owned()), &control);
            insert_tl_entry(2, Foo("b".to_owned()), &control);

            let other = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
            assert_tl(&other, "After main thread inserts");
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
                let h = s.spawn(move || {
                    let spawned_tid = thread::current().id();
                    let mut lock = spawned_tids.write().unwrap();
                    lock.push(spawned_tid);
                    drop(lock);

                    insert_tl_entry(1, value1.clone(), control);
                    let other = HashMap::from([(1, value1)]);
                    assert_tl(&other, "Before 1st insert");

                    insert_tl_entry(2, value2, control);
                    assert_tl(map_i, "Before 2nd insert");
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
