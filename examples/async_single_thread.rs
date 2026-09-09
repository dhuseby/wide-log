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
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

#[tokio::main(flavor = "current_thread")]
async fn main() {
    init_capture();
    handle_request().await;
}

async fn handle_request() {
    // Under the `tracing` feature, emit through the raw-JSON closure
    // (the capture-only subscriber prints nothing); without the
    // feature, `default_emit` writes the bare JSON line itself.
    #[cfg(feature = "tracing")]
    let fut = scope(raw_json_emit, async {
        // with_uuid() generates a UUIDv4 event id instead of the default
        // ULID (requires the `uuid` feature on wide-log). The guard
        // emits through the raw-JSON closure.
        let _guard = WideLogGuard::builder()
            .with_uuid()
            .with_emit(raw_json_emit)
            .build();

        wl_set!("service.name", "example-service");
        wl_inc!("requests");
        info!("request received");

        fetch_upstream().await;

        info!("request completed");
        // guard drops → event emitted via the raw-JSON closure.
    });
    #[cfg(not(feature = "tracing"))]
    let fut = scope_default(async {
        // with_uuid() generates a UUIDv4 event id instead of the default
        // ULID (requires the `uuid` feature on wide-log).
        let _guard = WideLogGuard::builder().with_uuid().build();

        wl_set!("service.name", "example-service");
        wl_inc!("requests");
        info!("request received");

        fetch_upstream().await;

        info!("request completed");
        // guard drops → event emitted via default_emit (bare JSON to stdout).
    });
    fut.await;
    // Flush the non-blocking stdout writer so the line lands before exit.
    wide_log::stdout_emit::flush();
}

async fn fetch_upstream() {
    warn!("upstream slow");
}
