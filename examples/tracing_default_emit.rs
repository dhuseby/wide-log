//! The macro-generated `default_emit` under the `tracing` feature:
//! the finished event is serialized and emitted through
//! `::tracing::info!(target: "wide_log", event = %s)` — without a
//! user-supplied `with_emit` closure.
//!
//! Build with:
//!
//! ```text
//! cargo run --example tracing_default_emit --features tracing
//! ```
//!
//! The subscriber stack here is capture-only: `WideLogCaptureLayer`
//! routes every canonical `tracing` record into the wide event, and
//! no formatting layer is installed, so the subscriber prints
//! nothing. The guard therefore emits through the raw-JSON closure:
//! the stdout line is one bare JSON object with no timestamp or
//! level prefix.
//!
//! To instead see the emit-side record through a formatting layer
//! (timestamp, level, `wide_log` target), add
//! `tracing_subscriber::fmt::layer()` to the stack; the capture
//! layer skips the reserved `wide_log` target either way, so the
//! finished event never re-captures itself.

use wide_log::wide_log;

#[cfg(feature = "tracing")]
#[allow(unused_imports)]
use wide_log::{debug, error, info, trace, warn};

#[cfg(feature = "tracing")]
fn init_capture() {
    use tracing_subscriber::prelude::*;
    // Capture-only subscriber stack: the capture layer routes every
    // canonical tracing record (application and dependency crates)
    // into the active wide event, and no formatting layer is
    // installed, so the subscriber itself prints nothing.
    tracing_subscriber::registry()
        .with(crate::WideLogCaptureLayer::new())
        .init();
}

#[cfg(not(feature = "tracing"))]
fn init_capture() {}

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

#[cfg(feature = "tracing")]
fn raw_json_emit(ev: &wide_log::WideEvent<EventKey>) {
    // Raw JSON output: one bare JSON line, no timestamp or level
    // prefix. The capture-only subscriber prints nothing, so this
    // emit is the only thing that writes to stdout.
    if let Ok(json) = ev.to_json() {
        println!("{json}");
    }
}

fn main() {
    init_capture();

    // Under the `tracing` feature, emit through the raw-JSON closure
    // so guard drop prints the bare JSON line (the `default_emit`
    // routing goes to the silent capture-only subscriber). Without
    // the feature, `default_emit` writes the bare JSON line itself.
    #[cfg(feature = "tracing")]
    let _guard = WideLogGuard::builder().with_emit(raw_json_emit).build();
    #[cfg(not(feature = "tracing"))]
    let _guard = WideLogGuard::builder().build();

    wl_set!("service.name", "tracing-default");
    wl_inc!("requests");
    info!("request received");
    info!("request completed");

    // _guard drops → bare JSON line on stdout in both modes.
    drop(_guard);
}
