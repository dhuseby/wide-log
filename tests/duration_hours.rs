//! Verifies `"total_h": duration!` renders the elapsed time as f64 hours.

use wide_log::wide_log;

wide_log!({
    "duration": { "total_h": duration! }
});

mod common;

#[test]
fn renders_as_f64_hours() {
    let (slot, emit) = common::make_capture();
    let _guard = WideLogGuard::builder().with_emit(emit).build();
    let elapsed = common::sleep_20ms();
    drop(_guard);

    let parsed = common::parse(&slot);
    let v = common::as_f64(&parsed, &["duration", "total_h"]);
    assert!(v > 0.0, "duration.total_h should be positive, got {v}");
    let expected = elapsed.as_secs_f64() / 3600.0;
    assert!(
        v >= expected * 0.5 && v <= expected * 2.0 + 1.0,
        "duration.total_h ({v}) should be near {expected} h \
         (within 2x + 1h tolerance)"
    );
    // 20ms is about 0.0000056 hours — must be well under 1.0.
    assert!(
        v < 1.0,
        "duration.total_h ({v}) must be < 1.0 for a 20ms sleep; \
         a value near 20.0 indicates the guard stored ms under _h"
    );
}
