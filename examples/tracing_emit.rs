//! A custom emit function that routes the serialized wide event
//! through `::tracing::info!` when the `tracing` feature is off —
//! and demonstrates the raw-JSON pattern when the feature is on.
//!
//! With the feature **off**, this example installs a custom emit via
//! `with_emit`. The emit serializes the event to JSON and hands it to
//! `::tracing::info!(event = %json)`, so the emitted line on stdout is
//! wrapped in the tracing fmt envelope:
//!
//! ```text
//! 2026-07-17T16:01:26Z INFO tracing_emit: event={"service":{"name":"tracing-example",...},"log":[...]}
//! ```
//!
//! With the feature **on**, the same subscriber-stack shape applies
//! (capture-only, nothing printed), but the emit is a plain
//! `println!("{json}")`: the finished event is printed as one bare
//! JSON line with no timestamp or level prefix, and every canonical
//! `tracing` record is captured into the wide event instead.

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

fn main() {
    init_capture();

    #[cfg(feature = "tracing")]
    let _guard = WideLogGuard::builder()
        .with_emit(|ev| {
            if let Ok(json) = ev.to_json() {
                // Raw JSON output: one bare JSON line, no envelope.
                println!("{json}");
            }
        })
        .build();

    #[cfg(not(feature = "tracing"))]
    let _guard = WideLogGuard::builder()
        .with_emit(|ev| {
            if let Ok(json) = ev.to_json() {
                // Use the fully-qualified path so we call the real tracing
                // macro. (The generated `info!` would route the message into
                // the wide-event log array instead.)
                ::tracing::info!(event = %json);
            }
        })
        .build();

    wl_set!("service.name", "tracing-example");
    wl_inc!("requests");
    info!("request received");
    info!("request completed");

    // _guard drops → the emit closure runs: bare JSON under the
    // feature, fmt-enveloped `event=` record without it.
    drop(_guard);
}
