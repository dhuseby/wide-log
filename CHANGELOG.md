# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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