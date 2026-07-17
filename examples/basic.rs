use wide_log::wide_log;

wide_log!({
    "service": {
        "name": "example-service",
        "version": "1.0.0",
    },
    "requests": counter!,
});

fn main() {
    let _guard = WideLogGuard::builder().build();

    wl_inc!("requests");

    info!("request received");
    warn!("upstream slow");

    // Drop the guard explicitly so duration.total_ms and event.timestamp are
    // set and the event is serialized to JSON and handed to the non-blocking
    // stdout writer as a single line:
    //
    // {"service":{"name":"example-service","version":"1.0.0"},
    //  "duration":{"total_ms":42},"requests":1,
    //  "event":{"timestamp":"2026-07-12T12:00:00.000Z","id":"01J6XK5R..."},
    //  "log":[{"level":"info","message":"request received"},
    //         {"level":"warn","message":"upstream slow"}]}
    drop(_guard);

    // The stdout writer thread is non-blocking; flush before exit so the
    // emitted line is actually written before the process terminates.
    wide_log::stdout_emit::flush();
}
