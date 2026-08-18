//! Verifies `"total_us": duration!` renders the elapsed time as f64 microseconds.

use wide_log::wide_log;

wide_log!({
    "duration": { "total_us": duration! }
});

mod common;

#[test]
fn renders_as_f64_micros() {
    let (slot, emit) = common::make_capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    let elapsed = common::sleep_20ms();
    drop(_guard);

    let parsed = common::parse(&slot);
    let v = common::as_f64(&parsed, &["duration", "total_us"]);
    assert!(v > 0.0, "duration.total_us should be positive, got {v}");
    let expected = elapsed.as_micros() as f64;
    assert!(
        v >= expected * 0.5 && v <= expected * 2.0 + 100_000.0,
        "duration.total_us ({v}) should be near {expected} us \
         (within 2x + 100000us tolerance)"
    );
    // A 20ms sleep must not yield ~20.0 in the micros field — that
    // would indicate the guard stored milliseconds under the _us key.
    assert!(
        v > 1000.0,
        "duration.total_us ({v}) must be > 1000.0 for a 20ms sleep; \
         a value near 20.0 indicates the guard stored ms under _us"
    );
}
