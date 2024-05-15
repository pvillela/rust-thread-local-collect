//! Variant of module [`crate::tlm::joined`] with a `send` API similar to that of [`crate::tlcr`] sub-modules.

use super::control_send::{ControlSendG, WithTakeTls};
use crate::tlm::joined::{Control as ControlOrig, Holder as HolderOrig, P as POrig};

pub use super::control_send::NothingAccumulatedError;

pub type Control<T, U> = ControlSendG<POrig<U, Option<U>>, T, U>;

impl<T, U> WithTakeTls<POrig<U, Option<U>>, U> for Control<T, U>
where
    U: 'static,
{
    fn take_tls(control: &ControlOrig<U, Option<U>>) {
        control.take_own_tl();
    }
}

pub type Holder<U> = HolderOrig<U, Option<U>>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::test_support::assert_eq_and_println;
    use std::{
        collections::HashMap,
        fmt::Debug,
        sync::Mutex,
        thread::{self, ThreadId},
    };

    #[derive(Debug, Clone, PartialEq)]
    struct Foo(String);

    type Data = (u32, Foo);

    type AccValue = HashMap<ThreadId, HashMap<u32, Foo>>;

    thread_local! {
        static MY_TL: Holder<AccValue> = Holder::new();
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
    fn own_thread_and_explicit_joins() {
        let mut control = Control::new(&MY_TL, HashMap::new, op, op_r);

        control.send_data((1, Foo("a".to_owned())));
        control.send_data((2, Foo("b".to_owned())));

        let own_tid = thread::current().id();

        let map_own = HashMap::from([(1, Foo("a".to_owned())), (2, Foo("b".to_owned()))]);
        let map = Mutex::new(HashMap::from([(own_tid, map_own)]));

        thread::scope(|s| {
            let hs = (0..2).map(|i| {
                let value1 = Foo("a".to_owned() + &i.to_string());
                let value2 = Foo("a".to_owned() + &i.to_string());
                let map_i = HashMap::from([(1, value1.clone()), (2, value2.clone())]);

                s.spawn(|| {
                    control.send_data((1, value1));
                    control.send_data((2, value2));

                    let spawned_tid = thread::current().id();
                    let mut lock = map.lock().unwrap();
                    lock.insert(spawned_tid, map_i);
                    drop(lock);
                })
            });
            hs.for_each(|h| h.join().unwrap());
        });

        let map = map.lock().unwrap();

        // Get accumulated value.
        let acc = control.drain_tls();
        assert_eq_and_println(&acc.unwrap(), &map, "drain_tls");

        // Repeat.
        let acc = control.drain_tls();
        assert_eq_and_println(&acc, &Err(NothingAccumulatedError), "2nd drain_tls");
    }
}
