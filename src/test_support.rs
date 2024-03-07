//! This module supports the creation of tests and examples.

use std::sync::atomic::{AtomicI32, Ordering};

/// Supports the coordination between threads through the waiting for gates to be opened.
/// At any point in time, all gates up to a `highest_open_gate` are open and all gates
/// above it are closed. A thread blocks and waits iff the gate it waits on is not open.
pub struct ThreadGater {
    highest_open_gate: AtomicI32,
}

impl ThreadGater {
    pub fn new() -> Self {
        Self {
            highest_open_gate: AtomicI32::new(-1),
        }
    }

    /// Wait until `gate` is open.
    pub fn wait_for(&self, gate: u32) {
        while self.highest_open_gate.load(Ordering::Relaxed) < gate as i32 {
            std::hint::spin_loop();
        }
    }

    /// Set the highest open gate to `gate`.
    pub fn open(&self, gate: u32) {
        self.highest_open_gate
            .fetch_max(gate as i32, Ordering::Relaxed);
    }
}
