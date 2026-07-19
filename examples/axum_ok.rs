use std::sync::OnceLock;

use axum::Router;
use axum::routing::get;
use tokio::sync::Notify;
use wide_log::wide_log;

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
    // duration.total_ms, serializes to JSON, writes to non-blocking stdout
    // via `default_emit`.
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
    let done = Notify::new();
    DONE.set(done).unwrap();

    let app = Router::new().route("/ok", get(ok)).layer(WideLogLayer);

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
