use wide_log::wide_log;

// Explicitly declare a custom duration leaf name.
// DURATION_PATH = &[Duration, WallMs] → sets duration.wall_ms on drop.
wide_log!({
    "service": { "name": null, "version": "1.0.0" },
    "duration": { "wall_ms": duration! },
    "requests": counter!,
});

fn main() {
    tracing_subscriber::fmt().init();

    let _guard = WideLogGuard::builder().build();

    wl_set!("service.name", "explicit-duration-example");
    wl_inc!("requests");
    info!("request received");

    // _guard drops → duration.wall_ms is set (not duration.total_ms).
}
