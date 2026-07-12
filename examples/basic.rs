use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

fn main() {
    tracing_subscriber::fmt().init();

    let _guard = WideLogGuard::new();

    wl_set!("service.name", "example-service");
    wl_inc!("requests");

    info!("request received");
    warn!("upstream slow");

    // _guard drops here → duration.total_ms is set automatically,
    // event is serialized to JSON, emitted via ::tracing::info!.
}
