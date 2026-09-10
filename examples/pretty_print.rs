//! A subscriber stack with an application-owned custom layer: the stack
//! combines the macro-generated `WideLogCaptureLayer` (capture) with a
//! local `PrettyPrintLayer` that pretty-prints the finished wide event.
//!
//! The guard uses the default emit, so the finished event is serialized
//! and routed through the subscriber stack as
//! `::tracing::info!(target: "wide_log", event = %json)`. The capture
//! layer skips that reserved target so the event is not re-captured into
//! itself, and the custom layer pretty-prints the JSON payload as
//! indented text (2-space indent).
//!
//! Build and run with:
//!
//! ```text
//! cargo run --example pretty_print --features tracing
//! ```
//!
//! The stdout output is one pretty-printed JSON block per event. The
//! `log` array holds the records captured from the application's
//! `info!`/`warn!` calls (and any dependency crate), next to the
//! counter and the auto-added `duration` and `event` metadata.

use wide_log::{info, warn, wide_log};

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

// An application-owned layer, the way a real logging stack carries
// custom formatting or routing layers. This one pretty-prints only the
// finished wide event: the emit-side record carries the reserved
// `wide_log` target, so every other record (application or dependency
// crate) is ignored here and captured by `WideLogCaptureLayer` instead.
struct PrettyPrintLayer;

impl<S: tracing::Subscriber> tracing_subscriber::layer::Layer<S> for PrettyPrintLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _cx: tracing_subscriber::layer::Context<'_, S>) {
        if event.metadata().target() != "wide_log" {
            return;
        }
        // The default emit sets the JSON payload as the `event` field:
        // `event = %json` produces a `DisplayValue<&str>` whose `Debug`
        // output is the JSON text itself, so `record_debug` receives the
        // raw JSON without quotes.
        struct ExtractEvent<'a>(&'a mut Option<String>);

        impl tracing::field::Visit for ExtractEvent<'_> {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "event" {
                    *self.0 = Some(value.to_string());
                }
            }

            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "event" {
                    *self.0 = Some(format!("{value:?}"));
                }
            }
        }
        let mut slot = None;
        event.record(&mut ExtractEvent(&mut slot));
        let Some(json) = slot else {
            return;
        };

        // Pretty-print the payload. A parse or serialization failure
        // falls back to the raw text: the layer never drops output and
        // never panics.
        match sonic_rs::from_str::<sonic_rs::Value>(&json) {
            Ok(value) => match sonic_rs::to_string_pretty(&value) {
                Ok(pretty) => println!("{pretty}"),
                Err(_) => println!("{json}"),
            },
            Err(_) => println!("{json}"),
        }
    }
}

fn main() {
    // Two-layer stack: capture routes every canonical tracing record
    // into the active wide event, and the custom layer pretty-prints the
    // finished event that the default emit sends through the stack.
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry()
        .with(crate::WideLogCaptureLayer::new())
        .with(PrettyPrintLayer)
        .init();

    let _guard = WideLogGuard::builder().build();

    wl_set!("service.name", "pretty-print-example");
    wl_inc!("requests");
    info!("request received");
    warn!("upstream slow");

    // _guard drops here → the default emit serializes the event and
    // sends it through the stack as the reserved-target `wide_log`
    // record; PrettyPrintLayer prints the indented JSON block.
    drop(_guard);
}
