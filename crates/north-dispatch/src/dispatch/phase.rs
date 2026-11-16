//! The phase a dispatch round is in, which the round's own limit names when it ends a round that outlived it.
use std::sync::Mutex;

static CURRENT: Mutex<&str> = Mutex::new("start");

pub fn enter(phase: &'static str) {
    if let Ok(mut current) = CURRENT.lock() {
        *current = phase;
    }
}

pub fn current() -> &'static str {
    CURRENT.lock().map_or("unknown", |current| *current)
}
