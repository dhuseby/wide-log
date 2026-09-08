use wide_log::wide_log;

#[cfg(feature = "tracing")]
#[allow(unused_imports)]
use wide_log::{debug, error, info, trace, warn};

wide_log!({
    "service": {
        "name": "example-service",
        "version": "1.0.0",
    },
    "requests": counter!,
});

#[cfg(feature = "tracing")]
fn init_capture() {
    use tracing_subscriber::prelude::*;
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .finish()
        .with(crate::WideLogCaptureLayer::new())
        .init();
}

#[cfg(not(feature = "tracing"))]
fn init_capture() {}

fn main() {
    init_capture();
    let _guard = WideLogGuard::builder().build();

    wl_inc!("requests");

    info!("request received");
    warn!("upstream slow");

    // Drop the guard explicitly so duration.total_ms and event.timestamp are
    // set and the event is serialized to JSON and handed to the non-blocking
    // stdout writer as a single line:
    //
    // {"service":{"name":"example-service","version":"1.0.0"},
    //  "duration":{"total_ms":42},"requests":1,
    //  "event":{"timestamp":"2026-07-12T12:00:00.000Z","id":"01J6XK5R..."},
    //  "log":[{"level":"info","message":"request received"},
    //         {"level":"warn","message":"upstream slow"}]}
    drop(_guard);

    // The stdout writer thread is non-blocking; flush before exit so the
    // emitted line is actually written before the process terminates.
    wide_log::stdout_emit::flush();
}
