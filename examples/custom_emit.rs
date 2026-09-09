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

wide_log!({
    "service": { "name": null, "version": "1.0.0" },
    "requests": counter!,
});

fn main() {
    init_capture();
    let _guard = WideLogGuard::builder()
        .with_emit(|ev| {
            if let Ok(json) = ev.to_json() {
                println!("{json}");
            }
        })
        .build();

    wl_set!("service.name", "example-service");
    wl_inc!("requests");
    info!("request received");
}
