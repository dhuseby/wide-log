//! Verifies `"total_m": duration!` renders the elapsed time as f64 minutes.

use wide_log::wide_log;

wide_log!({
    "duration": { "total_m": duration! }
});

mod common;

#[test]
fn renders_as_f64_minutes() {
    let (slot, emit) = common::make_capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    let elapsed = common::sleep_20ms();
    drop(_guard);

    let parsed = common::parse(&slot);
    let v = common::as_f64(&parsed, &["duration", "total_m"]);
    assert!(v > 0.0, "duration.total_m should be positive, got {v}");
    let expected = elapsed.as_secs_f64() / 60.0;
    assert!(
        v >= expected * 0.5 && v <= expected * 2.0 + 1.0,
        "duration.total_m ({v}) should be near {expected} min \
         (within 2x + 1min tolerance)"
    );
    // 20ms is about 0.00033 minutes — must be well under 1.0.
    assert!(
        v < 1.0,
        "duration.total_m ({v}) must be < 1.0 for a 20ms sleep; \
         a value near 20.0 indicates the guard stored ms under _m"
    );
}
