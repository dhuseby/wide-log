//! Verifies `"total_ms": duration!` renders the elapsed time as f64 milliseconds.

use wide_log::wide_log;

wide_log!({
    "duration": { "total_ms": duration! }
});

mod common;

#[test]
fn renders_as_f64_millis() {
    let (slot, emit) = common::make_capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    let elapsed = common::sleep_20ms();
    drop(_guard);

    let parsed = common::parse(&slot);
    let v = common::as_f64(&parsed, &["duration", "total_ms"]);
    assert!(v > 0.0, "duration.total_ms should be positive, got {v}");
    let expected = elapsed.as_millis() as f64;
    assert!(
        v >= expected * 0.5 && v <= expected * 2.0 + 100.0,
        "duration.total_ms ({v}) should be near {expected} ms \
         (within 2x + 100ms tolerance)"
    );
}
