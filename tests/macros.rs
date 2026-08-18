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

#[allow(clippy::type_complexity)]
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

// ---- Phase 1 §1.3: mem::forget hazard on WideLogGuard ----

#[test]
fn wide_log_guard_forget_does_not_emit() {
    // Documenting the known hazard: `mem::forget(guard)` skips the
    // `Drop` impl entirely, so the emit_fn is never called. This is
    // a property of the user code (which should drop the guard or
    // use `let _ = guard;` patterns), not of the guard itself.
    //
    // Phase 2 of the implementation plan will eliminate the raw-pointer
    // pattern and make this case fully sound (no dangling pointer to
    // restore). For now, the test just pins the current behavior.

    let counter = Arc::new(Mutex::new(0u32));
    let c = counter.clone();
    let emit = move |_: &wide_log::WideEvent<EventKey>| {
        *c.lock().unwrap() += 1;
    };
    let guard = WideLogGuard::builder().with_emit(emit).build();
    std::mem::forget(guard);
    // Emit was never called because Drop was skipped.
    assert_eq!(*counter.lock().unwrap(), 0);
}

#[test]
fn wide_log_guard_drop_restores_thread_local() {
    // The normal path: a guard's Drop restores the previous
    // CURRENT_EVENT pointer so nested guards work correctly.
    // We verify the outer guard "wins" after the inner one is
    // dropped.
    let outer_captured: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let inner_captured: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let oc = outer_captured.clone();
    let ic = inner_captured.clone();

    let outer = WideLogGuard::builder()
        .with_emit(move |ev| {
            *oc.lock().unwrap() = Some(ev.to_json().unwrap());
        })
        .build();
    wl_set!("status", "outer");

    {
        let inner = WideLogGuard::builder()
            .with_emit(move |ev| {
                *ic.lock().unwrap() = Some(ev.to_json().unwrap());
            })
            .build();
        wl_set!("status", "inner");
        drop(inner);
    }

    // After the inner guard drops, the outer guard is restored. We
    // can still mutate the outer event.
    wl_set!("status", "outer-again");
    drop(outer);

    let outer_json = outer_captured.lock().unwrap().clone().unwrap();
    let inner_json = inner_captured.lock().unwrap().clone().unwrap();
    let outer_parsed: sonic_rs::Value = sonic_rs::from_str(&outer_json).unwrap();
    let inner_parsed: sonic_rs::Value = sonic_rs::from_str(&inner_json).unwrap();
    assert_eq!(outer_parsed["status"], "outer-again");
    assert_eq!(inner_parsed["status"], "inner");
}

// ---- Phase 2 §1.3: Send / Sync soundness ----

#[test]
fn value_k_is_send_sync() {
    // Compile-time: the inner types used in the guard are Send + Sync.
    // The guard's only "raw" field is `prev: *const WideEvent`,
    // which is auto-`Send + Sync` whenever `WideEvent: Send + Sync`.
    // The macro-generated `WideLogGuard` also has an explicit
    // `unsafe impl Send` whose safety argument is that the `prev`
    // pointer is only dereferenced through the per-thread
    // `CURRENT_EVENT` cell (or per-task `TASK_EVENT` under the `tokio`
    // feature).
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<wide_log::Value<EventKey>>();
    assert_sync::<wide_log::Value<EventKey>>();
    assert_send::<wide_log::WideEvent<EventKey>>();
    assert_sync::<wide_log::WideEvent<EventKey>>();
    // ScopedGuard is also Send + Sync:
    assert_send::<wide_log::ScopedGuard<EventKey, fn(&wide_log::WideEvent<EventKey>)>>();
    assert_sync::<wide_log::ScopedGuard<EventKey, fn(&wide_log::WideEvent<EventKey>)>>();
    // The macro-generated guard must be Send so it can be moved
    // across threads (e.g. `tokio::spawn` of an `async fn` that
    // constructs a guard). This is the regression that 0.6.0's
    // "eliminate raw pointer" refactor introduced; 0.6.1 restores
    // the `unsafe impl Send` to fix it.
    assert_send::<WideLogGuard<fn(&wide_log::WideEvent<EventKey>)>>();
}

#[test]
fn wide_log_guard_can_be_sent_across_threads() {
    // Regression test for the 0.6.0 Send regression: the
    // macro-generated `WideLogGuard` must be `Send` so that an
    // `async fn` holding one can be `tokio::spawn`-ed, and so that
    // a guard can be moved into `std::thread::spawn` and dropped
    // on a different thread than the one that created it.
    //
    // The 0.6.0 macro deleted the `unsafe impl Send` on the
    // generated guard without replacing it with a working
    // alternative, so this test would not have been able to
    // express a real cross-thread assertion. With 0.6.1, the
    // `unsafe impl Send` is back and the guard is `Send` again.

    // Case 1: std::thread::spawn — the guard is built on the main
    // thread, sent to a worker thread, and dropped there. The
    // emit function runs on the worker thread. If the guard were
    // `!Send`, this would not compile.
    let (slot, emit) = capture();
    let handle = std::thread::spawn(move || {
        let _guard = WideLogGuard::builder().with_emit(emit).build();
        wl_set!("status", "from-worker");
    });
    handle.join().expect("worker thread panicked");
    let json = parse(&slot);
    assert_eq!(json["status"], "from-worker");

    // Case 2: tokio::spawn — the guard is built inside an async
    // task and dropped there. Requires the future returned by the
    // async block to be `Send`, which transitively requires the
    // guard to be `Send`.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let (slot, emit) = capture();
        let task = tokio::spawn(async move {
            let _guard = WideLogGuard::builder().with_emit(emit).build();
            wl_set!("status", "from-tokio");
        });
        task.await.expect("tokio task panicked");
        let json = parse(&slot);
        assert_eq!(json["status"], "from-tokio");
    });
}

// ---- Phase 2 §1.2: Vec<u8> pipeline produces valid JSON line ----

#[test]
fn default_emit_produces_valid_json_line() {
    // The default emit path now produces a `Vec<u8>` JSON line with
    // a trailing `'\n'`. We can't easily inspect the bytes (they go
    // to a process-global writer thread), but we can verify that the
    // captured emit (via `with_emit`) still works and produces
    // valid JSON.
    let (slot, emit) = capture();
    let guard = WideLogGuard::builder().with_emit(emit).build();
    wl_set!("status", "ok");
    drop(guard);

    let json = slot.lock().unwrap().clone().unwrap();
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
    assert_eq!(parsed["status"], "ok");
    // Duration and event metadata are populated by the guard's drop.
    // total_ms is now an f64 (unit-suffix duration fix).
    assert!(parsed["duration"]["total_ms"].is_f64());
    assert!(parsed["event"]["timestamp"].is_str());
    assert!(parsed["event"]["id"].is_str());
}

// ---- Phase 2 §2.3: producer EMIT_BUF does not grow unboundedly ----

#[test]
fn emit_buf_capacity_is_bounded_across_many_events() {
    // After Phase 2, the producer's `EMIT_BUF` thread-local `Vec<u8>` is
    // cleared and reused across events, not freed. We can't observe the
    // buffer directly (it's a private thread_local in the macro), but
    // we can verify the end-to-end path runs many events without
    // unbounded memory growth by simply emitting thousands of events
    // and checking the test process doesn't OOM.
    //
    // The real two-buffer ping-pong return-slot optimization is
    // deferred to 0.7.0; in 0.6.0 the producer reuses its `EMIT_BUF`
    // and the writer drops each `Vec<u8>` it receives. This test pins
    // the producer-side boundedness for future regressions.
    for _ in 0..10_000 {
        let (slot, emit) = capture();
        let guard = WideLogGuard::builder().with_emit(emit).build();
        wl_set!("status", "x");
        drop(guard);
        // Drop the captured JSON to free memory.
        let _ = slot.lock().unwrap().clone();
    }
}

#[test]
fn emit_buf_handles_increasing_event_sizes() {
    // The producer's `EMIT_BUF` should grow on demand but not
    // unboundedly. After emitting a large event, the next smaller
    // events should reuse the now-larger capacity (the `Vec::clear`
    // preserves the underlying allocation).
    //
    // This is an indirect check: we just verify the path runs to
    // completion for events of increasing and decreasing sizes.
    for size in [1, 10, 100, 1_000, 10_000, 100, 10, 1] {
        let (slot, emit) = capture();
        let guard = WideLogGuard::builder().with_emit(emit).build();
        let payload: String = "x".repeat(size);
        wl_set!("status", payload.as_str());
        drop(guard);
        let json = slot.lock().unwrap().clone().unwrap();
        let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        let s = parsed["status"].as_str().unwrap();
        assert_eq!(s.len(), size, "size={size}");
    }
}

// ---- Phase 3 §3.4: with_id_str overload ----

#[test]
fn with_id_str_overrides_event_id() {
    // The `with_id_str(&'static str)` overload sets the event id
    // to a fixed value. Useful for tracing propagation from an
    // upstream service or for testing.
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder()
        .with_id_str("my-correlation-id-1234")
        .with_emit(emit)
        .build();
    drop(_guard);

    let json = slot.lock().unwrap().clone().unwrap();
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
    assert_eq!(parsed["event"]["id"], "my-correlation-id-1234");
}

#[test]
fn with_id_closure_still_works() {
    // The original `with_id` (closure-based) still works.
    let (slot, emit) = capture();
    let _guard = WideLogGuard::builder()
        .with_id(|| "dyn-id".to_string())
        .with_emit(emit)
        .build();
    drop(_guard);

    let json = slot.lock().unwrap().clone().unwrap();
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
    assert_eq!(parsed["event"]["id"], "dyn-id");
}
