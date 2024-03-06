#![cfg(test)]

use std::sync::atomic::{AtomicBool, Ordering};

pub struct TrafficSignal(AtomicBool);

impl TrafficSignal {
    pub fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    pub fn continue_if_green(&self) {
        while !self.0.load(Ordering::Relaxed) {
            std::hint::spin_loop();
        }
    }

    pub fn set_green(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}
