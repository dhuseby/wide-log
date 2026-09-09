//! Shared helpers for the duration-unit integration tests.
//!
//! Each `wide_log!` invocation exports the same set of `#[macro_export]`
//! macros (`wl_set!`, `info!`, etc.), so multiple invocations cannot live
//! in the same test crate. The per-unit tests each live in their own test
//! file and import these helpers via `mod common;`. Not every test file
//! uses every helper, so unused items are tolerated here.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use sonic_rs::JsonValueTrait;
use wide_log::Key;

#[cfg(feature = "tracing")]
#[allow(unused_imports)]
pub use wide_log::{debug, error, info, trace, warn};

// The capture layer is generated at the `wide_log!` invocation site —
// the crate root of each test crate that includes this module — so it
// resolves as `crate::WideLogCaptureLayer` from here.
#[cfg(feature = "tracing")]
#[allow(unused_imports)]
pub use crate::WideLogCaptureLayer;

/// Installs a thread-local default subscriber with the capture layer
/// for tests that assert on the `log` array while the `tracing`
/// feature is on: unqualified log calls resolve to the re-exported
/// `tracing` macros and are captured into the active wide event by
/// this layer. Returns `Some(guard)` under the feature (the guard
/// keeps the subscriber installed for the enclosing scope) and
/// `None` without it (the generated level macros append to the
/// active event directly, so no subscriber is needed).
pub fn capture_subscriber() -> Option<tracing::subscriber::DefaultGuard> {
    #[cfg(feature = "tracing")]
    {
        use tracing_subscriber::prelude::*;
        Some(tracing::subscriber::set_default(
            tracing_subscriber::registry().with(crate::WideLogCaptureLayer::new()),
        ))
    }
    #[cfg(not(feature = "tracing"))]
    {
        None
    }
}

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
