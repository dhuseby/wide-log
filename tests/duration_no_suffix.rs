//! Verifies a `duration!` leaf with an unrecognized suffix (no `_<unit>`
//! trailing segment) defaults to milliseconds as f64.

use wide_log::wide_log;

wide_log!({
    "duration": { "wall": duration! }
});

mod common;

#[test]
fn unrecognized_suffix_defaults_to_ms_f64() {
    let (slot, emit) = common::make_capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    let elapsed = common::sleep_20ms();
    drop(_guard);

    let parsed = common::parse(&slot);
    let v = common::as_f64(&parsed, &["duration", "wall"]);
    assert!(v > 0.0, "duration.wall should be positive, got {v}");
    let expected = elapsed.as_millis() as f64;
    assert!(
        v >= expected * 0.5 && v <= expected * 2.0 + 100.0,
        "duration.wall ({v}) should default to ms near {expected} \
         (within 2x + 100ms tolerance)"
    );
}
