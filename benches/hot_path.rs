//! Hot-path benchmarks.
//!
//! These benchmarks isolate the cost of individual hot-path
//! operations and the buffers behind them:
//!
//! - Reusable thread-local `FMT_BUF` for `info!`/`warn!`/`error!`/
//!   `debug!`/`trace!` with format args (eliminates the
//!   `String::with_capacity(64)` allocation per call).
//! - Reusable thread-local `ULID_BUF` for the default event id
//!   (eliminates the per-guard ULID `String` allocation).
//! - `with_id_str(&'static str)` overload (avoids the
//!   `Box<dyn FnOnce>` indirection for fixed ids).
//!
//! The full-lifecycle end-to-end benchmark lives in
//! `benches/core.rs`; this file focuses on the per-operation cost
//! of each optimization.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::{Arc, Mutex};
use wide_log::wide_log;

#[cfg(feature = "tracing")]
#[allow(unused_imports)]
use wide_log::{debug, error, info, trace, warn};

#[cfg(feature = "tracing")]
fn init_capture() {
    use tracing_subscriber::prelude::*;
    // Leaked on purpose: the subscriber must outlive every bench
    // iteration, and the bench harness runs each function once per
    // process.
    let guard = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(crate::WideLogCaptureLayer::new()),
    );
    std::mem::forget(guard);
}

#[cfg(not(feature = "tracing"))]
fn init_capture() {}

// A separate schema from `benches/core.rs` to make this file
// self-contained. Includes nested objects (which exercise the
// existing-child reuse path on the guard's drop) and a counter.
wide_log!({
    "service": {
        "name": null,
        "version": null,
    },
    "request": {
        "method": null,
        "path": null,
        "status": null,
    },
    "requests": counter!,
});

// No-op emit — isolates accumulation cost from serialization.
fn noop_emit(_ev: &wide_log::WideEvent<EventKey>) {}

// ---------- FMT_BUF ----------

/// A single `info!` call with format args, backed by the
/// reusable thread-local `FMT_BUF` (one allocation, reused
/// across calls).
fn bench_fmt_buf(c: &mut Criterion) {
    init_capture();
    let mut group = c.benchmark_group("phase3_fmt_buf");

    // info! with format args: a single dynamic allocation reused
    // across calls.
    group.bench_function("info_format_args", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::builder().with_emit(noop_emit).build();
            info!("user {} did {} in {}ms", 42, "login", 15);
            drop(black_box(_guard));
        })
    });

    // info! literal: zero-alloc; measured to catch regressions.
    group.bench_function("info_literal", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::builder().with_emit(noop_emit).build();
            info!("request received");
            drop(black_box(_guard));
        })
    });

    // 10 info! calls: amortized cost should be much lower than 10x
    // the single-call cost.
    group.bench_function("info_format_args_10x", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::builder().with_emit(noop_emit).build();
            for i in 0..10 {
                info!("entry {} of 10 with arg {}", i, i * 2);
            }
            drop(black_box(_guard));
        })
    });

    group.finish();
}

// ---------- ULID_BUF ----------

/// The default event id generator, backed by the reusable
/// thread-local `ULID_BUF` (one allocation, reused across
/// guards).
fn bench_ulid_buf(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase3_ulid_buf");

    // Default id (ULID): single guard create+drop, zero steady-state
    // allocation (the buffer is reused across guards).
    group.bench_function("default_id_create_drop", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::builder().with_emit(noop_emit).build();
            drop(black_box(_guard));
        })
    });

    // 1000 guards in a row: amortized cost should approach zero
    // (one initial buffer allocation, then reuses).
    group.bench_function("default_id_1000_guards", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                let _guard = WideLogGuard::builder().with_emit(noop_emit).build();
                drop(black_box(_guard));
            }
        })
    });

    group.finish();
}

// ---------- with_id_str ----------

/// `with_id_str` vs `with_id` (closure).
///
/// `with_id_str` is the `&'static str` overload that avoids the
/// `Box<dyn FnOnce>` indirection. The closure-based `with_id` is
/// still available for dynamic ids.
fn bench_with_id_str(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase3_with_id");

    // with_id_str: new overload, zero-indirection.
    group.bench_function("with_id_str", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::builder()
                .with_id_str("fixed-correlation-id")
                .with_emit(noop_emit)
                .build();
            drop(black_box(_guard));
        })
    });

    // with_id (closure): existing API. We measure to confirm that
    // the dynamic path is not much slower than the static path.
    group.bench_function("with_id_closure", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::builder()
                .with_id(|| "dynamic-id".to_string())
                .with_emit(noop_emit)
                .build();
            drop(black_box(_guard));
        })
    });

    group.finish();
}

// ---------- End-to-end hot path ----------

/// End-to-end hot path: guard create + a few field sets + a log
/// message + drop. This is the most representative single-event
/// workload for wide-log, and captures the cumulative effect of
/// the reusable-buffer optimizations above.
type CaptureSlot = Arc<Mutex<Option<String>>>;

fn capture_emit() -> (
    CaptureSlot,
    impl FnOnce(&wide_log::WideEvent<EventKey>) + Send + 'static,
) {
    let slot: CaptureSlot = Arc::new(Mutex::new(None));
    let s = slot.clone();
    let emit = move |we: &wide_log::WideEvent<EventKey>| {
        if let Ok(json) = we.to_json() {
            *s.lock().unwrap() = Some(json);
        }
    };
    (slot, emit)
}

fn bench_end_to_end(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase3_end_to_end");

    // Full lifecycle with default id (ULID) and capture emit
    // (serialize to JSON). This is the "user-facing" cost of a
    // single event.
    group.bench_function("hot_path_capture_emit", |b| {
        b.iter_batched(
            capture_emit,
            |(_slot, emit)| {
                let guard = WideLogGuard::builder().with_emit(emit).build();
                wl_set!("service.name", "my-service");
                wl_set!("request.method", "GET");
                wl_set!("request.path", "/api/users/42");
                wl_set!("request.status", 200u64);
                wl_inc!("requests");
                info!("user {} logged in", 42);
                drop(black_box(guard));
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Same, with `with_id_str` for a known correlation id.
    group.bench_function("hot_path_with_id_str", |b| {
        b.iter_batched(
            capture_emit,
            |(_slot, emit)| {
                let guard = WideLogGuard::builder()
                    .with_id_str("req-12345")
                    .with_emit(emit)
                    .build();
                wl_set!("service.name", "my-service");
                wl_set!("request.method", "GET");
                wl_set!("request.path", "/api/users/42");
                wl_set!("request.status", 200u64);
                wl_inc!("requests");
                info!("user {} logged in", 42);
                drop(black_box(guard));
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

// ---------- log-message path cost ----------

/// Cost of appending one log entry through the tracing dispatch
/// path (capture layer + re-exported macros) versus the direct
/// macro path (generated macros, feature off). Under the `tracing`
/// feature the capture path runs; without it the direct macro path
/// runs, so the group name reflects the mode being measured.
#[cfg(feature = "tracing")]
fn bench_capture_vs_macro(c: &mut Criterion) {
    init_capture();
    let mut group = c.benchmark_group("log_message_path");
    group.bench_function("capture_layer", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::builder().with_emit(noop_emit).build();
            ::tracing::info!("capture path entry {}", 1);
            drop(black_box(_guard));
        })
    });
    group.finish();
}

#[cfg(not(feature = "tracing"))]
fn bench_capture_vs_macro(c: &mut Criterion) {
    let mut group = c.benchmark_group("log_message_path");
    group.bench_function("direct_macro", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::builder().with_emit(noop_emit).build();
            info!("capture path entry {}", 1);
            drop(black_box(_guard));
        })
    });
    group.finish();
}

criterion_group!(
    phase3_benches,
    bench_fmt_buf,
    bench_ulid_buf,
    bench_with_id_str,
    bench_end_to_end,
    bench_capture_vs_macro,
);
criterion_main!(phase3_benches);
