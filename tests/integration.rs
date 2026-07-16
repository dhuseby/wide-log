use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
    "status": null,
});

use sonic_rs::{JsonContainerTrait, JsonValueTrait};

type CaptureSlot = std::sync::Arc<std::sync::Mutex<Option<String>>>;

#[allow(clippy::type_complexity)]
fn capture() -> (
    CaptureSlot,
    impl FnOnce(&wide_log::WideEvent<EventKey>) + Send + 'static,
) {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let c = captured.clone();
    let emit = move |ev: &wide_log::WideEvent<EventKey>| {
        *c.lock().unwrap() = Some(ev.to_json().unwrap());
    };
    (captured, emit)
}

fn parse(slot: &std::sync::Arc<std::sync::Mutex<Option<String>>>) -> sonic_rs::Value {
    let json = slot.lock().unwrap().clone().unwrap();
    sonic_rs::from_str(&json).unwrap()
}

fn current_field_count() -> usize {
    current()
        .map(|e| {
            let json = e.to_json().unwrap();
            let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
            parsed.as_object().map(|o| o.len()).unwrap_or(0)
        })
        .unwrap_or(0)
}

#[test]
fn guard_emits_with_defaults_and_duration() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    wl_set!("service.name", "test-svc");
    wl_inc!("requests");
    wl_set!("status", "ok");

    drop(_guard);

    let parsed = parse(&captured);
    assert_eq!(parsed["service"]["version"], "1.0.0");
    assert_eq!(parsed["service"]["name"], "test-svc");
    assert_eq!(parsed["requests"], 1);
    assert_eq!(parsed["status"], "ok");
    assert!(parsed["duration"]["total_ms"].is_number());
}

#[test]
fn log_macros_accumulate() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    info!("request received");
    warn!("upstream slow");
    error!("request failed");

    drop(_guard);

    let parsed = parse(&captured);
    let log = &parsed["log"];
    assert_eq!(log.as_array().unwrap().len(), 3);
    assert_eq!(log[0]["level"], "info");
    assert_eq!(log[0]["message"], "request received");
    assert_eq!(log[1]["level"], "warn");
    assert_eq!(log[1]["message"], "upstream slow");
    assert_eq!(log[2]["level"], "error");
    assert_eq!(log[2]["message"], "request failed");
}

#[test]
fn log_macros_with_format_args() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    info!("request {}", 42);
    warn!("retry {}/{}", 1, 3);

    drop(_guard);

    let parsed = parse(&captured);
    let log = &parsed["log"];
    assert_eq!(log[0]["message"], "request 42");
    assert_eq!(log[1]["message"], "retry 1/3");
}

#[test]
fn nested_path_set() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    wl_set!("service.name", "nested-svc");

    drop(_guard);

    let parsed = parse(&captured);
    assert_eq!(parsed["service"]["name"], "nested-svc");
    assert_eq!(parsed["service"]["version"], "1.0.0");
}

#[test]
fn counter_inc_and_dec() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    wl_inc!("requests");
    wl_inc!("requests");
    wl_inc!("requests");
    wl_dec!("requests");

    drop(_guard);

    let parsed = parse(&captured);
    assert_eq!(parsed["requests"], 2);
}

#[test]
fn wl_add_works() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    wl_add!("requests", 10);
    wl_add!("requests", -3);

    drop(_guard);

    let parsed = parse(&captured);
    assert_eq!(parsed["requests"], 7);
}

#[test]
fn wl_null_works() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    wl_null!("status");

    drop(_guard);

    let parsed = parse(&captured);
    assert!(parsed["status"].is_null());
}

#[test]
fn macros_are_noop_without_guard() {
    wl_set!("service.name", "noop");
    wl_inc!("requests");
    info!("nothing happens");
}

#[test]
fn default_values_set_on_creation() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    drop(_guard);

    let parsed = parse(&captured);
    assert_eq!(parsed["service"]["version"], "1.0.0");
}

#[test]
fn no_log_key_when_no_log_entries() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    wl_set!("status", "ok");

    drop(_guard);

    let json = captured.lock().unwrap().clone().unwrap();
    assert!(!json.contains("\"log\""));
}

#[test]
fn duration_auto_added() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    std::thread::sleep(std::time::Duration::from_millis(2));

    drop(_guard);

    let parsed = parse(&captured);
    use sonic_rs::JsonValueTrait;
    let total_ms = parsed["duration"]["total_ms"].as_u64().unwrap();
    assert!(
        total_ms >= 1,
        "duration.total_ms should be >= 1, got {total_ms}"
    );
}

// ---- Event key tests ----

#[test]
fn event_key_auto_added() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    drop(_guard);

    let parsed = parse(&captured);
    assert!(
        parsed["event"]["timestamp"].is_str(),
        "event.timestamp should be a string"
    );
    assert!(
        parsed["event"]["id"].is_str(),
        "event.id should be a string"
    );
    let id = parsed["event"]["id"].as_str().unwrap();
    assert_eq!(id.len(), 26, "default ULID should be 26 chars, got: {id}");
}

#[test]
fn event_id_custom() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder()
        .with_emit(emit)
        .with_id(|| "custom-id-12345".to_string())
        .build();

    drop(_guard);

    let parsed = parse(&captured);
    assert_eq!(parsed["event"]["id"], "custom-id-12345");
}

#[test]
fn event_timestamp_timezone() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder()
        .with_emit(emit)
        .with_timezone(chrono_tz::Tz::America__New_York)
        .build();

    drop(_guard);

    let parsed = parse(&captured);
    let ts = parsed["event"]["timestamp"].as_str().unwrap();
    assert!(
        ts.contains("-04:00") || ts.contains("-05:00"),
        "timestamp should reflect America/New_York offset: {ts}"
    );
}

#[test]
fn event_id_is_ulid_by_default() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    drop(_guard);

    let parsed = parse(&captured);
    let id = parsed["event"]["id"].as_str().unwrap();
    // ULID is 26 chars, base32 Crockford encoding
    assert_eq!(id.len(), 26, "ULID should be 26 chars, got: {id}");
    assert!(
        id.chars().all(|c| c.is_ascii_alphanumeric()),
        "ULID should be alphanumeric: {id}"
    );
}

#[cfg(feature = "uuid")]
#[test]
fn event_id_is_uuid_with_uuid_feature() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).with_uuid().build();

    drop(_guard);

    let parsed = parse(&captured);
    let id = parsed["event"]["id"].as_str().unwrap();
    // UUIDv4 is 36 chars including hyphens
    assert_eq!(id.len(), 36, "UUIDv4 should be 36 chars, got: {id}");
    assert!(
        id.chars().filter(|c| *c == '-').count() == 4,
        "UUID should have 4 hyphens: {id}"
    );
}

#[test]
fn builder_with_emit_works() {
    let (captured, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    wl_set!("status", "ok");
    drop(_guard);

    let parsed = parse(&captured);
    assert_eq!(parsed["status"], "ok");
}

#[test]
fn nested_sync_scopes_innermost_accessible() {
    let outer_captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let inner_captured = std::sync::Arc::new(std::sync::Mutex::new(None));

    let oc = outer_captured.clone();
    let _outer = WideLogGuard::builder()
        .with_emit(move |ev| {
            *oc.lock().unwrap() = Some(ev.to_json().unwrap());
        })
        .build();

    wl_set!("status", "outer");
    // service.version default + status + event.id = 3
    assert_eq!(current_field_count(), 3);

    {
        let ic = inner_captured.clone();
        let _inner = WideLogGuard::builder()
            .with_emit(move |ev| {
                *ic.lock().unwrap() = Some(ev.to_json().unwrap());
            })
            .build();

        // Inner scope: current() returns the inner event, not the outer.
        wl_set!("status", "inner");
        // service.version default + status + event.id = 3
        assert_eq!(current_field_count(), 3);

        info!("inner log");
    }

    // After inner drop: outer event is restored.
    assert_eq!(current_field_count(), 3);

    drop(_outer);

    let outer_parsed = parse(&outer_captured);
    let inner_parsed = parse(&inner_captured);

    assert_eq!(outer_parsed["status"], "outer");
    assert_eq!(inner_parsed["status"], "inner");
    assert!(outer_parsed.get("log").is_none() || outer_parsed["log"].is_null());
    assert!(inner_parsed["log"].is_array());
}

#[test]
fn nested_sync_scopes_outer_restored_after_inner_drop() {
    let outer_captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let inner_captured = std::sync::Arc::new(std::sync::Mutex::new(None));

    let oc = outer_captured.clone();
    let _outer = WideLogGuard::builder()
        .with_emit(move |ev| {
            *oc.lock().unwrap() = Some(ev.to_json().unwrap());
        })
        .build();

    wl_inc!("requests");

    {
        let ic = inner_captured.clone();
        let _inner = WideLogGuard::builder()
            .with_emit(move |ev| {
                *ic.lock().unwrap() = Some(ev.to_json().unwrap());
            })
            .build();

        // Inner event is separate — inc doesn't affect outer.
        wl_inc!("requests");
        wl_inc!("requests");
    }

    // service.version default + requests + event.id = 3
    assert_eq!(current_field_count(), 3);

    drop(_outer);

    let outer_parsed = parse(&outer_captured);
    let inner_parsed = parse(&inner_captured);

    assert_eq!(outer_parsed["requests"], 1);
    assert_eq!(inner_parsed["requests"], 2);
}

#[test]
fn current_is_none_without_guard() {
    assert!(current().is_none());
}

#[test]
fn current_is_some_with_guard() {
    let _guard = WideLogGuard::builder().with_emit(|_| {}).build();
    assert!(current().is_some());
    drop(_guard);
    assert!(current().is_none());
}
