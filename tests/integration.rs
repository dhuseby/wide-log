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

#[test]
fn guard_emits_with_defaults_and_duration() {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let c = captured.clone();
    let _guard = EventKeyGuard::new_with_emit(move |ev| {
        *c.lock().unwrap() = Some(ev.to_json().unwrap());
    });

    wl_set!("service.name", "test-svc");
    wl_inc!("requests");
    wl_set!("status", "ok");

    drop(_guard);

    let json = captured.lock().unwrap().clone().unwrap();
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
    assert_eq!(parsed["service"]["version"], "1.0.0");
    assert_eq!(parsed["service"]["name"], "test-svc");
    assert_eq!(parsed["requests"], 1);
    assert_eq!(parsed["status"], "ok");
    assert!(parsed["duration"]["total_ms"].is_number());
}

#[test]
fn log_macros_accumulate() {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let c = captured.clone();
    let _guard = EventKeyGuard::new_with_emit(move |ev| {
        *c.lock().unwrap() = Some(ev.to_json().unwrap());
    });

    info!("request received");
    warn!("upstream slow");
    error!("request failed");

    drop(_guard);

    let json = captured.lock().unwrap().clone().unwrap();
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
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
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let c = captured.clone();
    let _guard = EventKeyGuard::new_with_emit(move |ev| {
        *c.lock().unwrap() = Some(ev.to_json().unwrap());
    });

    info!("request {}", 42);
    warn!("retry {}/{}", 1, 3);

    drop(_guard);

    let json = captured.lock().unwrap().clone().unwrap();
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
    let log = &parsed["log"];
    assert_eq!(log[0]["message"], "request 42");
    assert_eq!(log[1]["message"], "retry 1/3");
}

#[test]
fn nested_path_set() {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let c = captured.clone();
    let _guard = EventKeyGuard::new_with_emit(move |ev| {
        *c.lock().unwrap() = Some(ev.to_json().unwrap());
    });

    wl_set!("service.name", "nested-svc");

    drop(_guard);

    let json = captured.lock().unwrap().clone().unwrap();
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
    assert_eq!(parsed["service"]["name"], "nested-svc");
    assert_eq!(parsed["service"]["version"], "1.0.0");
}

#[test]
fn counter_inc_and_dec() {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let c = captured.clone();
    let _guard = EventKeyGuard::new_with_emit(move |ev| {
        *c.lock().unwrap() = Some(ev.to_json().unwrap());
    });

    wl_inc!("requests");
    wl_inc!("requests");
    wl_inc!("requests");
    wl_dec!("requests");

    drop(_guard);

    let json = captured.lock().unwrap().clone().unwrap();
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
    assert_eq!(parsed["requests"], 2);
}

#[test]
fn wl_add_works() {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let c = captured.clone();
    let _guard = EventKeyGuard::new_with_emit(move |ev| {
        *c.lock().unwrap() = Some(ev.to_json().unwrap());
    });

    wl_add!("requests", 10);
    wl_add!("requests", -3);

    drop(_guard);

    let json = captured.lock().unwrap().clone().unwrap();
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
    assert_eq!(parsed["requests"], 7);
}

#[test]
fn wl_null_works() {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let c = captured.clone();
    let _guard = EventKeyGuard::new_with_emit(move |ev| {
        *c.lock().unwrap() = Some(ev.to_json().unwrap());
    });

    wl_null!("status");

    drop(_guard);

    let json = captured.lock().unwrap().clone().unwrap();
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
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
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let c = captured.clone();
    let _guard = EventKeyGuard::new_with_emit(move |ev| {
        *c.lock().unwrap() = Some(ev.to_json().unwrap());
    });

    drop(_guard);

    let json = captured.lock().unwrap().clone().unwrap();
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
    assert_eq!(parsed["service"]["version"], "1.0.0");
}

#[test]
fn no_log_key_when_no_log_entries() {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let c = captured.clone();
    let _guard = EventKeyGuard::new_with_emit(move |ev| {
        *c.lock().unwrap() = Some(ev.to_json().unwrap());
    });

    wl_set!("status", "ok");

    drop(_guard);

    let json = captured.lock().unwrap().clone().unwrap();
    assert!(!json.contains("\"log\""));
}

#[test]
fn duration_auto_added() {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let c = captured.clone();
    let _guard = EventKeyGuard::new_with_emit(move |ev| {
        *c.lock().unwrap() = Some(ev.to_json().unwrap());
    });

    std::thread::sleep(std::time::Duration::from_millis(2));

    drop(_guard);

    let json = captured.lock().unwrap().clone().unwrap();
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
    use sonic_rs::JsonValueTrait;
    let total_ms = parsed["duration"]["total_ms"].as_u64().unwrap();
    assert!(total_ms >= 1, "duration.total_ms should be >= 1, got {total_ms}");
}