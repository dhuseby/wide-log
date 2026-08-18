//! Verifies `"total_ns": duration!` renders the elapsed time as f64 nanoseconds.

use wide_log::wide_log;

wide_log!({
    "duration": { "total_ns": duration! }
});

mod common;

#[test]
fn renders_as_f64_nanos() {
    let (slot, emit) = common::make_capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    let elapsed = common::sleep_20ms();
    drop(_guard);

    let parsed = common::parse(&slot);
    let v = common::as_f64(&parsed, &["duration", "total_ns"]);
    assert!(v > 0.0, "duration.total_ns should be positive, got {v}");
    let expected = elapsed.as_nanos() as f64;
    assert!(
        v >= expected * 0.5 && v <= expected * 2.0 + 100_000_000.0,
        "duration.total_ns ({v}) should be near {expected} ns \
         (within 2x + 100000000ns tolerance)"
    );
    // A 20ms sleep must not yield ~20.0 in the nanos field.
    assert!(
        v > 1_000_000.0,
        "duration.total_ns ({v}) must be > 1000000.0 for a 20ms sleep; \
         a value near 20.0 indicates the guard stored ms under _ns"
    );
}
