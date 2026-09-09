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

#[cfg(feature = "tracing")]
fn raw_json_emit(ev: &wide_log::WideEvent<EventKey>) {
    // Raw JSON output: print the serialized event as one bare JSON
    // line with no timestamp or level prefix. The subscriber stack is
    // capture-only (no formatting layer), so this emit is the only
    // thing that writes to stdout.
    if let Ok(json) = ev.to_json() {
        println!("{json}");
    }
}

fn main() {
    init_capture();
    // Under the `tracing` feature, emit through the raw-JSON closure
    // (the capture-only subscriber prints nothing); without the
    // feature, `default_emit` writes the bare JSON line itself.
    #[cfg(feature = "tracing")]
    let _guard = WideLogGuard::builder().with_emit(raw_json_emit).build();
    #[cfg(not(feature = "tracing"))]
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
