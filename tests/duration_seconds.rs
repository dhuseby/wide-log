//! Verifies `"total_s": duration!` renders the elapsed time as f64 seconds.

use wide_log::wide_log;

wide_log!({
    "duration": { "total_s": duration! }
});

mod common;

#[test]
fn renders_as_f64_seconds() {
    let (slot, emit) = common::make_capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    let elapsed = common::sleep_20ms();
    drop(_guard);

    let parsed = common::parse(&slot);
    let v = common::as_f64(&parsed, &["duration", "total_s"]);
    assert!(v > 0.0, "duration.total_s should be positive, got {v}");
    let expected = elapsed.as_secs_f64();
    assert!(
        v >= expected * 0.5 && v <= expected * 2.0 + 1.0,
        "duration.total_s ({v}) should be near {expected} s (within 2x + 1s tolerance)"
    );
    // A 20ms sleep must not yield ~20.0 in the seconds field — that
    // would indicate the guard stored milliseconds under the _s key.
    assert!(
        v < 1.0,
        "duration.total_s ({v}) must be < 1.0 for a 20ms sleep; \
         a value near 20.0 indicates the guard stored ms under _s"
    );
}
