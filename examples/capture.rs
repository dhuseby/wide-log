//! Capture mode: with the `tracing` feature on, records emitted
//! through the canonical `tracing` macros — by this application or
//! by any dependency crate — are appended to the active wide event's
//! `log` array by the macro-generated `WideLogCaptureLayer`.
//!
//! Build and run with:
//!
//! ```text
//! cargo run --example capture --features tracing
//! ```
//!
//! The subscriber stack is capture-only: the capture layer routes
//! every record into the wide event and no formatting layer is
//! installed, so the subscriber prints nothing. The guard emits
//! through the raw-JSON closure, so the only stdout line is the bare
//! JSON object whose `log` array contains both the application-level
//! `::tracing::info!` calls and the records from the dependency-like
//! child module below — with no timestamp or level prefix.

use wide_log::wide_log;

#[cfg(feature = "tracing")]
#[allow(unused_imports)]
use wide_log::{debug, error, info, trace, warn};

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

// A dependency-like module: in a real application this would be a
// separate crate that logs through the canonical `tracing` macros
// without knowing anything about wide-log.
mod dependency_like_crate {
    pub fn do_work() {
        ::tracing::debug!("dependency step starting");
        ::tracing::info!(items = 3u64, "dependency processed a batch");
        ::tracing::warn!("dependency saw a slow path");
    }
}

fn main() {
    // Capture-only subscriber stack: the capture layer routes every
    // canonical tracing record into the active wide event, and no
    // formatting layer is installed, so the subscriber itself prints
    // nothing.
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry()
        .with(crate::WideLogCaptureLayer::new())
        .init();

    // Raw JSON output: one bare JSON line, no timestamp or level
    // prefix. The capture-only subscriber prints nothing, so this
    // emit is the only thing that writes to stdout.
    let _guard = WideLogGuard::builder()
        .with_emit(|ev| {
            if let Ok(json) = ev.to_json() {
                println!("{json}");
            }
        })
        .build();

    wl_set!("service.name", "capture-example");
    wl_inc!("requests");
    ::tracing::info!("application started");
    dependency_like_crate::do_work();
    ::tracing::info!("application finished");

    // _guard drops → one bare JSON line with the captured entries in
    // the `log` array.
    drop(_guard);
}
