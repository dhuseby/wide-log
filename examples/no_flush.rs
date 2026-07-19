//! Reproduction for the time-based flush hang documented in `fix.md`.
//!
//! Emits a single wide-event line and exits **without** calling
//! `stdout_emit::flush()`. Under the default `FlushPolicy` (100 ms
//! max_interval), the writer thread *should* flush the buffered line
//! to stdout within ~100 ms of the last `submit`. Prior to the fix,
//! the writer loop only checked the flush timer when a new `Job`
//! arrived, so with no further submits the line stayed buffered in
//! the `BufWriter` and a consumer reading this process's stdout
//! pipe would hang forever.

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

    // Drop the guard to serialize + submit the event. We deliberately
    // do NOT call stdout_emit::flush() — the writer thread's
    // time-based flush is responsible for delivering the line. Sleep
    // briefly so the writer's timer-based wakeup has a chance to
    // flush before process teardown.
    drop(_guard);
    std::thread::sleep(std::time::Duration::from_millis(500));
}
