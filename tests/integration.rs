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
    assert!(
        total_ms >= 1,
        "duration.total_ms should be >= 1, got {total_ms}"
    );
}

// ---- Nested scope tests (§4.1) ----

#[test]
fn nested_sync_scopes_innermost_accessible() {
    let outer_captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let inner_captured = std::sync::Arc::new(std::sync::Mutex::new(None));

    let oc = outer_captured.clone();
    let _outer = EventKeyGuard::new_with_emit(move |ev| {
        *oc.lock().unwrap() = Some(ev.to_json().unwrap());
    });

    wl_set!("status", "outer");
    assert_eq!(current().map(|e| e.len()).unwrap_or(0), 2); // service.version default + status

    {
        let ic = inner_captured.clone();
        let _inner = EventKeyGuard::new_with_emit(move |ev| {
            *ic.lock().unwrap() = Some(ev.to_json().unwrap());
        });

        // Inner scope: current() returns the inner event, not the outer.
        wl_set!("status", "inner");
        assert_eq!(current().map(|e| e.len()).unwrap_or(0), 2); // service.version default + status

        info!("inner log");
    }

    // After inner drop: outer event is restored.
    // The outer event should still have status="outer" (not "inner").
    assert_eq!(current().map(|e| e.len()).unwrap_or(0), 2);

    drop(_outer);

    let outer_json = outer_captured.lock().unwrap().clone().unwrap();
    let inner_json = inner_captured.lock().unwrap().clone().unwrap();

    let outer_parsed: sonic_rs::Value = sonic_rs::from_str(&outer_json).unwrap();
    let inner_parsed: sonic_rs::Value = sonic_rs::from_str(&inner_json).unwrap();

    assert_eq!(outer_parsed["status"], "outer");
    assert_eq!(inner_parsed["status"], "inner");
    // Outer event has no log entries (info! went to inner event).
    assert!(outer_parsed.get("log").is_none() || outer_parsed["log"].is_null());
    assert!(inner_parsed["log"].is_array());
}

#[test]
fn nested_sync_scopes_outer_restored_after_inner_drop() {
    let outer_captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let inner_captured = std::sync::Arc::new(std::sync::Mutex::new(None));

    let oc = outer_captured.clone();
    let _outer = EventKeyGuard::new_with_emit(move |ev| {
        *oc.lock().unwrap() = Some(ev.to_json().unwrap());
    });

    wl_inc!("requests");

    {
        let ic = inner_captured.clone();
        let _inner = EventKeyGuard::new_with_emit(move |ev| {
            *ic.lock().unwrap() = Some(ev.to_json().unwrap());
        });

        // Inner event is separate — inc doesn't affect outer.
        wl_inc!("requests");
        wl_inc!("requests");
    }

    // Outer event should still have requests=1 (plus service.version default).
    assert_eq!(current().map(|e| e.len()).unwrap_or(0), 2);

    drop(_outer);

    let outer_json = outer_captured.lock().unwrap().clone().unwrap();
    let inner_json = inner_captured.lock().unwrap().clone().unwrap();

    let outer_parsed: sonic_rs::Value = sonic_rs::from_str(&outer_json).unwrap();
    let inner_parsed: sonic_rs::Value = sonic_rs::from_str(&inner_json).unwrap();

    assert_eq!(outer_parsed["requests"], 1);
    assert_eq!(inner_parsed["requests"], 2);
}

#[test]
fn current_is_none_without_guard() {
    assert!(current().is_none());
}

#[test]
fn current_is_some_with_guard() {
    let _guard = EventKeyGuard::new_with_emit(|_| {});
    assert!(current().is_some());
    drop(_guard);
    assert!(current().is_none());
}
