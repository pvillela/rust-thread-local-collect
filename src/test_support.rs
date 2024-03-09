//! This module supports the creation of tests and examples.

use std::{
    backtrace::Backtrace,
    fmt::Debug,
    process::abort,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

/// Supports the coordination between threads through the waiting for gates to be opened.
/// A thread blocks and waits iff the gate it waits on is not open. Gate numbers may range
/// from 0 to 63. The default timeout is 1 second.
pub struct ThreadGater {
    name: String,
    open_gates: AtomicU64,
    timeout: AtomicU64,
}

fn abort_with_backtrace() {
    let trace = Backtrace::force_capture();
    println!("backtrace:\n{trace}");
    abort();
}

fn validate_gate(gate: u8) {
    if gate > 63 {
        println!("FATAL: ThreadGater gate number {gate} must be between 0 and 63");
        abort_with_backtrace();
    }
}

impl ThreadGater {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            open_gates: AtomicU64::new(0),
            timeout: AtomicU64::new(1000),
        }
    }

    fn check_timeout(&self, start_time: Instant, gate: u8) {
        if Instant::now().duration_since(start_time) >= self.timeout() {
            println!(
                "FATAL: ThreadGater '{}' timed-out at gate {}",
                self.name, gate
            );
            abort_with_backtrace();
        }
    }

    /// Wait until `gate` is open or it times-out.
    pub fn wait_for(&self, gate: u8) {
        validate_gate(gate);
        let start_time = Instant::now();
        let gate_mask = 1u64 << gate;
        while self.open_gates.load(Ordering::Relaxed) & gate_mask == 0 {
            self.check_timeout(start_time, gate);
            std::hint::spin_loop();
        }
    }

    /// Open `gate`.
    pub fn open(&self, gate: u8) {
        validate_gate(gate);
        let gate_mask = 1u64 << gate;

        self.open_gates.fetch_or(gate_mask, Ordering::Relaxed);
    }

    /// Returns the configured `timeout`.
    pub fn timeout(&self) -> Duration {
        let millis = self.timeout.load(Ordering::Relaxed);
        Duration::from_millis(millis)
    }

    /// Sets a non-default `timeout`, with a minimum duration of 1 millisecond.
    /// The default timeout is 1 second.
    pub fn set_timeout(&self, timeout: Duration) {
        let millis = timeout.as_millis() as u64;
        let millis = millis.max(1);
        self.timeout.store(millis, Ordering::Relaxed);
    }
}

/// Prints `msg`, `left`, `right`, and then
/// asserts equality of `left` and `right` with`msg` as the assert message.
pub fn assert_eq_and_println<T: PartialEq + Debug>(left: T, right: T, msg: &str) {
    println!("{msg} - left={left:?}; right={right:?}");
    assert_eq!(left, right, "{msg}");
}
