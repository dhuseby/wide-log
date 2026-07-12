use std::hint::black_box;
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use wide_log::wide_log;

// Generate the wide-log schema once at module level. This produces:
//   EventKey enum, Key impl, thread_local CURRENT_EVENT, WideLogGuard,
//   current(), all wl_* and log-level macros.
wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "http": {
        "method": null,
        "path": null,
        "status": null,
    },
    "requests": counter!,
    "retries": counter!,
    "status": null,
    "flag": null,
});

// ---------- Helpers ----------

/// No-op emit closure — isolates accumulation cost from serialization/IO.
fn noop_emit(_ev: &wide_log::WideEvent<EventKey>) {}

/// Capture-slot emit closure — serializes the event to JSON on drop.
/// Measures the full emit path including `to_json`.
type CaptureSlot = std::sync::Arc<std::sync::Mutex<Option<String>>>;

fn capture_emit() -> (
    CaptureSlot,
    impl FnOnce(&wide_log::WideEvent<EventKey>) + Send + 'static,
) {
    let slot: CaptureSlot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let s = slot.clone();
    let emit = move |we: &wide_log::WideEvent<EventKey>| {
        if let Ok(json) = we.to_json() {
            *s.lock().unwrap() = Some(json);
        }
    };
    (slot, emit)
}

// ---------- Benchmarks ----------

/// Benchmark: guard create + drop with no operations in between.
/// Measures the baseline overhead of the guard lifecycle (thread_local
/// set/restore, SmallVec allocation, duration computation, emit call).
///
/// The guard MUST be created and dropped inside the measurement routine
/// (not in iter_batched's setup) because the guard sets a thread_local
/// pointer on creation and restores it on drop. Creating multiple guards
/// in the setup phase would clobber each other's pointers.
fn bench_guard_create_drop(c: &mut Criterion) {
    let mut group = c.benchmark_group("guard_create_drop");

    // With no-op emit: isolates guard overhead.
    group.bench_function("noop_emit", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            drop(black_box(_guard));
        })
    });

    // With capture emit (includes serialization to JSON on drop):
    group.bench_function("capture_emit", |b| {
        b.iter_batched(
            capture_emit,
            |(_slot, emit)| {
                let _guard = WideLogGuard::new_with_emit(emit);
                drop(black_box(_guard));
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

/// Benchmark: wl_set! on a single-segment path (top-level key).
/// Measures current() + add_path (single segment) cost.
///
/// The guard is created inside the routine. We measure just the wl_set!
/// call (plus guard create/drop overhead, which is constant across
/// comparisons).
fn bench_wl_set_single(c: &mut Criterion) {
    let mut group = c.benchmark_group("wl_set_single");

    // String value (most common case):
    group.bench_function("string", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            wl_set!("status", "ok");
            drop(black_box(_guard));
        })
    });

    // U64 value:
    group.bench_function("u64", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            wl_set!("status", 200u64);
            drop(black_box(_guard));
        })
    });

    // Bool value:
    group.bench_function("bool", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            wl_set!("flag", true);
            drop(black_box(_guard));
        })
    });

    group.finish();
}

/// Benchmark: wl_set! on a multi-segment (nested) path.
/// Measures current() + add_path (descend + add) cost.
fn bench_wl_set_nested(c: &mut Criterion) {
    let mut group = c.benchmark_group("wl_set_nested");

    // Two-segment path: "service.name"
    group.bench_function("two_segment", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            wl_set!("service.name", "my-service");
            drop(black_box(_guard));
        })
    });

    // Two-segment path in a different subtree: "http.method"
    group.bench_function("two_segment_http", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            wl_set!("http.method", "GET");
            drop(black_box(_guard));
        })
    });

    // Repeated sets to the same nested key (update path):
    group.bench_function("update_existing_nested", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            wl_set!("service.name", "a");
            wl_set!("service.name", "b");
            wl_set!("service.name", "c");
            drop(black_box(_guard));
        })
    });

    group.finish();
}

/// Benchmark: wl_inc! and wl_dec! on counter fields.
fn bench_wl_inc_dec(c: &mut Criterion) {
    let mut group = c.benchmark_group("wl_inc_dec");

    // Single inc on absent counter:
    group.bench_function("inc_absent", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            wl_inc!("requests");
            drop(black_box(_guard));
        })
    });

    // Inc on existing counter (initial set + inc):
    group.bench_function("inc_existing", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            wl_inc!("requests");
            wl_inc!("requests");
            wl_inc!("requests");
            drop(black_box(_guard));
        })
    });

    // Dec:
    group.bench_function("dec_absent", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            wl_dec!("retries");
            drop(black_box(_guard));
        })
    });

    // wl_add!:
    group.bench_function("add_n", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            wl_add!("requests", 10);
            wl_add!("requests", -3);
            drop(black_box(_guard));
        })
    });

    group.finish();
}

/// Benchmark: info! / warn! / etc. log entry accumulation.
fn bench_log_macros(c: &mut Criterion) {
    let mut group = c.benchmark_group("log_macros");

    // Literal message (no formatting):
    group.bench_function("info_literal", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            info!("request received");
            drop(black_box(_guard));
        })
    });

    // Formatted message (format! + FastStr allocation):
    group.bench_function("info_format", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            info!("request {} received in {}ms", 42, 15);
            drop(black_box(_guard));
        })
    });

    // Multiple log entries (amortized push cost):
    group.bench_function("info_multiple", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            info!("request received");
            info!("processing started");
            warn!("upstream slow");
            info!("request completed");
            drop(black_box(_guard));
        })
    });

    group.finish();
}

/// Benchmark: to_json serialization in isolation.
/// Builds an event directly (no guard) and serializes it.
/// No thread_local issues here since we bypass the guard entirely.
fn bench_to_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("to_json");

    // Small event (2 fields):
    group.bench_function("small", |b| {
        b.iter_batched(
            || {
                let mut ev = wide_log::WideEvent::<EventKey>::new();
                ev.add(EventKey::Status, "ok");
                ev.add(EventKey::Requests, 1u64);
                ev
            },
            |ev| {
                let json = ev.to_json().unwrap();
                black_box(json);
            },
            BatchSize::SmallInput,
        )
    });

    // Medium event (nested + multiple fields + log entries):
    group.bench_function("medium", |b| {
        b.iter_batched(
            || {
                let mut ev = wide_log::WideEvent::<EventKey>::new();
                ev.add_path(&[EventKey::Service, EventKey::Name], "my-service");
                ev.add_path(&[EventKey::Service, EventKey::Version], "1.0.0");
                ev.add_path(&[EventKey::Http, EventKey::Method], "GET");
                ev.add_path(&[EventKey::Http, EventKey::Path], "/api/users");
                ev.add_path(&[EventKey::Http, EventKey::Status], 200u64);
                ev.add(EventKey::Requests, 42u64);
                ev.add(EventKey::Status, "ok");
                ev.append_log_entry("info", "request received");
                ev.append_log_entry("warn", "upstream slow");
                ev.append_log_entry("info", "request completed");
                ev
            },
            |ev| {
                let json = ev.to_json().unwrap();
                black_box(json);
            },
            BatchSize::SmallInput,
        )
    });

    // Large event (many fields + many log entries):
    group.bench_function("large", |b| {
        b.iter_batched(
            || {
                let mut ev = wide_log::WideEvent::<EventKey>::new();
                ev.add_path(&[EventKey::Service, EventKey::Name], "my-service");
                ev.add_path(&[EventKey::Service, EventKey::Version], "1.0.0");
                ev.add_path(&[EventKey::Http, EventKey::Method], "GET");
                ev.add_path(&[EventKey::Http, EventKey::Path], "/api/users/42/details");
                ev.add_path(&[EventKey::Http, EventKey::Status], 200u64);
                ev.add(EventKey::Requests, 1337u64);
                ev.add(EventKey::Retries, 3u64);
                ev.add(EventKey::Status, "ok");
                ev.add(EventKey::Flag, true);
                for i in 0..20 {
                    ev.append_log_entry("info", &format!("log entry number {i}"));
                }
                ev
            },
            |ev| {
                let json = ev.to_json().unwrap();
                black_box(json);
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

/// Benchmark: full request lifecycle — guard creation, field sets,
/// counter increments, log messages, drop + serialize.
/// This is the end-to-end benchmark that represents typical usage.
fn bench_full_lifecycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_lifecycle");

    // No-op emit (accumulation only, no serialization):
    group.bench_function("noop_emit", |b| {
        b.iter(|| {
            let guard = WideLogGuard::new_with_emit(noop_emit);
            wl_set!("service.name", "my-service");
            wl_set!("http.method", "GET");
            wl_set!("http.path", "/api/users");
            wl_set!("http.status", 200u64);
            wl_inc!("requests");
            wl_inc!("requests");
            info!("request received");
            warn!("upstream slow");
            info!("request completed");
            drop(black_box(guard));
        })
    });

    // With capture emit (full path including serialization):
    group.bench_function("capture_emit", |b| {
        b.iter_batched(
            capture_emit,
            |(_slot, emit)| {
                let guard = WideLogGuard::new_with_emit(emit);
                wl_set!("service.name", "my-service");
                wl_set!("http.method", "GET");
                wl_set!("http.path", "/api/users");
                wl_set!("http.status", 200u64);
                wl_inc!("requests");
                wl_inc!("requests");
                info!("request received");
                warn!("upstream slow");
                info!("request completed");
                drop(black_box(guard));
            },
            BatchSize::SmallInput,
        )
    });

    // With throughput measurement (bytes per second for the capture_emit path):
    group.throughput(Throughput::Elements(1));
    group.bench_function("capture_emit_throughput", |b| {
        b.iter_batched(
            capture_emit,
            |(slot, emit)| {
                let guard = WideLogGuard::new_with_emit(emit);
                wl_set!("service.name", "my-service");
                wl_set!("http.method", "GET");
                wl_set!("http.path", "/api/users");
                wl_set!("http.status", 200u64);
                wl_inc!("requests");
                wl_inc!("requests");
                info!("request received");
                warn!("upstream slow");
                info!("request completed");
                drop(black_box(guard));
                let json_len = slot.lock().unwrap().as_ref().map(|s| s.len()).unwrap_or(0);
                black_box(json_len);
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

/// Benchmark: current() access overhead — the TLS lookup that every
/// wl_* and log macro calls. Measures the cost of the thread_local
/// closure indirection + pointer dereference.
fn bench_current_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("current_access");

    group.bench_function("with_guard", |b| {
        b.iter(|| {
            let _guard = WideLogGuard::new_with_emit(noop_emit);
            for _ in 0..100 {
                let _ev = current();
                black_box(_ev);
            }
            drop(black_box(_guard));
        })
    });

    group.bench_function("without_guard", |b| {
        b.iter(|| {
            let _ev = current();
            black_box(_ev);
        })
    });

    group.finish();
}

/// Benchmark: repeated wl_set! to the same key (update vs insert).
/// Measures the linear scan cost as entries grow.
fn bench_wl_set_repeat(c: &mut Criterion) {
    let mut group = c.benchmark_group("wl_set_repeat");

    for n in [1, 5, 10, 20] {
        group.bench_with_input(BenchmarkId::new("distinct_keys", n), &n, |b, &n| {
            b.iter(|| {
                let _guard = WideLogGuard::new_with_emit(noop_emit);
                for i in 0..n {
                    match i % 6 {
                        0 => wl_set!("status", "ok"),
                        1 => wl_set!("flag", true),
                        2 => wl_set!("service.name", "svc"),
                        3 => wl_set!("http.method", "GET"),
                        4 => wl_set!("http.path", "/api"),
                        _ => wl_set!("http.status", 200u64),
                    }
                }
                drop(black_box(_guard));
            })
        });
    }

    for n in [1, 5, 10, 20] {
        group.bench_with_input(BenchmarkId::new("same_key_update", n), &n, |b, &n| {
            b.iter(|| {
                let _guard = WideLogGuard::new_with_emit(noop_emit);
                for _ in 0..n {
                    wl_set!("status", "ok");
                }
                drop(black_box(_guard));
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_guard_create_drop,
    bench_wl_set_single,
    bench_wl_set_nested,
    bench_wl_inc_dec,
    bench_log_macros,
    bench_to_json,
    bench_full_lifecycle,
    bench_current_access,
    bench_wl_set_repeat,
);
criterion_main!(benches);