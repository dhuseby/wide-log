use wide_log::wide_log;

wide_log!({
    "service": {
        "name": "example-service",
        "version": "1.0.0",
    },
    "requests": counter!,
});

fn main() {
    tracing_subscriber::fmt().init();

    let _guard = WideLogGuard::builder().build();

    wl_inc!("requests");

    info!("request received");
    warn!("upstream slow");

    // _guard drops here → duration.total_ms and event.timestamp are set
    // automatically, event is serialized to JSON, emitted via ::tracing::info!.
}