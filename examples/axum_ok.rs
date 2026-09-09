use std::sync::OnceLock;

use axum::Router;
use axum::routing::get;
use tokio::sync::Notify;
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
    "http": {
        "method": null,
        "path": null,
        "status": null,
    },
});
// "duration": { "total_ms": duration! } is auto-added

// Notifier fired after the handler completes so `main` can shut the server
// down after exactly one request.
static DONE: OnceLock<Notify> = OnceLock::new();

async fn ok() -> &'static str {
    wl_set!("service.name", "ok-service");
    wl_set!("http.method", "GET");
    wl_set!("http.path", "/ok");
    wl_set!("http.status", 200u64);

    info!("request received");

    do_work().await;

    info!("request completed");

    // Signal `main` to initiate graceful shutdown after this one request.
    // The guard still drops (and emits) when `scope_default` completes as
    // the handler future resolves.
    DONE.get().unwrap().notify_one();

    // Handler returns → WideLogLayer drops the guard → sets
    // duration.total_ms, serializes to JSON, and emits. Under the
    // `tracing` feature the emit is the raw-JSON closure (bare JSON
    // line); without the feature, `default_emit` writes the bare JSON
    // line to the non-blocking stdout writer.
    ""
}

async fn do_work() {
    warn!("upstream slow");
    fetch_upstream().await;
    info!("upstream done");
}

async fn fetch_upstream() {
    error!("upstream failed");
}

#[tokio::main]
async fn main() {
    init_capture();
    let done = Notify::new();
    DONE.set(done).unwrap();

    // Under the `tracing` feature the middleware's `default_emit`
    // would route the finished event through the (silent) subscriber;
    // wrap the handler in `scope(raw_json_emit, ...)` instead so the
    // guard drop prints the bare JSON line. Without the feature,
    // `WideLogLayer` + `default_emit` write the bare JSON line.
    #[cfg(feature = "tracing")]
    let app = Router::new()
        .route("/ok", get(|| scope(raw_json_emit, ok())))
        .layer(WideLogLayer::new());
    #[cfg(not(feature = "tracing"))]
    let app = Router::new()
        .route("/ok", get(ok))
        .layer(WideLogLayer::new());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    println!("Server listening on http://127.0.0.1:3000");
    println!("Run this in another terminal to trigger the wide-log emit, then the service exits:");
    println!("  curl http://127.0.0.1:3000/ok");
    println!("Waiting for a request on /ok...");

    // Serve until the first request completes, then shut down.
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            DONE.get().unwrap().notified().await;
        })
        .await
        .unwrap();

    // Ensure the emitted JSON line lands on stdout before exit.
    wide_log::stdout_emit::flush();
}
