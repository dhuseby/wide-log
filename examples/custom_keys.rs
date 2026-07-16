use wide_log::wide_log;

// Customize built-in key strings using the dotted-path override syntax.
// Event.Id => "correlation_id" means the generated event ID is serialized
// under "correlation_id" instead of the default "id".
wide_log!([
    Event.Id        => "correlation_id",
    Log.Level       => "severity",
    Log.Message     => "msg",
    Duration.TotalMs => "elapsed_ms"
], {
    "service": { "name": "example", "version": "1.0.0" },
    "requests": counter!,
});

fn main() {
    tracing_subscriber::fmt().init();

    let _guard = WideLogGuard::builder().build();

    wl_inc!("requests");
    info!("request received");
    warn!("upstream slow");

    // _guard drops → emitted JSON uses custom key names:
    // {"service":{"name":"example","version":"1.0.0"},
    //  "duration":{"elapsed_ms":0},"requests":1,
    //  "event":{"timestamp":"...","correlation_id":"01J6XK5R..."},
    //  "log":[{"severity":"info","msg":"request received"},
    //         {"severity":"warn","msg":"upstream slow"}]}
}