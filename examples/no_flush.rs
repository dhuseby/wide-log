//! Reproduction for the time-based flush hang documented in `fix.md`.
//!
//! Emits a single wide-event line and exits **without** calling
//! `stdout_emit::flush()`. Under the default `FlushPolicy` (100 ms
//! max_interval), the writer thread *should* flush the buffered line
//! to stdout within ~100 ms of the last `submit`. Prior to the fix,
//! the writer loop only checked the flush timer when a new `Job`
//! arrived, so with no further submits the line stayed buffered in
//! the `BufWriter` and a consumer reading this process's stdout
//! pipe would hang forever.

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

wide_log!({
    "service": {
        "name": "example-service",
        "version": "1.0.0",
    },
    "requests": counter!,
});

fn main() {
    init_capture();
    // Under the `tracing` feature, emit through the raw-JSON closure:
    // the capture-only subscriber prints nothing and the closure
    // prints synchronously, so the writer-thread timer is not part of
    // this mode's path. The no-flush demonstration below applies to
    // the default (feature-off) branch, where `default_emit` hands
    // the line to the non-blocking stdout writer.
    #[cfg(feature = "tracing")]
    let _guard = WideLogGuard::builder().with_emit(raw_json_emit).build();
    #[cfg(not(feature = "tracing"))]
    let _guard = WideLogGuard::builder().build();

    wl_inc!("requests");
    info!("request received");
    warn!("upstream slow");

    // Drop the guard to serialize + submit the event. We deliberately
    // do NOT call stdout_emit::flush() — the writer thread's
    // time-based flush is responsible for delivering the line. Sleep
    // briefly so the writer's timer-based wakeup has a chance to
    // flush before process teardown.
    drop(_guard);
    std::thread::sleep(std::time::Duration::from_millis(500));
}
