use axum::Router;
use axum::routing::get;
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

async fn ok() -> &'static str {
    wl_set!("service.name", "ok-service");
    wl_set!("http.method", "GET");
    wl_set!("http.path", "/ok");
    wl_set!("http.status", 200u64);

    info!("request received");

    do_work().await;

    info!("request completed");

    // Handler returns → WideLogLayer drops the guard → sets duration.total_ms,
    // serializes to JSON, emits via ::tracing::info!.
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
    tracing_subscriber::fmt().init();

    let app = Router::new().route("/ok", get(ok)).layer(WideLogLayer);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
