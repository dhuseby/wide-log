use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
    "retries": counter!,
    "status": null,
    "flag": null,
});

use sonic_rs::{JsonContainerTrait, JsonValueTrait};
use std::sync::{Arc, Mutex};

type CaptureSlot = Arc<Mutex<Option<String>>>;

fn capture() -> (
    CaptureSlot,
    impl FnOnce(&wide_log::WideEvent<EventKey>) + Send + 'static,
) {
    let slot: CaptureSlot = Arc::new(Mutex::new(None));
    let s = slot.clone();
    let emit = move |we: &wide_log::WideEvent<EventKey>| {
        *s.lock().unwrap() = Some(we.to_json().unwrap());
    };
    (slot, emit)
}

fn parse(slot: &CaptureSlot) -> sonic_rs::Value {
    let json = slot.lock().unwrap().clone().unwrap();
    sonic_rs::from_str(&json).unwrap()
}

// ---- §5.1: All log-level macros work ----

#[test]
fn all_log_level_macros_literal() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    info!("info message");
    warn!("warn message");
    error!("error message");
    debug!("debug message");
    trace!("trace message");

    drop(_guard);

    let parsed = parse(&slot);
    let log = parsed["log"].as_array().unwrap();
    assert_eq!(log.len(), 5);
    assert_eq!(log[0]["level"], "info");
    assert_eq!(log[0]["message"], "info message");
    assert_eq!(log[1]["level"], "warn");
    assert_eq!(log[1]["message"], "warn message");
    assert_eq!(log[2]["level"], "error");
    assert_eq!(log[2]["message"], "error message");
    assert_eq!(log[3]["level"], "debug");
    assert_eq!(log[3]["message"], "debug message");
    assert_eq!(log[4]["level"], "trace");
    assert_eq!(log[4]["message"], "trace message");
}

#[test]
fn all_log_level_macros_format_args() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    info!("info {}", 1);
    warn!("warn {}", 2);
    error!("error {}", 3);
    debug!("debug {}", 4);
    trace!("trace {}", 5);

    drop(_guard);

    let parsed = parse(&slot);
    let log = parsed["log"].as_array().unwrap();
    assert_eq!(log[0]["message"], "info 1");
    assert_eq!(log[1]["message"], "warn 2");
    assert_eq!(log[2]["message"], "error 3");
    assert_eq!(log[3]["message"], "debug 4");
    assert_eq!(log[4]["message"], "trace 5");
}

#[test]
fn log_entries_accumulate_in_order() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    info!("first");
    warn!("second");
    info!("third");
    error!("fourth");
    debug!("fifth");
    trace!("sixth");
    info!("seventh");

    drop(_guard);

    let parsed = parse(&slot);
    let log = parsed["log"].as_array().unwrap();
    assert_eq!(log.len(), 7);
    assert_eq!(log[0]["message"], "first");
    assert_eq!(log[1]["message"], "second");
    assert_eq!(log[2]["message"], "third");
    assert_eq!(log[3]["message"], "fourth");
    assert_eq!(log[4]["message"], "fifth");
    assert_eq!(log[5]["message"], "sixth");
    assert_eq!(log[6]["message"], "seventh");
}

// ---- §5.2: wl_set! with all value types ----

#[test]
fn wl_set_all_value_types() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    wl_set!("service.name", "string-val");
    wl_set!("status", "ok");
    wl_set!("flag", true);
    wl_set!("requests", 42u64);
    wl_set!("retries", -7i64);

    drop(_guard);

    let parsed = parse(&slot);
    assert_eq!(parsed["service"]["name"], "string-val");
    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["flag"], true);
    assert_eq!(parsed["requests"], 42);
    assert_eq!(parsed["retries"], -7);
}

#[test]
fn wl_set_overwrites_existing() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    wl_set!("status", "first");
    wl_set!("status", "second");
    wl_set!("status", "third");

    drop(_guard);

    let parsed = parse(&slot);
    assert_eq!(parsed["status"], "third");
}

#[test]
fn wl_set_with_string_value() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    let owned = String::from("owned-string");
    wl_set!("service.name", owned);
    wl_set!("status", "borrowed");

    drop(_guard);

    let parsed = parse(&slot);
    assert_eq!(parsed["service"]["name"], "owned-string");
    assert_eq!(parsed["status"], "borrowed");
}

#[test]
fn wl_set_with_unit_sets_null() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    wl_set!("status", "not-null");
    wl_set!("status", ());

    drop(_guard);

    let parsed = parse(&slot);
    assert!(parsed["status"].is_null());
}

// ---- §5.3: wl_inc! / wl_dec! / wl_add! ----

#[test]
fn wl_inc_initializes_to_one() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    wl_inc!("requests");
    drop(_guard);
    let parsed = parse(&slot);
    assert_eq!(parsed["requests"], 1);
}

#[test]
fn wl_inc_increments_existing_u64() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    wl_set!("requests", 10u64);
    wl_inc!("requests");
    wl_inc!("requests");
    drop(_guard);
    let parsed = parse(&slot);
    assert_eq!(parsed["requests"], 12);
}

#[test]
fn wl_dec_initializes_to_minus_one() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    wl_dec!("retries");
    drop(_guard);
    let parsed = parse(&slot);
    assert_eq!(parsed["retries"], -1);
}

#[test]
fn wl_dec_decrements_existing() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    wl_set!("retries", 5u64);
    wl_dec!("retries");
    wl_dec!("retries");
    drop(_guard);
    let parsed = parse(&slot);
    assert_eq!(parsed["retries"], 3);
}

#[test]
fn wl_dec_does_not_go_negative_from_u64() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    wl_set!("retries", 1u64);
    wl_dec!("retries");
    drop(_guard);
    let parsed = parse(&slot);
    assert_eq!(parsed["retries"], 0);
}

#[test]
fn wl_dec_from_i64() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    wl_set!("retries", -3i64);
    wl_dec!("retries");
    drop(_guard);
    let parsed = parse(&slot);
    assert_eq!(parsed["retries"], -4);
}

#[test]
fn wl_add_positive_to_absent() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    wl_add!("requests", 42);
    drop(_guard);
    let parsed = parse(&slot);
    assert_eq!(parsed["requests"], 42);
}

#[test]
fn wl_add_negative_to_absent() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    wl_add!("retries", -5);
    drop(_guard);
    let parsed = parse(&slot);
    assert_eq!(parsed["retries"], -5);
}

#[test]
fn wl_add_to_existing_u64() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    wl_set!("requests", 100u64);
    wl_add!("requests", 50);
    wl_add!("requests", -30);
    drop(_guard);
    let parsed = parse(&slot);
    assert_eq!(parsed["requests"], 120);
}

#[test]
fn wl_add_to_non_numeric_overwrites() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    wl_set!("status", "ok");
    wl_add!("status", 5);
    drop(_guard);
    let parsed = parse(&slot);
    assert_eq!(parsed["status"], 5);
}

// ---- §5.4: wl_null! ----

#[test]
fn wl_null_sets_null() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    wl_null!("status");
    drop(_guard);
    let parsed = parse(&slot);
    assert!(parsed["status"].is_null());
}

#[test]
fn wl_null_overwrites_existing() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    wl_set!("status", "ok");
    wl_null!("status");
    drop(_guard);
    let parsed = parse(&slot);
    assert!(parsed["status"].is_null());
}

#[test]
fn wl_null_nested_path() {
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    wl_null!("service.name");
    drop(_guard);
    let parsed = parse(&slot);
    assert!(parsed["service"]["name"].is_null());
}

// ---- All macros are no-ops without a guard ----

#[test]
fn all_macros_noop_without_guard() {
    // None of these should panic.
    wl_set!("service.name", "noop");
    wl_set!("status", 42u64);
    wl_set!("flag", true);
    wl_inc!("requests");
    wl_dec!("retries");
    wl_add!("requests", 5);
    wl_null!("status");
    info!("info noop");
    warn!("warn noop");
    error!("error noop");
    debug!("debug noop");
    trace!("trace noop");

    // Format-arg variants:
    info!("info {} noop", 1);
    warn!("warn {} noop", 2);
    error!("error {} noop", 3);
    debug!("debug {} noop", 4);
    trace!("trace {} noop", 5);

    // current() should be None.
    assert!(current().is_none());
}

// ---- §5.5: info! shadowing tracing::info! ----

#[test]
fn info_shadows_tracing_info() {
    // When both `wide_log::info!` and `tracing::info!` are available,
    // `info!` should resolve to the wide-log macro (appending to the event),
    // while `::tracing::info!` (fully qualified) should call the real tracing macro.
    //
    // We verify this by:
    // 1. Using `info!` (unqualified) → should append to the wide event's log list.
    // 2. Using `::tracing::info!` → should NOT append to the log list.

    // Bring tracing's info! into scope so it could be shadowed.
    #[allow(unused_imports)]
    use tracing::info as _;

    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    // This should use the wide-log `info!` macro (defined by `wide_log!`).
    info!("shadowed by wide-log");

    // This should use the real `tracing::info!` (fully qualified).
    ::tracing::info!("this goes to tracing, not the log list");

    // Another wide-log info! call.
    info!("second wide-log entry");

    drop(_guard);

    let parsed = parse(&slot);
    let log = parsed["log"].as_array().unwrap();
    assert_eq!(
        log.len(),
        2,
        "only wide-log info! calls should be in the log list"
    );
    assert_eq!(log[0]["message"], "shadowed by wide-log");
    assert_eq!(log[1]["message"], "second wide-log entry");
}

#[test]
fn all_log_macros_shadow_tracing() {
    // Bring all tracing log macros into scope.
    #[allow(unused_imports)]
    use tracing::{debug as _, error as _, info as _, trace as _, warn as _};

    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();

    // Unqualified calls should use wide-log macros.
    info!("i");
    warn!("w");
    error!("e");
    debug!("d");
    trace!("t");

    // Fully qualified calls should use tracing.
    ::tracing::info!("ti");
    ::tracing::warn!("tw");
    ::tracing::error!("te");
    ::tracing::debug!("td");
    ::tracing::trace!("tt");

    drop(_guard);

    let parsed = parse(&slot);
    let log = parsed["log"].as_array().unwrap();
    assert_eq!(log.len(), 5, "only 5 wide-log entries, not 10");
    assert_eq!(log[0]["message"], "i");
    assert_eq!(log[1]["message"], "w");
    assert_eq!(log[2]["message"], "e");
    assert_eq!(log[3]["message"], "d");
    assert_eq!(log[4]["message"], "t");
}

// ---- Verify ::tracing::info! in default_emit calls the real tracing macro ----

#[test]
fn default_emit_uses_real_tracing_macro() {
    // Just verify builder().build() + info! + drop works without panic.
    let _guard = WideLogGuard::builder().build();
    info!("test message via default emit");
    drop(_guard);
    // If default_emit used the shadowing info!, it would be a no-op
    // (current() is None during emit). The event would still be serialized
    // and sent to tracing. So the test passing means no panic/stack overflow.
}

#[test]
fn default_emit_does_not_cause_infinite_recursion() {
    let _guard = WideLogGuard::builder().build();

    // Call the real tracing::info! — this should go to tracing, not the log list.
    ::tracing::info!("real tracing message");

    // Call the wide-log info! — this should go to the log list.
    info!("wide-log message");

    drop(_guard);
    // No panic = success. The guard drops, default_emit calls ::tracing::info!,
    // which outputs the JSON event to tracing.
}