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
//! The emitted stdout line is one JSON object whose `log` array
//! contains both the application-level `::tracing::info!` calls and
//! the record from the dependency-like child module below.

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
    // The capture layer must be part of the subscriber stack: the
    // layer is what routes tracing records into the active wide
    // event. The `fmt` layer formats and prints the emit-side JSON
    // record (in tracing mode `default_emit` routes the finished
    // event through `::tracing::info!`, so a formatting layer is
    // needed to see output at all).
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(crate::WideLogCaptureLayer::new())
        .init();

    let _guard = WideLogGuard::builder().build();

    wl_set!("service.name", "capture-example");
    wl_inc!("requests");
    ::tracing::info!("application started");
    dependency_like_crate::do_work();
    ::tracing::info!("application finished");

    // _guard drops → event serialized to JSON and emitted through
    // the subscriber (tracing mode), with the `log` array carrying
    // the entries captured above.
    drop(_guard);

    wide_log::stdout_emit::flush();
}
