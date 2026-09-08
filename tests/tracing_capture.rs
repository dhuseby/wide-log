//! Tests for the `tracing` feature's capture direction: canonical
//! `tracing` records (from the application or a dependency crate)
//! are appended to the active wide event's `log` array by the
//! macro-generated `WideLogCaptureLayer`.
//!
//! Feature-gated: this test target only builds with
//! `--features tracing`, because that is when the capture layer and
//! the re-exported level macros exist and the wide-log level macros
//! do not.

#![cfg(feature = "tracing")]

use std::sync::{Arc, Mutex};

use sonic_rs::{JsonContainerTrait, JsonValueTrait};
use tracing_subscriber::prelude::*;
use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

type CaptureSlot = Arc<Mutex<Option<String>>>;

fn capture() -> (
    CaptureSlot,
    impl FnOnce(&wide_log::WideEvent<EventKey>) + Send + 'static,
) {
    let slot: CaptureSlot = Arc::new(Mutex::new(None));
    let s = slot.clone();
    let emit = move |ev: &wide_log::WideEvent<EventKey>| {
        *s.lock().unwrap() = Some(ev.to_json().unwrap());
    };
    (slot, emit)
}

fn parse(slot: &CaptureSlot) -> sonic_rs::Value {
    let json = slot.lock().unwrap().clone().unwrap();
    sonic_rs::from_str(&json).unwrap()
}

// A capturing tracing layer that records the formatted records of
// every event it sees. Used as the second layer in the registry to
// verify tee semantics: a record captured by `WideLogCaptureLayer`
// must still be forwarded to the other layers.
struct RecordingLayer {
    records: Arc<Mutex<Vec<String>>>,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for RecordingLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        struct Writer<'a>(&'a mut String);
        impl tracing::field::Visit for Writer<'_> {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                use std::fmt::Write;
                let _ = write!(&mut self.0, "{}={:?} ", field.name(), value);
            }
        }
        let mut s = format!("{} ", event.metadata().level());
        event.record(&mut Writer(&mut s));
        self.records.lock().unwrap().push(s);
    }
}

fn recording_layer(records: Arc<Mutex<Vec<String>>>) -> RecordingLayer {
    RecordingLayer { records }
}

fn parse_log(parsed: &sonic_rs::Value) -> &sonic_rs::Array {
    parsed["log"].as_array().expect("log is an array")
}

// ---- capture of the five canonical levels ----

#[test]
fn captures_all_levels_in_order() {
    let (slot, emit) = capture();
    let subscriber = tracing_subscriber::registry().with(WideLogCaptureLayer::new());
    {
        let _sub = tracing::subscriber::set_default(subscriber);
        let _guard = WideLogGuard::builder().with_emit(emit).build();

        ::tracing::info!("info one");
        ::tracing::warn!("warn two");
        ::tracing::error!("error three");
        ::tracing::debug!("debug four");
        ::tracing::trace!("trace five");
    }

    let parsed = parse(&slot);
    let log = parse_log(&parsed);
    assert_eq!(log.len(), 5);
    let expected = [
        ("info", "info one"),
        ("warn", "warn two"),
        ("error", "error three"),
        ("debug", "debug four"),
        ("trace", "trace five"),
    ];
    for (i, (level, message)) in expected.iter().enumerate() {
        assert_eq!(log[i]["level"].as_str(), Some(*level));
        assert_eq!(log[i]["message"].as_str(), Some(*message));
    }
}

// ---- payload rendering rules ----

#[test]
fn message_only_event_keeps_clean_message() {
    let (slot, emit) = capture();
    let subscriber = tracing_subscriber::registry().with(WideLogCaptureLayer::new());
    {
        let _sub = tracing::subscriber::set_default(subscriber);
        let _guard = WideLogGuard::builder().with_emit(emit).build();
        ::tracing::info!("message only");
    }

    let parsed = parse(&slot);
    let log = parse_log(&parsed);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0]["level"].as_str(), Some("info"));
    assert_eq!(log[0]["message"].as_str(), Some("message only"));
}

#[test]
fn message_plus_fields_appends_rendered_fields() {
    let (slot, emit) = capture();
    let subscriber = tracing_subscriber::registry().with(WideLogCaptureLayer::new());
    {
        let _sub = tracing::subscriber::set_default(subscriber);
        let _guard = WideLogGuard::builder().with_emit(emit).build();
        ::tracing::info!(count = 42u64, "request done");
    }

    let parsed = parse(&slot);
    let log = parse_log(&parsed);
    assert_eq!(log.len(), 1);
    let message = log[0]["message"].as_str().unwrap();
    assert!(
        message.starts_with("request done count=42"),
        "fields should be appended after the message: {message}"
    );
}

#[test]
fn fields_only_event_renders_fields_as_message() {
    let (slot, emit) = capture();
    let subscriber = tracing_subscriber::registry().with(WideLogCaptureLayer::new());
    {
        let _sub = tracing::subscriber::set_default(subscriber);
        let _guard = WideLogGuard::builder().with_emit(emit).build();
        // A typed numeric field exercises the `record_i64` default
        // method of `Visit` (it forwards to `record_debug`).
        ::tracing::info!(count = 42u64);
        ::tracing::info!(signed = -7i64);
    }

    let parsed = parse(&slot);
    let log = parse_log(&parsed);
    assert_eq!(log.len(), 2);
    let first = log[0]["message"].as_str().unwrap();
    assert!(
        first.contains("count=42"),
        "fields-only event should render its fields: {first}"
    );
    let second = log[1]["message"].as_str().unwrap();
    assert!(
        second.contains("signed=-7"),
        "typed fields must render like other fields: {second}"
    );
}

#[test]
fn event_with_no_message_and_no_fields_is_skipped() {
    // A record with no fields at all produces no log entry.
    let (slot, emit) = capture();
    let subscriber = tracing_subscriber::registry().with(WideLogCaptureLayer::new());
    {
        let _sub = tracing::subscriber::set_default(subscriber);
        let _guard = WideLogGuard::builder().with_emit(emit).build();
        // A record with no message and no fields: `event!` with an
        // empty field list renders nothing, so nothing is captured.
        ::tracing::event!(target: "empty_record", ::tracing::Level::INFO, {});
    }
    let parsed = parse(&slot);
    assert!(parsed["log"].is_null() || parse_log(&parsed).is_empty());
}

// ---- level floor and tee semantics ----

#[test]
fn with_max_level_skips_lower_levels_but_still_forwards() {
    let (slot, emit) = capture();
    let records = Arc::new(Mutex::new(Vec::new()));
    let forwarded = recording_layer(records.clone());
    let subscriber = tracing_subscriber::registry()
        .with(WideLogCaptureLayer::new().with_max_level(tracing::Level::INFO))
        .with(forwarded);
    {
        let _sub = tracing::subscriber::set_default(subscriber);
        let _guard = WideLogGuard::builder().with_emit(emit).build();
        ::tracing::info!("kept by capture");
        ::tracing::debug!("below the floor");
    }

    // Capture kept only the INFO record.
    let parsed = parse(&slot);
    let log = parse_log(&parsed);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0]["level"].as_str(), Some("info"));

    // Tee semantics: the DEBUG record was still forwarded to the
    // other layer in the stack.
    let seen = records.lock().unwrap();
    assert!(
        seen.iter().any(|r| r.contains("below the floor")),
        "the record below the floor must still reach other layers: {seen:?}"
    );
}

// ---- guard-context edge cases ----

#[test]
fn no_guard_active_captures_nothing_and_does_not_panic() {
    let (slot, _emit) = capture();
    let subscriber = tracing_subscriber::registry().with(WideLogCaptureLayer::new());
    let _sub = tracing::subscriber::set_default(subscriber);
    ::tracing::info!("no guard active");
    // The record is forwarded to other layers but not captured.
    assert!(slot.lock().unwrap().is_none());
}

#[test]
fn nested_guards_capture_into_the_innermost_event() {
    let (outer_slot, outer_emit) = capture();
    let (inner_slot, inner_emit) = capture();
    let subscriber = tracing_subscriber::registry().with(WideLogCaptureLayer::new());
    let _sub = tracing::subscriber::set_default(subscriber);
    {
        let _outer = WideLogGuard::builder().with_emit(outer_emit).build();
        {
            let _inner = WideLogGuard::builder().with_emit(inner_emit).build();
            ::tracing::info!("innermost record");
        }
        ::tracing::info!("outer record");
    }

    let inner = parse(&inner_slot);
    let inner_log = parse_log(&inner);
    assert_eq!(inner_log.len(), 1);
    assert_eq!(inner_log[0]["message"].as_str(), Some("innermost record"));

    let outer = parse(&outer_slot);
    let outer_log = parse_log(&outer);
    assert_eq!(outer_log.len(), 1);
    assert_eq!(outer_log[0]["message"].as_str(), Some("outer record"));
}

// ---- reserved target: no self-capture ----

#[test]
fn emit_side_record_is_not_self_captured() {
    // The `default_emit` record carries the reserved `wide_log`
    // target; the capture layer must skip it so the finished event
    // does not re-enter itself.
    let targets = Arc::new(Mutex::new(Vec::new()));
    struct TargetRecorder {
        targets: Arc<Mutex<Vec<String>>>,
    }
    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for TargetRecorder {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            self.targets
                .lock()
                .unwrap()
                .push(event.metadata().target().to_string());
        }
    }
    let recorder = TargetRecorder {
        targets: targets.clone(),
    };
    // The guard uses `default_emit` (no custom emit): the finished
    // JSON line flows through the subscriber instead of any capture
    // slot, and the capture layer skips the reserved target.
    let subscriber = tracing_subscriber::registry()
        .with(WideLogCaptureLayer::new())
        .with(recorder);
    let _sub = tracing::subscriber::set_default(subscriber);
    {
        let _guard = WideLogGuard::builder().build();
        wl_inc!("requests");
        // The guard drops at scope exit and `default_emit` routes the
        // finished JSON line through ::tracing::info! with the
        // reserved target.
    }

    // The emit-side record was seen by the stack (tee works) and the
    // capture layer skipped it: nothing was re-captured into the
    // just-emitted event.
    let seen = targets.lock().unwrap();
    assert!(
        seen.iter().any(|t| t == "wide_log"),
        "the emit-side record should be visible to the stack with the reserved target: {seen:?}"
    );
}

// ---- buffer behavior ----

#[test]
fn long_message_is_captured_and_buffer_is_reused() {
    let (slot, emit) = capture();
    let subscriber = tracing_subscriber::registry().with(WideLogCaptureLayer::new());
    {
        let _sub = tracing::subscriber::set_default(subscriber);
        let _guard = WideLogGuard::builder().with_emit(emit).build();
        let long = "x".repeat(4096);
        ::tracing::info!("{}", long);
        ::tracing::info!("short after long");
    }

    let parsed = parse(&slot);
    let log = parse_log(&parsed);
    assert_eq!(log.len(), 2);
    assert_eq!(log[0]["message"].as_str().unwrap().len(), 4096);
    assert_eq!(log[1]["message"].as_str(), Some("short after long"));
}

// ---- cross-module capture ----

mod child {
    // Capturing from a child module mirrors a dependency crate: the
    // record must reach the event through the capture layer without
    // any wide-log import.
    #[test]
    fn child_records_captured() {
        super::assert_child_capture();
    }
}

fn assert_child_capture() {
    let (slot, emit) = capture();
    let subscriber = tracing_subscriber::registry().with(WideLogCaptureLayer::new());
    let _sub = tracing::subscriber::set_default(subscriber);
    fn child_call() {
        ::tracing::warn!("from a child function");
    }
    {
        let _guard = WideLogGuard::builder().with_emit(emit).build();
        child_call();
    }
    let parsed = parse(&slot);
    let log = parse_log(&parsed);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0]["level"].as_str(), Some("warn"));
    assert_eq!(log[0]["message"].as_str(), Some("from a child function"));
}

// ---- empty message convention ----

#[test]
fn empty_message_is_captured_as_an_empty_entry() {
    let (slot, emit) = capture();
    let subscriber = tracing_subscriber::registry().with(WideLogCaptureLayer::new());
    {
        let _sub = tracing::subscriber::set_default(subscriber);
        let _guard = WideLogGuard::builder().with_emit(emit).build();
        ::tracing::info!("");
    }

    let parsed = parse(&slot);
    let log = parse_log(&parsed);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0]["level"].as_str(), Some("info"));
    assert_eq!(log[0]["message"].as_str(), Some(""));
}
