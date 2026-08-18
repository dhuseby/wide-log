use wide_log::wide_log;

// Explicitly declare a custom duration leaf name.
// DURATION_PATH = &[Duration, WallMs] → sets duration.wall_ms on drop.
wide_log!({
    "service": { "name": null, "version": "1.0.0" },
    "duration": { "wall_us": duration! },
    "requests": counter!,
});

fn main() {
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
