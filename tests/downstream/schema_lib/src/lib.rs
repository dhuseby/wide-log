//! Schema-owning library crate: the only crate in the downstream workspace
//! that invokes `wide_log!`.
//!
//! The generated items (`WideLogGuard`, `EventKey`, the hook shim) live at
//! this crate's root. The importer crates call the public functions below
//! so a guard from this schema can be active while the importers append
//! through the wide-log crate-root macros. Nested-guard tests construct
//! and drop this crate's generated `WideLogGuard` directly.

use std::sync::{Arc, Mutex};

#[cfg(feature = "tracing")]
#[allow(unused_imports)]
use wide_log::{debug, error, info, trace, warn};

use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": null,
    },
    "requests": counter!,
});

/// Runs `f` inside a wide-log guard from this crate's schema and returns
/// the emitted JSON line. The importer binaries call this so their
/// crate-root macro calls land on an event owned by this crate.
pub fn with_guard_emit_json<R>(f: impl FnOnce() -> R) -> String {
    let json = Arc::new(Mutex::new(String::new()));
    let sink = json.clone();
    let _guard = WideLogGuard::builder()
        .with_emit(move |ev| {
            if let Ok(line) = ev.to_json() {
                *sink.lock().unwrap() = line;
            }
        })
        .build();

    f();

    // The guard drops before the take: drop order sets duration,
    // timestamp, and the emit line the sink captured above.
    drop(_guard);

    std::mem::take(&mut *json.lock().unwrap())
}

/// Appends an entry through this crate's generated level macros (the
/// direct `CURRENT_EVENT` path, not the crate-root import path).
pub fn log_via_generated_macros(message: &str) {
    info!("schema-lib direct: {}", message);
}

/// Bumps the `requests` counter in the schema.
pub fn count_request() {
    wl_inc!("requests");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_guard_emits_one_json_line() {
        let json = with_guard_emit_json(|| {
            count_request();
            log_via_generated_macros("unit");
        });
        assert!(json.starts_with('{'), "emitted line is a JSON object");
        assert!(
            json.contains(r#""requests":1"#),
            "counter visible in {json}"
        );
    }
}
