//! Tests for the optional `tracing` feature on `wide-log-macros`.
//!
//! When the `tracing` feature is enabled on the macros crate
//! (gated on the `tracing` feature on `wide-log`), the
//! macro-generated `default_emit` routes the serialized event
//! through `::tracing::info!(event = %json)` instead of writing
//! the bare JSON line to non-blocking stdout. A one-time
//! `eprintln!` warning is emitted on first use to remind the user
//! that this is a transition aid, not the default.

#![cfg(feature = "tracing")]

use std::sync::Mutex;
use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

// A simple custom tracing subscriber that captures the formatted
// line (so we can assert that the `default_emit` did route through
// `::tracing::info!` rather than writing bare JSON to stdout).
static CAPTURED: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct CapturingSubscriber;

impl tracing::Subscriber for CapturingSubscriber {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _span: &tracing::Id, _record: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _span: &tracing::Id, _follows: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        // Render the event into a string so we can inspect it.
        struct StringVisitor<'a>(&'a mut String);
        impl tracing::field::Visit for StringVisitor<'_> {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                use std::fmt::Write;
                let _ = write!(&mut self.0, "{}={:?} ", field.name(), value);
            }
        }
        let mut s = String::new();
        let mut v = StringVisitor(&mut s);
        event.record(&mut v);
        CAPTURED.lock().unwrap().push(s);
    }
    fn enter(&self, _span: &tracing::Id) {}
    fn exit(&self, _span: &tracing::Id) {}
}

#[test]
fn tracing_feature_routes_default_emit_through_tracing_info() {
    // Install our capturing subscriber as the default for this
    // thread.
    let _guard = tracing::subscriber::set_default(CapturingSubscriber);

    // Build a guard WITHOUT a custom emit, so the macro-generated
    // `default_emit` is used.
    {
        let _g = WideLogGuard::builder().build();
        wl_set!("service.name", "tracing-feature-test");
        wl_inc!("requests");
    }

    let captured = CAPTURED.lock().unwrap();
    assert_eq!(
        captured.len(),
        1,
        "default_emit should have routed through ::tracing::info! exactly once"
    );

    let record = &captured[0];
    // The event field should contain the JSON event string. The
    // string will be Debug-formatted (quoted), so we look for the
    // key bits of the event.
    assert!(
        record.contains("event="),
        "captured record should contain an `event=` field: {record}"
    );
    assert!(
        record.contains("service"),
        "captured event should contain the service key: {record}"
    );
    assert!(
        record.contains("tracing-feature-test"),
        "captured event should contain the service.name value: {record}"
    );
}
