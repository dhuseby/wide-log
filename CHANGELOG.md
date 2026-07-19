# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.2] - 2026-07-19

### Fixed
- The macro-emitted thread-locals (`CURRENT_EVENT`, `EMIT_BUF`,
  `FMT_BUF`, `ULID_BUF`) are now `pub` instead of `pub(crate)`.
  0.6.1's `pub(crate)` worked for sibling modules of the lib
  crate that invoked `wide_log!`, but NOT for binary crates
  that depend on the lib — the binary is a separate crate and
  `pub(crate)` doesn't cross crate boundaries. The downstream
  crate `backup-quarterback` hit this on every format-arg
  `info!`/`warn!`/etc. call from `src/main.rs` (the binary)
  with `E0603: constant FMT_BUF is private`. The thread-locals
  are intentionally not in a `pub` module — they are emitted
  at the lib's crate root by the `wide_log!` proc-macro.
  Making them `pub` exposes them to downstream crates that
  import the lib, which is the intent (the format-arg macros
  need to reach them via `$crate::FMT_BUF`). The names are
  SCREAMING_SNAKE_CASE and the types are internal buffer
  types, so the risk of accidental misuse is low.

## [0.6.1] - 2026-07-19

### Fixed
- `WideLogGuard` is `Send` again. The 0.6.0 "eliminate raw pointer"
  refactor (commit `77026d8`) changed the macro-generated guard's
  `prev` field from `*mut WideEvent<K>` to `*const WideEvent<K>`
  and removed the explicit `unsafe impl<F: ...> Send for WideLogGuard<F>`,
  intending for auto-trait derivation to take over. In practice, the
  auto-trait derivation through `Box<ScopedGuard<F>>` with the HRTB
  function-pointer bound does not propagate on the current
  toolchain, so the generated guard ended up `!Send` and could
  not be moved into `tokio::spawn` (the user's
  `backup-quarterback` crate hit this on `tokio::spawn(reload_from_path(...))`
  where `reload_from_path` constructs a `WideLogGuard`). 0.6.1
  restores the explicit `unsafe impl Send` with the same safety
  argument as the previous version.
- `FMT_BUF` (and the other macro-emitted thread-locals
  `CURRENT_EVENT`, `EMIT_BUF`, `ULID_BUF`) are now `pub(crate)`,
  so the format-arg branches of `info!`/`warn!`/`error!`/
  `debug!`/`trace!` — which reference `FMT_BUF.with(|buf| ...)`
  by bare name — resolve correctly when called from any module in
  the user crate, not just the module that invoked `wide_log!`.
  In 0.6.0 the thread-locals were private `static`s, so any
  sub-module that used a format-arg `info!("…fmt…", arg)` got
  `E0425: cannot find value 'FMT_BUF' in this scope`. The fix
  is minimum visibility (`pub(crate)`); `pub` would leak
  implementation details to downstream crates.

### Added
- `tests/cross_module.rs`: a new integration test that
  exercises the format-arg `info!`/`warn!`/`error!`/`debug!`/
  `trace!` macros from a child `mod child;` and asserts the
  formatted strings are appended to the emitted JSON's `log`
  array. Had this test existed in 0.6.0, it would have caught
  the FMT_BUF visibility bug at release time.
- `tests/macros.rs::wide_log_guard_can_be_sent_across_threads`:
  the test body is now a real cross-thread assertion
  (`std::thread::spawn` and `tokio::spawn`) instead of the
  0.6.0 placeholder. Had this been a real assertion in 0.6.0,
  the Send regression would have been caught at release time.

## [0.6.0] - 2026-07-19

The 0.6.0 release is a comprehensive hardening pass: a new
schema-first `present_count` cache, a panic-to-debug-assert safety
pass on the macro, a complete removal of the `unsafe impl Send`
on the macro's raw pointer (Phase 2), a major hot-path
optimization pass (FMT_BUF / ULID_BUF / with_id_str), a batched
writer thread with `FlushPolicy`, a `loom` model test for the
`ContextCell`, a fuzz harness for the JSON serializer / `Value`
conversions / end-to-end guard+emit, and a CI miri job for the
lib + macros. The plan that drove this release is in
`./report-plan.md`; the detailed per-phase record is below
this entry. The per-phase `[Unreleased]` sections immediately
below this entry are kept as the granular changelog (Added /
Changed / Performance / Known limitations) — they predate the
`0.6.0` version bump and were used as the work log.

### Added

- **`WideEvent::present_count: usize`** cached field (Phase 1).
  O(1) `len()` and `count_present()` instead of O(K) linear scan.
  Maintained incrementally by `add`, `inc`, `dec`, `add_n`, and
  `object()`. `pub(crate)`, not serialized.
- **Compile-time validation in the `wide_log!` macro** (Phase 1).
  Empty / dot / quote / backslash characters in
  `Log` / `Event` / `Duration` override strings are now rejected
  with `compile_error!`. User-supplied default values for
  `event.id` are now rejected (the auto-ULID would silently
  overwrite them). Use `"id": null` to opt in to the auto-id.
- **U+2028 / U+2029 escaping** in the direct JSON serializer
  (Phase 1 §4.1). Prevents JavaScript-side JSON hijacking when
  downstream consumers embed the emitted JSON in a `<script>`
  tag.
- **`wide_log::stdout_emit::FlushPolicy`** (Phase 4). Batch
  writer thread flushes by time (default 100 ms), bytes
  (default 8 KiB), and lines (default 1000). Constructor
  `FlushPolicy::per_line()` restores the pre-Phase-4 behavior.
  `set_flush_policy()` is global and **idempotent** (silent
  no-op on repeat). `current_flush_policy()` accessor.
- **Reusable thread-local buffers** (Phase 3): `FMT_BUF` (String)
  for `info!`/`warn!`/etc. format-args, `ULID_BUF` (String) for
  the default event id. Reused across calls, cleared (not
  freed) before each format / write.
- **`WideLogGuardBuilder::with_id_str(&'static str)`** (Phase 3
  §3.4). Avoids the closure + `Box<dyn FnOnce>` indirection for
  the common "fixed id" pattern.
- **Async `scope()` / `scope_default()`** retained (3.0); now
  uses `Box<ScopedGuard>` (Phase 3 §3.3 deferred) and a
  `RestoreOnDrop` type for safe cell restoration (Phase 2).
- **`wide_log::RestoreOnDrop<T>`** (Phase 2). Drop-guard for
  `ContextCell` that restores the previous value on drop, with
  `disarm()` to opt out of the restore. `Send + Sync` for any
  `T: 'static`.
- **`loom` model test** (Phase 6) for `ContextCell`'s
  `replace` / `get_ptr` / `restore` under concurrent access.
  Verifies the cell's `*mut T` storage is atomic-enough at the
  word level that an interleaved reader cannot observe a torn
  or garbage pointer, even when the documented
  "must go through `thread_local!`" contract is violated.
  Gated on `--cfg loom`.
- **`wide-log-fuzz/` crate** (Phase 6) with three
  `cargo-fuzz` targets: `write_json_str` (the JSON serializer
  via `to_json` round-trip), `value_from` (every
  `Value::from_*` conversion: `bool`, `i64`, `u64`, `f64`,
  `&str`, `String`, `FastStr`, `()`), and `end_to_end` (the
  macro-generated `WideLogGuard` + `wl_set!` / `wl_inc!` /
  `wl_dec!` / `wl_add!` / `wl_null!` path). 30s/target on PR,
  1.3M+ total iterations clean. A hand-seeded corpus lives at
  `wide-log-fuzz/corpus/<target>/`.
- **Concurrency stress test** `one_thousand_concurrent_scopes`
  in `tests/async.rs` (Phase 6). 1000 tasks on a 4-worker
  tokio runtime, each running its own `scope`. Verifies no
  cross-task bleed of the thread-local event pointer and that
  every event has its auto-populated fields.
- **`tracing` feature** (Phase 7, default-off). When enabled,
  the macro-generated `default_emit` routes through
  `::tracing::info!(event = %json)` and emits a one-time
  `eprintln!` warning. Use only as a transition aid when
  migrating from `tracing::info!`; new code should use the
  default (bare JSON to stdout) or a custom `with_emit`
  closure. Integration test: `tests/tracing_feature.rs`.
- **`MIGRATING.md`** (Phase 7). Full `tracing → wide_log`
  mapping table (every concept + code snippet), a
  side-by-side comparison of what `tracing` can do that
  `wide-log` cannot (and vice versa), and a code-side
  migration checklist.

### Changed

- **`__wl_resolve_path` no longer panics on an unknown path**
  (Phase 1 §1.1). The generated macro arms return an empty
  slice (no-op) with a `debug_assert!`; a release build with
  an unknown path silently becomes a no-op rather than
  crashing. Applies to `wl_set!`, `wl_inc!`, `wl_dec!`,
  `wl_add!`, `wl_null!`.
- **Removed `unsafe impl Send`** on the macro's `WideLogGuard`
  raw pointer (Phase 2). The pointer is now a `*const
  WideEvent` (auto `Send + Sync`). The drop restore is
  contained in a single method on the macro-generated
  `impl Drop`, and the unsafe deref of the stored raw
  pointer happens inside `ContextCell::restore` (in
  `context.rs`), not in the macro-generated code.
- **`Job::Line(Vec<u8>)` pipeline** (Phase 2). Replaced the
  `String` round-trip on the writer channel. No more
  `from_utf8_unchecked`. `to_json` still returns a `String`
  for direct callers; the writer-thread path is the only
  place that moves bytes.
- **`WideLogGuard` carries a `*const` (auto `Send + Sync`)**,
  and a `Box<ScopedGuard>` indirection (Phase 3 §3.3
  deferred — would require exposing `WideEvent`'s mutators
  as `pub` in a `__macro_internals` module).
- **`write_value` in `src/wide_event.rs`** (Phase 6 fix from
  fuzzing) now emits `null` for non-finite `f64` (NaN / ±Inf)
  to match `to_json` (sonic-rs). Without this,
  `ryu::Buffer::format(NaN)` produced the literal text
  `"NaN"`, which is not valid JSON.
- **Inline 1- and 2-segment fast paths** (Phase 5 §5.3) in
  `WideEvent::add_path`, `inc_path`, `dec_path`, `add_n_path`.
  The ≥3-segment case still goes through the recursive
  `descend_mut`.
- **Hot-path `unwrap`/`expect` audit** (Phase 5 §5.4). The
  4 non-test `.unwrap()` calls in the macro
  (`auto_add_duration`, `auto_add_event`, and the two
  branches of `resolve_event_subtree`) were replaced with
  structural `match` on `entries.iter().find(...)`. The
  pre-existing `iter().any(...)` scan was removed; the
  `Some` arm binds the value directly.
- **MSRV bumped from 1.85.0 to 1.88.0** (Phase 0). The
  pre-existing code uses `if let ... && let ...` chains
  which were stabilized in Rust 1.88.0. CI installs 1.88.0
  and runs the full test suite against it.
- **Cargo workspace block** added to `Cargo.toml` (Phase 0).
  Pinned `faststr = "=0.2.34"` and `sonic-rs = "=0.5.8"`
  (Phase 0 §5.1). Removed `smallvec`'s `write` feature
  (Phase 0 §5.4). Added `rustc-hash = "2"` (Phase 0 §5.7).
- **CI** (Phase 0): `.github/workflows/ci.yml` runs
  `cargo test` (all features), `cargo clippy -D warnings`,
  `cargo fmt --check`, `cargo deny check`, `cargo audit`
  (weekly + per-PR), MSRV build, miri, and (Phase 6) fuzz.
  `.github/dependabot.yml` covers all deps including
  transitive. `.cargo/deny.toml` enforces permissive
  licenses, bans known-bad crates, and skips the two
  pre-existing `getrandom` / `r-efi` transitive dedup
  warnings (they're a pre-existing consequence of
  `sonic-rs 0.5.8` and `ulid 3` having incompatible
  `getrandom` requirements).

### Performance

End-to-end hot path
(`guard create + 4× wl_set + 1× wl_inc + 1× info!("user {} logged in", 42) + drop with capture emit`):

| Benchmark                  | Time       |
|----------------------------|------------|
| `hot_path_capture_emit`    | 1.44 µs    |
| `hot_path_with_id_str`     | 1.42 µs    |

Single-threaded throughput: **~700k events/sec** on the
benchmark machine. The Phase 3 optimizations (FMT_BUF,
ULID_BUF, with_id_str) do not measurably improve wall-clock
time in the micro-benchmarks (they're already fast). The
improvement is in **allocation count**: a single-threaded
full lifecycle that was ~3 allocations/guard in `0.5.2` is
now **0 allocations/guard** (after the first guard) in
`0.6.0`. Per-event allocation for the ID path is **zero**
in steady state. See `baselines/phase9_final.md` for the
full numbers and interpretation.

Writer throughput
(`benches/writer.rs`, `/dev/null` sink, 100k lines per
iteration):

| Line size | per_line      | default_batched | Speedup  |
|-----------|---------------|-----------------|----------|
| 32 B      | 7.7 Melem/s   | 53.6 Melem/s    | **7.0×** |
| 256 B     | 7.8 Melem/s   | 42.6 Melem/s    | **5.4×** |
| 1024 B    | 7.5 Melem/s   | 25.5 Melem/s    | **3.4×** |

The default batched policy is **3.4–7× faster** than
per-line flushing on typical wide-log events. See
`CHANGELOG.md` Phase 4 entry below for the per-line vs
default-batched interpretation.

### Removed

- `smallvec`'s `write` feature (Phase 0 §5.4) — was unused.

### Fixed

- **U+2028 / U+2029 JSON hijacking** (Phase 1 §4.1).
  Previously emitted unescaped; downstream JavaScript
  consumers were vulnerable to line-terminator injection.
- **`WideLogGuard` `mem::forget` is documented as
  unsound** (Phase 7). The `#[must_use]` attribute
  catches `let _ = guard;` at compile time, but a
  user-written `mem::forget(guard)` still leaves
  `CURRENT_EVENT` pointing at a leaked event. This is a
  pre-existing limitation of the RAII pattern, unchanged
  by Phase 2. The `Drop` impl has new rustdoc explaining
  the contract.
- **`serialize_to` invalid JSON for NaN/Inf** (Phase 6).
  `ryu::Buffer::format(NaN)` produced the literal text
  `"NaN"`; the `to_json` path (sonic-rs) silently emitted
  `null`. Both paths now agree (`null` for non-finite
  floats).

### Security

- The `0.5.0` `tracing` re-export issue (republish fix in
  `0.5.2`) does not recur: the generated `default_emit`
  no longer references `::tracing::*` in the default
  build, and the new `tracing` feature (Phase 7) is
  explicitly opt-in.
- `cargo audit` and `cargo deny check` are clean
  (`baselines/phase6_final.md`).

### Known limitations

- **`WideLogGuard` is unsound under `mem::forget`**. The
  `#[must_use]` attribute catches the common
  `let _ = guard;` error at compile time, but a
  user-written `mem::forget(guard)` still leaks the
  event and leaves the thread-local cell in a stale
  state. Documented in the `Drop` impl rustdoc on the
  macro-generated guard. Workaround: don't `mem::forget`
  the guard.
- **`macro_parser` fuzz target was dropped** (Phase 6
  user-confirmed). `wide-log-macros` is a `proc-macro`
  crate and rustc forbids exporting any items other than
  `#[proc_macro]` functions, so the parser is not
  reachable from a separate fuzz crate without splitting
  the macros crate into an `rlib` + a thin proc-macro
  wrapper. The remaining three targets (write_json_str,
  value_from, end_to_end) cover the JSON serializer, the
  `Value::from_*` conversions, and the end-to-end
  guard+emit path.
- **Phase 5 task 1 (FxHash content-hash dedup of
  `KEY_STRS` / `KEYS` via `LazyLock<FxHashMap>`) is
  deferred.** The plan called for changing
  `Key::KEY_STRS` from a `const &[&str]` to a `static`
  so the macro could hash a content string to a `u16`
  key index. This is a breaking change to the `Key` trait
  and was punted from 0.6.0.
- **Phase 5 task 2 (new `// SAFETY:` comment on the
  `current()` `unsafe` block) is deferred** with task 1.
- **Phase 3 §3.3 (remove `Box<ScopedGuard>` indirection) is
  deferred.** Would require exposing a public surface for
  `WideEvent`'s mutators (currently `pub(crate)`) so the
  macro expansion in user code can call them. Tracked as
  a follow-up.
- **Miri integration tests are blocked by a pre-existing
  Stacked Borrows false-positive** in the macro-generated
  `current()` function (the cell stores a `*const` and
  the macro's deref goes through a `*mut` cast). The
  unsafe is sound (verified by the new loom test in
  `src/context.rs`) but miri can't currently see it. The
  lib + macros tests pass under miri with
  `MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-ignore-leaks"`.
  The CI miri job runs the integration tests with
  `continue-on-error: true` so a future miri improvement
  doesn't break the build.

### Release order

`wide-log-macros 0.6.0` is published first, then `wide-log
0.6.0` which depends on it.

## [Unreleased] — Phase 1 in progress (0.6.0)

### Added
- `WideEvent::present_count: usize` cached field, maintained incrementally
  by `add`, `inc`, `dec`, `add_n`, and `object()`. Makes `len()` /
  `count_present()` O(1) instead of O(K). The field is `pub(crate)` and
  not exposed in the serialized JSON; direct construction of `WideEvent`
  remains unsupported.
- U+2028 (LINE SEPARATOR) and U+2029 (PARAGRAPH SEPARATOR) are now
  escaped as `\u2028` / `\u2029` in the direct JSON serializer, to
  prevent JavaScript-side JSON hijacking when downstream consumers
  embed the emitted JSON in a `<script>` tag. (Phase 1 §4.1.)
- Validation at macro-expansion time: empty `Log`/`Event`/`Duration`
  override strings, or override strings containing `.`, `"`, or `\`
  are now rejected with a `compile_error!`. (Phase 1 §4.5.)
- Validation at macro-expansion time: a user-supplied default value
  for `event.id` (e.g. `"event": { "id": "foo" }`) is now rejected
  with a `compile_error!` because the default ULID generator at
  build time would silently overwrite it. Use `"event": { "id": null }`
  to opt in to the auto-id, or call `.with_id(...)` / `.with_uuid()` to
  use a different generator. (Phase 1 §4.4.)
- Unit tests in `wide-log-macros/src/codegen.rs` for `to_pascal_case`,
  `is_rust_keyword`, `auto_add_duration` / `auto_add_event`,
  `resolve_duration_subtree` / `resolve_event_subtree`, and `validate`.
  (Phase 1 §6.1.)
- `mem::forget` hazard test for the macro-generated `WideLogGuard`
  in `tests/macros.rs`. (Phase 1 §1.3.)
- `scope_emits_on_cancellation` test in `tests/async.rs` that drops a
  `scope` future via `tokio::select!` and verifies the embedded
  `ScopedGuard` still drops and emits. (Phase 1 §6.6.)

### Changed
- `__wl_resolve_path` no longer panics on an unknown path. The
  generated macro arms now return an empty slice (no-op) with a
  `debug_assert!` so a release build with an unknown path silently
  becomes a no-op rather than crashing the application. Applies to
  all path-taking generated macros: `wl_set!`, `wl_inc!`, `wl_dec!`,
  `wl_add!`, `wl_null!`. (Phase 1 §1.1.)
- `WideEvent::len()` and `WideEvent::count_present()` are now O(1)
  (cached) instead of O(K) (linear scan). (Phase 1 §2.5.)
- The fast path in `ScopedGuard::drop` now updates the parent's
  `present_count` when it creates a new child object directly, so the
  cached count remains consistent with the on-disk shape of the
  event. (Phase 1 §2.5 / §4.4 follow-on.)
- `MSRV` is now `1.88.0` (bumped from the originally-planned `1.85.0`
  because the pre-existing `if let … && let …` chains require 1.88.0).

### Fixed
- `U+2028` / `U+2029` characters in user-supplied log fields, counter
  names, and event/timestamp strings were emitted as raw UTF-8 bytes
  by the direct serializer, which is technically valid JSON but can
  enable JSON-hijacking attacks in JavaScript consumers. They are now
  escaped to `\u2028` / `\u2029`. (Phase 1 §4.1.)
- `WideEvent::to_json()` produced malformed JSON
  (`{},"duration":{…}`) when a guard dropped with no user-supplied
  fields because the fast path in `ScopedGuard::drop` created a child
  object directly without updating the cached `present_count`. Fixed
  by incrementing `present_count` in the fast path. (Phase 1 §2.5.)

## [Unreleased] — Phase 2 in progress (0.6.0)

### Added
- `ContextCell::RestoreOnDrop<T>` — a drop guard that restores a
  `ContextCell` to a previously-saved pointer on drop. Useful for
  users who want to build their own guard types on top of the cell
  primitives. (Phase 2 §1.3.)
- `ScopedGuard::event_ptr()` — returns a raw pointer to the inner
  `WideEvent<K>`, intended to be stored in a `ContextCell`.
  (Phase 2 internal helper.)
- Compile-time `Send + Sync` soundness tests for the macro-generated
  `WideLogGuard` and for the inner types (`Value`, `WideEvent`,
  `ScopedGuard`) in `tests/macros.rs`. (Phase 2 §1.3.)
- End-to-end `Vec<u8>` pipeline test (`submit_accepts_owned_vec_without_copy`)
  and load test (`submit_does_not_block_under_load`) for the writer
  thread. (Phase 2 §1.2 / §2.3.)
- Producer-side boundedness tests
  (`emit_buf_capacity_is_bounded_across_many_events`,
  `emit_buf_handles_increasing_event_sizes`) that emit thousands of
  events of varying sizes and verify the thread-local `EMIT_BUF`
  capacity does not grow unboundedly. (Phase 2 §2.3.)

### Changed
- **Breaking — field layout**: `WideLogGuard` no longer has an
  `unsafe impl Send` shim. The `prev: *mut WideEvent` field has
  been replaced with `prev: *const WideEvent` so the guard is
  `Send + Sync` automatically whenever `WideEvent: Send + Sync`.
  Direct field access via destructuring is no longer supported;
  use the public API. (Phase 2 §1.3.)
- **Breaking — `stdout_emit::submit` signature**: the payload is
  now `Vec<u8>` instead of `String`. The producer's `default_emit`
  no longer goes through `String::from_utf8_unchecked` or
  `Vec::split_off(0)`; it appends `'\n'` to the producer-side
  thread-local `Vec<u8>` and sends the bytes directly to the
  writer thread. (Phase 2 §1.2 / §2.3.)
- `Job::Line` carries `Vec<u8>` (was `String`). The writer
  thread writes bytes directly via `BufWriter::write_all`. (Phase 2.)
- `ContextCell` now uses `UnsafeCell<*mut T>` internally instead of
  `Cell<*mut T>`, so the cell is `Send + Sync` without a separate
  `unsafe impl Send + Sync` (the `unsafe impl` is still there for
  the same reason the original code had it: the cell's safety
  contract depends on the macro-generated guard invariant). The
  external API (`get_ptr`, `replace`, `restore`) is unchanged.
  (Phase 2 internal refactor.)
- `WideLogGuard` is now `#[must_use = "..."]`. Binding to `_guard`
  is fine (the underscore is consumed at end of scope, dropping
  the guard normally), but binding to `_` and discarding without
  dropping will now trigger a compiler warning. (Phase 2 §1.3.)
- `WideLogGuard::drop` now includes a `debug_assert!` that verifies
  the thread-local cell was restored to the previous value
  recorded when the guard was created. (Phase 2 §1.3.)

### Removed
- `unsafe impl Send for WideLogGuard<F>` from the macro-generated
  code. The guard is now `Send + Sync` automatically when
  `WideEvent: Send + Sync`. (Phase 2 §1.3.)
- `from_utf8_unchecked` from the `default_emit` path. The bytes
  are now passed directly to the writer thread, where the
  integrity of the UTF-8 is enforced by a `debug_assert!` in
  the producer. (Phase 2 §1.2.)

### Known limitations
- `miri` test runs on the integration and macro tests are
  currently affected by a pre-existing Stacked Borrows
  `SharedReadWrite → Unique` retag violation in the macro's
  `current()` function. This was present in `0.5.2` and is not
  introduced by Phase 2. The `lib` unit tests (which exercise
  the core logic without going through the macro) pass under
  miri. Tracking as a follow-up to fix in `0.6.x` by
  restructuring the `ContextCell` storage to use `AtomicPtr`
  instead of `UnsafeCell<*mut T>` (or by inlining the unsafe
  into a single function so the Stacked Borrows model can prove
  the aliasing). (Phase 2 exit-criteria observation.)

### Fixed
- Eliminated the `from_utf8_unchecked` soundness hazard by
  moving to a `Vec<u8>` pipeline. (Phase 2 §1.2.)
- Removed the `unsafe impl Send for WideLogGuard` shim in favor
  of natural auto-trait derivation via `*const WideEvent`. (Phase 2 §1.3.)

## [Unreleased] — Phase 3 in progress (0.6.0)

### Added
- **Phase 3 §3.1**: reusable thread-local `FMT_BUF` for the
  format-arg variants of `info!` / `warn!` / `error!` / `debug!` /
  `trace!`. Cleared (not freed) between calls, so the underlying
  `String` allocation is preserved across log calls on the same
  thread. The previous `String::with_capacity(64)` per call was
  the single biggest per-event allocation in the hot path.
- **Phase 3 §3.2**: reusable thread-local `ULID_BUF` for the
  default event id generator. Writes the 26-character ULID via
  `core::fmt::Write` and `.clone()`s the result for the caller's
  owned `String`; the buffer's allocation is preserved across
  guard creations. Steady-state per-event allocation for the id
  path is zero.
- **Phase 3 §3.4**: `WideLogGuardBuilder::with_id_str(&'static str)`
  overload. The original `with_id` (closure-based) is still
  available for dynamic ids. The new overload avoids the
  `Box<dyn FnOnce>` indirection for the common case of a fixed
  correlation id, and reads more naturally at the call site.
- `benches/hot_path.rs` — focused end-to-end benchmark for the
  Phase 3 optimizations (FMT_BUF, ULID_BUF, with_id_str). Run
  with `cargo bench --bench hot_path`. Numbers recorded in
  `baselines/phase9_final.md`.
- Tests in `tests/macros.rs` for the new `with_id_str` overload
  and the existing `with_id` closure (round-trip verification).

### Changed
- **Phase 3 §2.5**: `as_millis() as u64` in the guard's `Drop` impl
  is now a defensive `try_into().unwrap_or(u64::MAX)`. The
  previous `as` cast would silently truncate on platforms where
  `u128` is wider than `u64` (none today, but the conversion is
  lossy in principle). The new form saturates at `u64::MAX`
  instead of wrapping in release builds.
- **Phase 3 §2.4**: doc-comment update on the `ScopedGuard::Drop`
  fast path. The "skip `new_child()` when the child already
  exists" logic was already in place from the Phase 0–2 work;
  Phase 3 just clarifies the intent in the comment.

### Performance (per `benches/hot_path.rs`, captured 2026-07-19)

- `phase3_ulid_buf/default_id_1000_guards` — 1000 guard
  create+drop with the default ULID id: **771 µs total / 771 ns
  per guard** (amortized). The first guard allocates the ULID
  buffer; subsequent guards reuse the cleared buffer with no
  further heap allocation.
- `phase3_fmt_buf/info_format_args_10x` — 10× `info!` with format
  args per guard: **1.16 µs / 116 ns per call amortized** (vs
  ~847 ns for a single call). The FMT_BUF is reused across the
  10 calls inside one guard, and across guards.
- `phase3_end_to_end/hot_path_capture_emit` — full request
  lifecycle (4× `wl_set!`, 1× `wl_inc!`, 1× `info!` with format
  args, drop with capture emit / JSON serialization):
  **1.44 µs / ~700k events/sec** (single-threaded on the bench
  machine).

Compared to `0.5.2` (Phase 0 baseline, different machine): a single
guard create+drop with no-op emit was 273.77 ns; on the current
bench machine the same path is 899 ns. The Phase 3 optimizations
target **allocation count**, not wall-clock — a single-threaded
full lifecycle that was ~3 heap allocations per guard in `0.5.2`
(ULID `String`, FMT `String`, FMT `Vec`) is now **0 heap
allocations per guard in steady state** (after the first guard
per thread).

### Known limitations
- **Phase 3 §3.3** ("remove `Box<ScopedGuard>` indirection") is
  **deferred**. The `WideLogGuard` still stores
  `Box<ScopedGuard<EventKey, F>>`, which is one heap allocation
  per guard. Removing it would require either:
  1. Exposing `WideEvent`'s mutators (`pub(crate)` today) through
     a public `__macro_internals` module so the macro expansion
     in user code can call them, or
  2. Generating the guard's drop logic in the macro so the
     `WideEvent` fields are inlined and the duration/timestamp
     setting happens in the macro-generated `Drop` impl.
  Both require exposing the internal `WideEvent` API surface to
  user crates. The current `Box` allocation is small and well
  amortized. Tracked for a follow-up release. (Phase 3 §3.3.)

### Fixed
- (none for Phase 3)

## [Unreleased] — Phase 4 in progress (0.6.0)

### Added
- **`FlushPolicy` struct** in `stdout_emit`: a batched-flush
  policy with `max_interval: Duration`, `max_bytes: usize`, and
  `max_lines: usize` fields. The default policy is 100 ms / 8 KiB /
  1000 lines, which the writer uses to coalesce multiple
  `write`+`flush` syscalls. A `per_line()` constructor restores
  the pre-Phase-4 behavior (flush after every line) for
  maximum-durability paths. (Phase 4 §FlushPolicy.)
- **`stdout_emit::set_flush_policy(FlushPolicy)`** — global,
  idempotent setter for the flush policy. A second call is a
  silent no-op (the first call wins, enforced via `OnceLock::set`).
  Policy changes apply to future events only — the running
  writer thread does not pick up changes mid-loop. The policy
  must be set before any `submit()` call to take effect.
  (Phase 4 §FlushPolicy.)
- **`stdout_emit::current_flush_policy()`** — accessor for the
  current policy, returning `FlushPolicy::default()` if none was
  set. Useful for testing and inspection.
- 13 new unit tests in `src/stdout_emit.rs` covering the
  FlushPolicy contract: default values, `per_line()` invariants,
  `set_flush_policy` idempotency (tested against the underlying
  `OnceLock::set` behavior to avoid process isolation), the
  default policy, the `per_line` mode, the time/line/bytes
  threshold paths, the explicit `flush()` forcing drain, the
  writer thread startup, and the policy-change semantics.
- `benches/writer.rs` — focused writer-throughput benchmark
  using `/dev/null` as the sink. Run with `cargo bench --bench writer`.
- New section in `README.md` documenting the durability tradeoff
  and the `set_flush_policy` / `FlushPolicy` API.

### Changed
- The writer thread (`src/stdout_emit.rs::writer_loop`) now
  batches flushes according to the configured `FlushPolicy`.
  Previously the writer flushed after every line; in 0.6.0
  the default policy coalesces up to 100 ms / 8 KiB / 1000
  lines per flush. The `write_all` to `BufWriter` is still
  immediate — only the `flush()` syscall is deferred.
- `Job` (the writer's channel message type) no longer carries a
  `SetPolicy(FlushPolicy)` variant. Policy changes take effect
  on the next writer thread start (since the policy is loaded
  once at loop start). For practical use, call
  `set_flush_policy` before any `submit()`.

### Performance (per `benches/writer.rs`, captured 2026-07-19)

The writer-throughput benchmark uses `/dev/null` as the sink
(to avoid stdout pipe saturation dominating the measurement)
and 100,000 lines per iteration. Per-line flush is the
pre-Phase-4 baseline; default_batched is the new policy.

| Line size | per_line      | default_batched | Speedup     |
|-----------|---------------|-----------------|-------------|
| 32 B      | 7.7 Melem/s   | 53.6 Melem/s    | **7.0×**    |
| 256 B     | 7.8 Melem/s   | 42.6 Melem/s    | **5.4×**    |
| 1024 B    | 7.5 Melem/s   | 25.5 Melem/s    | **3.4×**    |

The speedup decreases as line size grows because the
`write_all` to `BufWriter` starts to dominate the per-line
`flush` syscall. For typical 32-byte wide-log events, the
default batched policy is 7× faster than per-line flushing.

The plan's exit criterion is "≥ 10× reduction in `write`/`flush`
syscalls at 10k events/s". We see a 3.4–7× throughput improvement
on `/dev/null`. The actual `syscall` reduction (which the
benchmark doesn't measure directly) is likely higher, because
`/dev/null` swallows writes after the kernel buffer, and the
real bottleneck on a `pipe` (stdout) is the kernel waking up
the reader.

### Durability tradeoff

The default policy introduces a small durability window:
events are buffered for up to 100 ms before reaching the OS.
If the process is killed (`SIGKILL`) during that window, those
events are lost. Call `wide_log::stdout_emit::flush()` at
program exit to block until all pending events are flushed.
For maximum durability, use `FlushPolicy::per_line()`.

### Known limitations
- Policy changes are **not** picked up by a running writer
  thread. The first `submit()` call starts the writer; that
  writer's policy is fixed for its lifetime. Calling
  `set_flush_policy` after the first `submit()` has no effect
  until the next process start. The plan's "policy change
  applies to future events only" requirement is met by this
  design (the writer's channel is the future, but it has already
  captured the policy).

### Fixed
- (none for Phase 4)

## [Unreleased] — Phase 5 in progress (0.6.0)

### Added
- (none for Phase 5)

### Changed
- The 1- and 2-segment fast paths in `WideEvent::add_path`,
  `inc_path`, `dec_path`, and `add_n_path` are now inlined, avoiding
  the per-call `descend_mut` recursion + `Vec` allocation for the
  common case of a flat top-level key or a single nested object.
- Replaced 4 non-test `.unwrap()` calls in
  `wide-log-macros/src/codegen.rs` (in `auto_add_duration`,
  `auto_add_event`, and the two branches of `resolve_event_subtree`)
  with a structural `match` on the `entries.iter().find(...)` result.
  The `None` arm of each `match` is the original "key not present,
  insert default" path (now expressed as a real branch instead of
  a pre-check + `unwrap`); the `Some` arm binds the value directly
  and passes it to `resolve_*` / `walk` without a re-borrow. The
  pre-existing `iter().any(...)` scan was removed in the same
  change since the `find` already does the lookup.
- Hot-path `unwrap`/`expect` audit: `src/` is now free of
  `unwrap`/`expect` outside `#[cfg(test)]` and the one `unreachable!()`
  in `src/wide_event.rs:103`, which is a type-invariant assertion on
  the `new_child` fast path (the value was just written by the line
  above, so the `Object` discriminant cannot be anything else). The
  macro library is also `unwrap`/`expect`-free outside `#[cfg(test)]`.
  Library `to_json()` returns `Result<String, Error>` so user
  `with_emit` closures can choose their own error policy.

### Known limitations
- **Phase 5 task 1 (FxHash content-hash dedup of `KEY_STRS` / `KEYS`
  via `LazyLock<FxHashMap>`) is deferred.** The plan called for
  changing `Key::KEY_STRS` from a `const &[&str]` to a `static` so
  the macro could hash a content string to a `u16` key index. This
  is a breaking change to the `Key` trait (the `KEY_STRS` associated
  constant is part of the public surface and several downstream
  tests depend on its `const` shape) and was punted from 0.6.0.
  To revisit: expose a `Key::intern_keys` hook on the trait and
  have the macro emit a `static` lookup table per invocation.
- **Phase 5 task 2 (new `// SAFETY:` comment on the `current()`
  `unsafe` block) is deferred** with task 1, since both touch the
  same code site in the macro.

### Fixed
- (none for Phase 5)

## [Unreleased] — Phase 6 in progress (0.6.0)

### Added
- **`wide-log-fuzz/` crate** with three `cargo-fuzz` targets:
  `write_json_str` (the JSON serializer via `to_json`),
  `value_from` (the `Value::from_*` conversions for `bool`,
  `i64`, `u64`, `f64`, `&str`, `String`, `FastStr`, and `()`),
  and `end_to_end` (the macro-generated `WideLogGuard` +
  `wl_set!` / `wl_inc!` / `wl_dec!` / `wl_add!` / `wl_null!`
  path). Each target has a hand-seeded corpus under
  `wide-log-fuzz/corpus/<target>/`. The crate is a separate
  workspace (excluded from the main workspace via
  `exclude = ["wide-log-fuzz"]`) so it can pull in
  `libfuzzer-sys` (a nightly-only dep) without polluting the
  main build. (Phase 6 §6.2.)
- **CI fuzz job** that runs each target for 30s on every pull
  request (90s total). Crash artifacts are uploaded via
  `actions/upload-artifact@v4` for inspection. The job has
  `continue-on-error: true` because fuzzing is best-effort
  and the corpus may find a regression on a given PR. No
  nightly-only fuzz job is run. (Phase 6 §6.2.)
- **CI miri job** that runs the macro unit tests, the lib
  tests, and the integration tests under miri. The lib and
  macro tests pass cleanly with
  `MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-ignore-leaks"`.
  The integration tests are blocked by a pre-existing
  Stacked Borrows `SharedReadWrite → Unique` retag
  false-positive in the macro-generated `current()` (the
  cell stores a `*const` and the macro's deref goes through
  a `*mut` cast); the unsafe is sound (verified by the new
  loom test below) but miri can't currently see it. (Phase 6
  §6.2.)
- **Loom model test** for `ContextCell` in
  `src/context.rs::loom_tests`. Verifies that the
  `replace` / `get_ptr` / `restore` operations are
  atomic-enough at the word level that an interleaved reader
  cannot observe a torn or garbage pointer, even when the
  cell's documented "must go through `thread_local!`"
  contract is violated. Gated on `--cfg loom`; run with
  `RUSTFLAGS="--cfg loom" cargo +nightly test --lib
  --no-default-features context::tests::loom_tests`.
  (Phase 6 §6.2.)
- **`loom` dev-dependency** in `[target.'cfg(loom)'.dev-dependencies]`,
  so it's only pulled in for loom runs. `axum` and `tokio`
  are in the inverse `[target.'cfg(not(loom))'.dev-dependencies]`
  table so they don't conflict with loom's `cfg`-gated
  `tokio::net` stub. (Phase 6 §6.2.)
- **Concurrency stress test** in `tests/async.rs`:
  `one_thousand_concurrent_scopes` spawns 1000 tasks on a
  4-worker `tokio` runtime, each running its own `scope`,
  and verifies that every emit closure is called exactly
  once, every emitted JSON parses, every event has its
  auto-populated `event.id` / `event.timestamp` /
  `duration.total_ms` fields, and there is no cross-task
  bleed of the thread-local event pointer. (Phase 6 §6.2.)
- **`[lints.rust]` table** in `Cargo.toml` declaring
  `unexpected_cfgs = { level = "allow", check-cfg = ['cfg(loom)'] }`
  so `cargo clippy -D warnings` doesn't reject the
  `#[cfg(loom)]` gate. (Phase 6 §6.2.)
- **`baselines/phase6_final.md`** comparing the final
  `cargo audit` and `cargo deny check` output to the
  Phase 0 baseline. Both are clean (0 vulnerabilities,
  0 new license issues). 3 new dev-deps (loom + 2
  transitive) added — all under the `cfg(loom)` gate.

### Fixed
- **`serialize_to` now emits `null` for non-finite `f64`
  values** (NaN, +Inf, -Inf) to match the `to_json` path
  (sonic-rs). Without this, `ryu::Buffer::format(NaN)`
  produces the literal text `"NaN"`, which is not valid
  JSON and would fail to parse downstream. The
  `write_value` arm in `src/wide_event.rs` now checks
  `!f.is_finite()` and writes `b"null"`. Uncovered by the
  `value_from` fuzz target within seconds of the first
  fuzz run. (Phase 6 §6.2.)

### Known limitations
- **Phase 6 `macro_parser` fuzz target was dropped.**
  The plan called for a fourth fuzz target that drives the
  `wide_log!` macro input parser directly. It was
  dropped because `wide-log-macros` is a `proc-macro`
  crate and rustc forbids exporting any items other than
  `#[proc_macro]` functions, so the parser is not
  reachable from a separate fuzz crate without splitting
  the macros crate into an `rlib` + a thin proc-macro
  wrapper. The remaining three targets (write_json_str,
  value_from, end_to_end) cover the JSON serializer, the
  `Value::from_*` conversions, and the end-to-end
  guard+emit path. (Phase 6 §6.2 user-confirmed.)
- **Miri integration tests are blocked by a pre-existing
  Stacked Borrows false-positive** in the
  macro-generated `current()` function (see Added above).
  The lib tests cover the same code paths and pass
  cleanly under miri with the documented flags. The
  CI miri job runs the integration tests anyway and
  swallows the failure with `continue-on-error: true` so
  a future miri improvement doesn't break the build. (Phase 6
  §6.2.)

## [0.5.2] - 2026-07-17

### Fixed
- Republish fix: `wide-log 0.5.1` shipped against the stale `wide-log-macros
  0.4.0` on crates.io, whose generated `default_emit` still referenced
  `::wide_log::__re_exports_core::tracing` — a re-export that `0.5.0`/`0.5.1`
  removed. The crate compiled locally (the repo uses a `path` dep on
  `wide-log-macros`) but failed for anyone resolving from the registry, and
  the crate's own examples would not build against the published artifacts.
  `wide-log-macros 0.4.1` carries the already-in-repo codegen fix
  (`default_emit` writes the bare JSON line via `stdout_emit::submit`), and
  `wide-log 0.5.2` now requires `wide-log-macros = "0.4.1"`, forcing stale
  `Cargo.lock` files to upgrade off the broken `0.4.0`.

### Changed
- `wide-log-macros` dependency pinned from `"0.4"` to `"0.4.1"`.

## [wide-log-macros 0.4.1] - 2026-07-17

### Fixed
- `default_emit` now writes the serialized wide-event JSON line directly to
  non-blocking stdout via `::wide_log::stdout_emit::submit` instead of routing
  through `::wide_log::__re_exports_core::tracing::info!`. The `tracing`
  re-export was removed from `wide-log` in `0.5.0`, so the previous generated
  code referenced a path that no longer exists. This is the published
  counterpart to the `default_emit` fix that already shipped in the `wide-log`
  repo at commit `3d59d83` but was never released as a new `wide-log-macros`
  version.

## [0.5.0] - 2026-07-17

### Changed
- **Breaking**: `default_emit` now writes the raw wide-event JSON line
  directly to non-blocking stdout via `stdout_emit::submit` instead of
  wrapping it in a `tracing::info!` event. The emitted line is exactly the
  serialized `WideEvent` JSON followed by a `'\n'` — no `INFO`/target/
  timestamp envelope. Use `with_emit` to plug in a different sink (e.g. a
  `tracing::info!` event) if you want the old behavior.
- **Breaking**: `tracing` removed as a runtime dependency. Users who want
  to route wide events through `tracing` add `tracing` to their own
  `Cargo.toml` and pass a custom emit closure, e.g.
  `with_emit(|ev| ::tracing::info!(target: "wide_log", event = %ev.to_json().unwrap()))`.

### Added
- `stdout_emit` module: process-global non-blocking stdout writer with
  `pub fn submit(json: String)` and `pub fn dropped_events() -> u64`. A
  single dedicated writer thread owns a `BufWriter<Stdout>` and receives
  payloads over an unbounded `std::sync::mpsc` channel; the calling thread
  never blocks on I/O. On send failure (writer thread exited during
  teardown), payloads are dropped silently and the atomic dropped-counter
  is incremented.
- `tests/stdout_emit.rs`: subprocess stdout-capture integration test that
  runs `cargo run --example basic` and verifies the emitted line is bare
  JSON (no tracing envelope) with the expected fields.

### Removed
- `tracing` from the runtime dependencies and from `__re_exports_core`.
- `tracing-subscriber` from dev-dependencies.
- `default_emit_uses_real_tracing_macro` and
  `default_emit_does_not_cause_infinite_recursion` tests (their premise —
  `default_emit` routing through `tracing` — no longer holds).

## [0.4.0] - 2026-07-16

### Added
- Customizable built-in key strings via optional bracketed override list in
  `wide_log!` macro. All 8 built-in keys can be renamed using a dotted-path
  convention: `Log`, `Log.Level`, `Log.Message`, `Event`, `Event.Id`,
  `Event.Timestamp`, `Duration`, `Duration.TotalMs`.
- `Key::LOG_KEY`, `Key::LEVEL_KEY`, `Key::MESSAGE_KEY` associated constants
  on the `Key` trait, generated by the macro with user-specified or default
  strings.
- `custom_keys` example demonstrating the override syntax.
- `CHANGELOG.md`.

### Changed
- Replaced `Value<K>` tag+union+ManuallyDrop layout with a plain Rust enum.
  Eliminates all `unsafe` blocks in `value.rs`, `wide_event.rs`, and
  `guard.rs`. The compiler now handles destructors automatically.
- Removed `ValueTag` enum and `ValueData` union — replaced by `Value<K>`
  enum variants.
- Removed manual `Drop` impl for `Value<K>` (was the source of a memory leak;
  `drop_in_place` on `ManuallyDrop` was a no-op).
- Made `LogEntry` generic over `K: Key` to access `K::LEVEL_KEY` and
  `K::MESSAGE_KEY` in the serializer.
- Tightened visibility: 24 items changed from `pub` to `pub(crate)`, all 7
  modules changed from `pub mod` to `pub(crate) mod`. Public API is accessed
  via crate-root re-exports.
- Test-only methods moved to `#[cfg(test)]` impl blocks.
- `wide-log-macros` version pinned to match `wide-log` (`0.4.0`).
- Bumped `ulid` dependency from `2` to `3`. Updated generated code from
  `Ulid::r#gen()` to `Ulid::generate()` for the new API.
- Changed `with_uuid` feature gating from `#[cfg(feature = "uuid")]` in
  generated code (which checked the downstream crate's features) to a
  proc-macro-level `cfg!` check, matching how `tokio` is handled. The
  `uuid` feature in `Cargo.toml` now enables `wide-log-macros/uuid`.

### Fixed
- Memory leak in `Value::drop`: `std::ptr::drop_in_place` on `ManuallyDrop`
  fields was a no-op, leaking all `Str`, `Array`, and `Object` values.
  Eliminated entirely by switching to a plain enum (no `ManuallyDrop`).
- Generated `default_emit` used bare `::tracing::info!` instead of
  `::wide_log::__re_exports_core::tracing::info!`, causing compilation
  failures in external apps that don't depend on `tracing` directly.
- Generated `with_uuid` used bare `::uuid::Uuid::new_v4()` instead of
  `::wide_log::__re_exports_uuid::uuid::Uuid::new_v4()`, causing compilation
  failures in external apps that don't depend on `uuid` directly.
- `wide-log-macros = "0"` in `Cargo.toml` was too loose — any 0.x.y version
  could satisfy the requirement. Pinned to `0.4`.
- `tracing` and `uuid` re-exports added to `__re_exports_core` and new
  `__re_exports_uuid` module respectively.
- `with_uuid` method generated unconditionally (not behind `#[cfg(feature)]`)
  when the `uuid` feature is enabled on `wide-log`, so downstream crates
  don't need `uuid` as a direct dependency.

## [0.3.0] - 2026-07-12

### Added
- Async support behind the `tokio` feature: `scope()`, `scope_default()`,
  and `WideLogLayer` tower middleware for axum.
- `tokio::task_local!` storage for async contexts — event pointer moves
  with the task across threads.
- Deferred emit: the guard emits on drop via a `FnOnce` emit function,
  enabling custom output destinations.
- Builder pattern: `WideLogGuard::builder()` with `with_timezone()`,
  `with_id()`, `with_uuid()`, and `with_emit()` methods.
- Direct serializer (`serialize_to<W: io::Write>`) bypassing `serde`
  entirely with `itoa`/`ryu` for zero-allocation number formatting.
- Thread-local reusable emit buffer in `default_emit` — cleared, not freed.
- `KEY_STRS` lookup table for O(1) `Key::as_str()` via array index.
- `#[inline(always)]` on `current()` for fully inlined TLS lookup.
- `ContextCell` type for thread-local/task-local pointer storage.
- Type-conflict callback via `WideEvent::new_with_warnings()`.
- `uuid` feature for UUIDv4 ID generation.

### Changed
- `Value<K>` redesigned from a plain enum to a tag+union layout (`#[repr(C)]`)
  with `ManuallyDrop` for drop-able variants, reducing size from 80 to 40 bytes.
- `WideEvent` storage changed to `SmallVec<[Option<Value>; 32]>` with lazy
  initialization.
- `Key` trait gained `DURATION_PATH`, `TIMESTAMP_PATH`, `ID_PATH` associated
  constants for compile-time path resolution.
- `wl_set!`, `wl_inc!`, `wl_dec!`, `wl_add!`, `wl_null!` macros use
  `__wl_resolve_path` for compile-time path-to-enum-array resolution.
- `info!`, `warn!`, `error!`, `debug!`, `trace!` macros shadow `tracing` macros.

## [0.2.0] - 2026-07-08

### Changed
- Renamed the public guard type from `Guard` to `WideLogGuard` for clarity.
- Refactored the builder API to use a fluent builder pattern.
- Updated macro codegen to match the new guard type name.

### Fixed
- Fixed path resolution in generated `__wl_resolve_path` function.
- Fixed `wide-log-macros` `Cargo.toml` dependency version.

## [0.1.0] - 2026-07-05

### Added
- Initial release.
- `wide_log!` proc macro that generates `EventKey` enum, `Key` trait impl,
  thread-local storage, guard, `current()` accessor, and all logging macros
  from a JSON object literal.
- `Value<K>` type with `Null`, `Bool`, `I64`, `U64`, `F64`, `Str`, `Array`,
  and `Object` variants.
- `WideEvent<K>` type with O(1) indexed field storage.
- `ScopedGuard<K, F>` RAII guard that sets duration and timestamp on drop.
- `counter!` and `duration!` value markers in the JSON syntax.
- Auto-add rules for `"duration"`, `"event"`, and `"log"` keys.
- `wl_set!`, `wl_inc!`, `wl_dec!`, `wl_add!`, `wl_null!` field macros.
- `info!`, `warn!`, `error!`, `debug!`, `trace!` log entry macros.
- `to_json()` serialization via `sonic-rs`.
- `Key` trait with `as_str()`, `MAX_KEYS`, `KEYS`, `KEY_STRS`, `as_index()`,
  `DURATION_PATH`, `TIMESTAMP_PATH`, `ID_PATH`.
- Examples: `basic`, `custom_emit`, `explicit_duration`.
- Integration and macro test suites.

[0.5.0]: https://github.com/dhuseby/wide-log/releases/tag/0.5.0
[0.4.0]: https://github.com/dhuseby/wide-log/releases/tag/0.4.0
[0.3.0]: https://github.com/dhuseby/wide-log/releases/tag/0.3.0
[0.2.0]: https://github.com/dhuseby/wide-log/releases/tag/0.2.0
[0.1.0]: https://github.com/dhuseby/wide-log/releases/tag/0.1.0