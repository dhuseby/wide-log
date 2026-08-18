//! Shared helpers for the duration-unit integration tests.
//!
//! Each `wide_log!` invocation exports the same set of `#[macro_export]`
//! macros (`wl_set!`, `info!`, etc.), so multiple invocations cannot live
//! in the same test crate. The per-unit tests each live in their own test
//! file and import these helpers via `mod common;`.

use std::sync::{Arc, Mutex};

use sonic_rs::JsonValueTrait;
use wide_log::Key;

pub type CaptureSlot = Arc<Mutex<Option<String>>>;

#[allow(clippy::type_complexity)]
pub fn make_capture<K: Key>() -> (
    CaptureSlot,
    impl FnOnce(&wide_log::WideEvent<K>) + Send + 'static,
) {
    let slot: CaptureSlot = Arc::new(Mutex::new(None));
    let s = slot.clone();
    let emit = move |we: &wide_log::WideEvent<K>| {
        *s.lock().unwrap() = Some(we.to_json().unwrap());
    };
    (slot, emit)
}

pub fn parse(slot: &CaptureSlot) -> sonic_rs::Value {
    let json = slot.lock().unwrap().clone().unwrap();
    sonic_rs::from_str(&json).unwrap()
}

pub fn as_f64(parsed: &sonic_rs::Value, path: &[&str]) -> f64 {
    let mut cur = parsed;
    for seg in path {
        cur = &cur[*seg];
    }
    cur.as_f64()
        .unwrap_or_else(|| panic!("expected f64 at {}, got {:?}", path.join("."), cur))
}

/// Sleep for 20 milliseconds and return the actual elapsed duration
/// measured around the sleep. Tests use this delta to compute tolerant
/// bounds so they do not flake under scheduler jitter.
pub fn sleep_20ms() -> std::time::Duration {
    let start = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(20));
    start.elapsed()
}
