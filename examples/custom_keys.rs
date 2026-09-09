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

// Customize built-in key strings using the dotted-path override syntax.
// Event.Id => "correlation_id" means the generated event ID is serialized
// under "correlation_id" instead of the default "id".
wide_log!([
    Event.Id        => "correlation_id",
    Log.Level       => "severity",
    Log.Message     => "msg",
    Duration.TotalMs => "elapsed_ms"
], {
    "service": { "name": "example", "version": "1.0.0" },
    "requests": counter!,
});

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

    // _guard drops → emitted JSON uses custom key names, written to
    // non-blocking stdout:
    // {"service":{"name":"example","version":"1.0.0"},
    //  "duration":{"elapsed_ms":0},"requests":1,
    //  "event":{"timestamp":"...","correlation_id":"01J6XK5R..."},
    //  "log":[{"severity":"info","msg":"request received"},
    //         {"severity":"warn","msg":"upstream slow"}]}
}
