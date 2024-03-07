//! This module supports the creation of tests and examples.

use std::{
    backtrace::Backtrace,
    process::abort,
    sync::atomic::{AtomicU64, Ordering},
};

/// Supports the coordination between threads through the waiting for gates to be opened.
/// A thread blocks and waits iff the gate it waits on is not open. Gate numbers may range
/// from 0 to 63.
pub struct ThreadGater {
    open_gates: AtomicU64,
}

fn validate_gate(gate: u8) {
    if gate > 63 {
        println!("FATAL: gate number {gate} must be between 0 and 63");
        let trace = Backtrace::force_capture();
        println!("backtrace:\n{trace}");
        abort();
    }
}

impl ThreadGater {
    pub fn new() -> Self {
        Self {
            open_gates: AtomicU64::new(0),
        }
    }

    /// Wait until `gate` is open.
    pub fn wait_for(&self, gate: u8) {
        validate_gate(gate);
        let gate_mask = 1u64 << gate;

        while self.open_gates.load(Ordering::Relaxed) & gate_mask == 0 {
            std::hint::spin_loop();
        }
    }

    /// Open `gate`.
    pub fn open(&self, gate: u8) {
        validate_gate(gate);
        let gate_mask = 1u64 << gate;

        self.open_gates.fetch_or(gate_mask, Ordering::Relaxed);
    }
}
