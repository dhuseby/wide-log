//! Behavior #2 of the three emit modes in `wide-log`: the `tracing`
//! *feature* routes the macro-generated `default_emit` through
//! `::tracing::info!` automatically — *without* a user-supplied
//! `with_emit` closure.
//!
//! Contrast with [`tracing_emit`](./tracing_emit.rs), which reaches the same
//! stdout envelope *manually* by passing a `with_emit` closure that calls
//! `::tracing::info!` itself, with the `tracing` feature **off**. Here the
//! feature does the routing at macro expansion time: the generated
//! `default_emit` serializes the event to JSON, converts to a `String`, and
//! emits `::tracing::info!(event = %s)`.
//!
//! Build with:
//!
//! ```text
//! cargo run --example tracing_default_emit --features tracing
//! ```
//!
//! A one-time `eprintln!` is printed on first use reminding that this is a
//! migration aid (not the default). The emitted stdout line is wrapped in the
//! tracing fmt envelope (timestamp, level, target), and the JSON is the
//! value of the `event=` field rather than a bare top-level JSON object:
//!
//! ```text
//! 2026-07-17T16:01:26Z INFO tracing_default_emit: event={"service":{"name":"tracing-default",...},"log":[...],"duration":{"total_ms":...},"event":{"timestamp":"...","id":"..."}}
//! ```
//!
//! Because the writer is the user's tracing subscriber (not wide-log's
//! stdout writer thread), there is **no** `stdout_emit::flush()` to call at
//! the end of `main` for this mode.

use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

fn main() {
    // Install a tracing fmt subscriber so the generated `default_emit`'s
    // `::tracing::info!(event = %json)` produces an envelope-prefixed line
    // on stdout. Without a subscriber, the tracing call is a no-op.
    tracing_subscriber::fmt().init();

    // No `with_emit` here: the `tracing` feature rewrites the generated
    // `default_emit` to route through `::tracing::info!` for us.
    let _guard = WideLogGuard::builder().build();

    wl_set!("service.name", "tracing-default");
    wl_inc!("requests");
    info!("request received");
    info!("request completed");

    // _guard drops → event serialized and emitted via the generated
    // `default_emit` as `::tracing::info!(event = %json)`. The line on
    // stdout is the tracing fmt envelope with the JSON as the `event=`
    // field value — not a bare top-level JSON object (compare with
    // `basic.rs`).
    drop(_guard);
}