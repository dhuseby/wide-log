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

// Explicitly declare a custom duration leaf name.
// DURATION_PATH = &[Duration, WallMs] → sets duration.wall_ms on drop.
wide_log!({
    "service": { "name": null, "version": "1.0.0" },
    "duration": { "wall_us": duration! },
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

    wl_set!("service.name", "explicit-duration-example");
    wl_inc!("requests");
    info!("request received");

    // _guard drops → duration.wall_ms is set (not duration.total_ms).
    // The event is serialized to JSON and written to non-blocking stdout.
    drop(_guard);

    // The stdout writer thread is non-blocking; flush before exit so the
    // emitted line is actually written before the process terminates.
    wide_log::stdout_emit::flush();
}
