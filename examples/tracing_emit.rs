//! Scenario 4 from `wltest`: a custom emit function that routes the
//! serialized wide event through `::tracing::info!`.
//!
//! Unlike [`basic`](./basic.rs) (which uses the generated `default_emit`
//! that writes a bare JSON line to stdout), this example installs a custom
//! emit via `with_emit`. The emit serializes the event to JSON and hands it
//! to `::tracing::info!(event = %json)`, so the emitted line on stdout is
//! wrapped in the tracing fmt envelope:
//!
//! ```text
//! 2026-07-17T16:01:26Z INFO tracing_emit: event={"service":{"name":"tracing-example",...},"log":[...]}
//! ```
//!
//! Notice the `event=` field marker and the `INFO` envelope prefix — the
//! JSON itself is not re-escaped (it is emitted via `%` Display formatting),
//! but it is no longer a bare top-level JSON object on the line. Compare
//! this with `basic.rs`, whose `default_emit` output starts directly with
//! `{"service":...`.

use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

fn main() {
    // Install a tracing fmt subscriber so the `::tracing::info!` call below
    // produces an envelope-prefixed line on stdout.
    tracing_subscriber::fmt().init();

    let _guard = WideLogGuard::builder()
        .with_emit(|ev| {
            if let Ok(json) = ev.to_json() {
                // Use the fully-qualified path so we call the real tracing
                // macro. (The generated `info!` would route the message into
                // the wide-event log array instead.)
                ::tracing::info!(event = %json);
            }
        })
        .build();

    wl_set!("service.name", "tracing-example");
    wl_inc!("requests");
    info!("request received");
    info!("request completed");

    // _guard drops → event serialized to JSON and emitted via the custom
    // emit as `::tracing::info!(event = %json)`. The line on stdout is
    // wrapped in the tracing fmt envelope and the JSON is the value of the
    // `event=` field — not a bare top-level JSON object.
    drop(_guard);
}
