#![cfg(feature = "tlcr")]

//! This module is supported on **`feature="tlcr"`** only.

use std::cell::Cell;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use thread_local::ThreadLocal;

#[cfg(test)]
mod test {
    use super::foo;

    #[test]
    fn test() {
        foo();
    }
}

pub fn foo() {
    let tls = Arc::new(ThreadLocal::new());

    // Create a bunch of threads to do stuff
    for _ in 0..5 {
        let h1 = {
            let tls2 = tls.clone();
            thread::spawn(move || {
                // Increment a counter to count some event...
                println!("h1 before assignment: {tls2:?}");
                let cell = tls2.get_or(|| Cell::new(0));
                cell.set(cell.get() + 1);
                println!("h1 after assignment: {tls2:?}");
                thread::sleep(Duration::from_millis(1));
            })
        };

        let h2 = {
            let tls2 = tls.clone();
            thread::spawn(move || {
                println!("h2 before assignment: {tls2:?}");
                // Increment a counter to count some event...
                let cell = tls2.get_or(|| Cell::new(0));
                cell.set(cell.get() + 1);
                println!("h2 after assignment: {tls2:?}");
            })
        };

        h2.join().unwrap();
        h1.join().unwrap();
    }

    let tls = Arc::try_unwrap(tls).unwrap();
    tls.into_iter().for_each(|x| println!("{x:?}"));
    assert!(false);
}
