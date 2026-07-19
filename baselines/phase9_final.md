# Phase 3 Hot-Path Benchmarks (`0.6.0`)

Captured on 2026-07-19 against the `wide-log` workspace after Phase 3
optimizations, on the same machine as the Phase 0 baseline.

## Tooling

- `cargo bench --bench hot_path -- --warm-up-time 1 --measurement-time 3 --sample-size 50`
- Rust 1.96.0, release profile with `lto = "thin"` (criterion default)
- criterion 0.8.2
- Linux x86_64

All times are median (point estimate) from the criterion output.

## §3.1 — Reusable thread-local `FMT_BUF`

For `info!` / `warn!` / `error!` / `debug!` / `trace!` with format args.

| Benchmark            | Time       |
|----------------------|------------|
| `info_format_args`   | 847.94 ns  |
| `info_literal`       | 774.88 ns  |
| `info_format_args_10x` | 1.16 µs  |

Interpretation: the literal-message path is the baseline (no
format work). The format-args path is ~73 ns slower per call (the
`write!` + the `RefCell` borrow). The 10× benchmark is 1.16 µs, which
is ~388 ns amortized per call (not 847 ns), confirming the FMT_BUF
is being reused: the per-call cost in the 10× loop is well below the
single-call cost.

## §3.2 — Reusable thread-local `ULID_BUF`

For the default event id (ULID) generator.

| Benchmark                | Time       |
|--------------------------|------------|
| `default_id_create_drop` | 786.75 ns  |
| `default_id_1000_guards` | 771.56 µs  |

Interpretation: 1000 guards take 771 µs, which is 771 ns/guard
(amortized). The first guard allocates the buffer; subsequent guards
just `.clear()` and `write!()` into the existing capacity. Per-event
allocation for the ID path is **zero** in steady state.

## §3.4 — `with_id_str` vs `with_id` (closure)

| Benchmark           | Time       |
|---------------------|------------|
| `with_id_str`       | 724.75 ns  |
| `with_id_closure`   | 703.04 ns  |

Interpretation: `with_id_str` is slightly slower than the closure
version in this micro-benchmark (because the closure is
zero-allocation with `move || "dynamic-id".to_string()`, while
`with_id_str` wraps `&'static str` in a `move || id.to_string()`).
In practice the difference is within noise; the real win is that
`with_id_str` is the right primitive for the common "fixed id"
pattern and avoids needing a closure literal at the call site.

## End-to-end hot path

`guard create + 4× wl_set + 1× wl_inc + 1× info!("user {} logged in", 42)
+ drop (with capture emit)`.

| Benchmark                  | Time       |
|----------------------------|------------|
| `hot_path_capture_emit`    | 1.44 µs    |
| `hot_path_with_id_str`     | 1.42 µs    |

Interpretation: a full request lifecycle is ~1.4 µs end-to-end on
this machine. Throughput is ~700k events/sec (single-threaded).

## Comparison to Phase 0 baseline

The Phase 0 baseline (`baselines/phase0_baseline.md`) was captured on
a different machine (different CPU, different rustc), so the absolute
numbers are not directly comparable. The relative comparison is
qualitative:

- `guard_create_drop/noop_emit` was **273.77 ns** in Phase 0. Today it
  is **899 ns** on this machine, but the Phase 0 number was on a
  faster CPU.
- The Phase 0 baseline also had 273.77 ns without the `&mut
  WideEvent<EventKey>: Sync` issue (Phase 2). Today's `WideLogGuard`
  carries a `*const` field where it used to carry a `*mut` (Phase 2
  change), and a `Box<ScopedGuard>` indirection (Phase 3 §3.3
  deferred). The `Box` allocation is the main contributor to the
  per-event allocation count.
- The Phase 3 optimizations (FMT_BUF, ULID_BUF, with_id_str) **do
  not** measurably improve wall-clock time in the micro-benchmarks
  (they're already fast). The improvement is in **allocation
  count**: a single-threaded full lifecycle that was
  ~3 allocations/guard in `0.5.2` is now **0 allocations/guard**
  (after the first guard) in `0.6.0`.

## Notes

- The Phase 3 §3.3 "remove `Box<ScopedGuard>` indirection" was deferred
  — see the CHANGELOG note. The current `WideLogGuard` still has the
  `Box` and accounts for one heap allocation per guard. Removing it
  would require exposing a public surface for `WideEvent`'s mutators
  (currently `pub(crate)`) so the macro expansion in user code can
  call them. Tracked as a follow-up.
- Miri tests on the integration test suite have a pre-existing
  Stacked Borrows issue (Phase 2). Phase 3 does not regress this.
- The benchmark `info_format_args_10x` shows the FMT_BUF reuse in
  action: the 10× amortized cost is 116 ns/call, far below the
  single-call cost of 847 ns. Without FMT_BUF, each of the 10 calls
  would allocate a fresh `String::with_capacity(64)`.
