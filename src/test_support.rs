//! This module supports the creation of tests and examples.

use std::sync::atomic::{AtomicI32, Ordering};

/// Supports the coordination between threads through the waiting for gates to be opened.
/// At any point in time, all gates up to a `highest_open_gate` are open and all gates
/// above it are closed. A thread can wait on a specific numbered gate or on the current
/// gate. A thread blocks and waits iff the gate it waits on is not open.
pub struct ThreadGater {
    curr_gate: AtomicI32,
    highest_open_gate: AtomicI32,
}

impl ThreadGater {
    pub fn new() -> Self {
        Self {
            curr_gate: AtomicI32::new(-1),
            highest_open_gate: AtomicI32::new(-1),
        }
    }

    /// Wait until `gate` is open.
    pub fn wait_for(&self, gate: u32) {
        self.curr_gate.store(gate as i32, Ordering::Relaxed);
        while self.highest_open_gate.load(Ordering::Relaxed) < gate as i32 {
            std::hint::spin_loop();
        }
    }

    /// Increment the current gate and wait until it is open.
    pub fn wait(&self) {
        let prev_gate = self.curr_gate.fetch_add(1, Ordering::Relaxed);
        while self.highest_open_gate.load(Ordering::Relaxed) < prev_gate + 1 {
            std::hint::spin_loop();
        }
    }

    /// Set the highest open gate to `gate`.
    pub fn open(&self, gate: u32) {
        self.highest_open_gate
            .fetch_max(gate as i32, Ordering::Relaxed);
    }

    /// Increment the highest open gate.
    pub fn open_next(&self) {
        self.highest_open_gate.fetch_add(1, Ordering::Relaxed);
    }
}
