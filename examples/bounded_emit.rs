//! Behavior #4 of the emit modes in `wide-log`: a **bounded** channel
//! between producers and the stdout writer thread, providing backpressure.
//!
//! The default channel is unbounded (`mpsc::channel`): [`submit`] never
//! blocks, but in-flight memory is bounded only by available memory. This
//! example switches the channel to `mpsc::sync_channel(8)` via
//! [`set_channel_capacity`] before any `submit` call. With a bounded
//! channel, [`submit`] **blocks** when the 8-event buffer is full, applying
//! backpressure from the writer thread to the producer. The emitted lines
//! on stdout are identical to the default (bare JSON) — the bounded channel
//! only changes the in-flight buffering, not the output format.
//!
//! For a non-blocking variant that drops on overflow instead of blocking,
//! see [`try_submit`].
//!
//! ```text
//! {"service":{"name":"bounded-example","version":"1.0.0"},"requests":1,"duration":{"total_ms":...},"event":{"timestamp":"...","id":"..."},"log":[{"level":"info","message":"request 0 received"}, ...]}
//! ```
//!
//! Build with:
//!
//! ```text
//! cargo run --example bounded_emit
//! ```

use wide_log::stdout_emit::{set_channel_capacity, ChannelCapacity};
use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

fn main() {
    // Configure the writer's channel as a bounded `sync_channel(8)` before
    // the first `submit`. Idempotent: the first call wins; subsequent calls
    // are silent no-ops. Must be called before the writer is started (which
    // happens lazily on the first `submit`).
    set_channel_capacity(ChannelCapacity::Bounded(8));

    let _guard = WideLogGuard::builder().build();

    wl_set!("service.name", "bounded-example");
    wl_inc!("requests");

    // A burst of log entries. With capacity 8 the writer thread can hold at
    // most 8 in-flight events; if it falls behind, `submit` blocks here
    // rather than growing the queue unboundedly.
    for i in 0..20 {
        info!("request {} received", i);
    }

    // Drop the guard to serialize + submit the event. With the bounded
    // channel this may briefly block if the writer is still draining, but
    // the line format on stdout is identical to `basic.rs`.
    drop(_guard);

    // Drain before exit so the bounded buffer is flushed to stdout.
    wide_log::stdout_emit::flush();
}