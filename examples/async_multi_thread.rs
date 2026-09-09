use wide_log::wide_log;

#[cfg(feature = "tracing")]
#[allow(unused_imports)]
use wide_log::{debug, error, info, trace, warn};

#[cfg(feature = "tracing")]
fn init_capture() {
    use tracing_subscriber::prelude::*;
    // Capture-only subscriber stack: the capture layer routes every
    // canonical tracing record (application and dependency crates)
    // into the active wide event, and no formatting layer is
    // installed, so the subscriber itself prints nothing.
    tracing_subscriber::registry()
        .with(crate::WideLogCaptureLayer::new())
        .init();
}

#[cfg(not(feature = "tracing"))]
fn init_capture() {}

#[cfg(feature = "tracing")]
fn raw_json_emit(ev: &wide_log::WideEvent<EventKey>) {
    // Raw JSON output: print the serialized event as one bare JSON
    // line with no timestamp or level prefix. The subscriber stack is
    // capture-only (no formatting layer), so this emit is the only
    // thing that writes to stdout.
    if let Ok(json) = ev.to_json() {
        println!("{json}");
    }
}

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    init_capture();
    let mut handles = vec![];
    for i in 0..10 {
        handles.push(tokio::spawn(handle_request(i)));
    }
    for h in handles {
        h.await.unwrap();
    }
}

async fn handle_request(id: u64) {
    // Under the `tracing` feature, emit through the raw-JSON closure
    // (the capture-only subscriber prints nothing); without the
    // feature, `default_emit` writes the bare JSON line itself.
    #[cfg(feature = "tracing")]
    let fut = scope(raw_json_emit, async {
        wl_set!("service.name", format!("worker-{id}"));
        wl_inc!("requests");
        info!("request {} started", id);

        // The task may be moved to another thread here.
        // task_local! ensures the event pointer moves with it.
        tokio::task::yield_now().await;

        info!("request {} completed", id);
    });
    #[cfg(not(feature = "tracing"))]
    let fut = scope_default(async {
        wl_set!("service.name", format!("worker-{id}"));
        wl_inc!("requests");
        info!("request {} started", id);

        // The task may be moved to another thread here.
        // task_local! ensures the event pointer moves with it.
        tokio::task::yield_now().await;

        info!("request {} completed", id);
    });
    fut.await;
    // guard drops → event written to stdout with duration.total_ms
}
