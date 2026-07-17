# Wide Logging Strategy

Taking inspiration from [https://loggingsucks.com](https://loggingsucks.com)
this document outlines how to use this Rust crate to implement a wide logging
strategy for your application. Instead of outputting log entries all throughout
the processing of a transaction, wide logging instead accumulate details from
each step/subsystem and outputs a single JSON formatted structured log object
per transaction.

The crate provides a single `wide_log!` proc-macro that generates the key enum,
guard, storage, and all logging macros from a JSON object literal describing
the event shape.

## Wide Structure

For every transaction an application processes, it emits one wide log: a single JSON
object that accumulates fields throughout the request/task lifecycle and is
serialized on completion. An application may include any additional data in any
structure as deemed necessary for the operation of the application.

The `wide-log` crate guarantees the presence of three top-level keys in every
emitted event, auto-adding them if the user does not declare them:

| **Field**     | **Type** | **Description**                                                  |
| ------------- | -------- | ---------------------------------------------------------------- |
| `event`       | object   | [Event Data](#event-data) — auto-added if absent                 |
| `duration`    | object   | [Duration Data](#duration-data) — auto-added if absent           |
| `log`         | array    | [Log Entries](#log-entries) — auto-added, never declared by user |

An application typically also declares an `app` object (see
[App Data](#app-data)), but it is not required by the crate.

Example of a minimal emitted wide log (auto-added keys only):

```
{
  "event": {
    "timestamp": "2026-07-12T12:00:00.000Z",
    "id": "01J6XK5R7N4Q2V3XH65992TQV"
  },
  "duration": {
    "total_ms": 42
  }
}
```

Example of a typical emitted wide log with user-declared keys and
accumulated log entries:

```
{
  "app": {
    "name": "example-app",
    "version": "1.0.0"
  },
  "http": {
    "method": "GET",
    "path": "/ok",
    "status": 200
  },
  "requests": 1,
  "duration": {
    "total_ms": 869.6
  },
  "event": {
    "timestamp": "2026-07-12T12:00:00.000Z",
    "id": "01J6XK5R7N4Q2V3XH65992TQV"
  },
  "log": [
    { "level": "info", "message": "request received" },
    { "level": "warn", "message": "upstream slow" },
    { "level": "info", "message": "request completed" }
  ]
}
```

### Log Entries

The `log` key is handled entirely internally by the crate. The user never
declares `"log"` in the `wide_log!` JSON. Log entries are accumulated via the
`info!`, `warn!`, `error!`, `debug!`, and `trace!` macros (which shadow the
`tracing` macros of the same names when `wide_log` is in scope). Each entry is
serialized as:

```
{ "level": "info", "message": "request received" }
```

Literal string messages are stored zero-copy as `&'static str`; formatted
messages are owned via `FastStr`. The `log` array appears in the serialized
output only when at least one log entry has been accumulated.

### Event Data

Every wide log documents an event. The `event` object is auto-added as
`"event": { "timestamp": null, "id": null }` if the user does not declare it.
The guard sets these fields automatically:

| **Field**    | **Type**        | **Description**                                                                  |
| ------------ | --------------- | -------------------------------------------------------------------------------- |
| `timestamp`  | RFC 3339 string | Time the event is emitted (set on guard drop, in the guard's timezone)           |
| `id`         | string          | A [ULID](https://github.com/ulid/spec) to correlate transactions across apps  |

The `timestamp` is set to the current time as an RFC 3339 string on guard
drop. The timezone defaults to UTC and is configurable via
`WideLogGuard::builder().with_timezone(...)`.

The `id` is set on `build()` (guard creation) to a ULID string by default, or
a UUIDv4 string when the `uuid` feature is enabled and `with_uuid()` is used.
A custom ID generator can be supplied via `WideLogGuard::builder().with_id(...)`.

Users may add additional fields to the `event` object by declaring them in
the `wide_log!` JSON. The crate ensures `timestamp` and `id` are always
present, adding them if missing.

Example:

```
{
  "event": {
    "timestamp": "2026-07-12T12:00:00.000Z",
    "id": "01J6XK5R7N4Q2V3XH65992TQV"
  }
}
```

### App Data

Identifies which application emitted the event. The `app` object is
user-defined — the crate does not require it and does not impose any schema.
Applications define their own `app` object in the `wide_log!` JSON with
whatever fields they need. Common fields include:

| **Field** | **Type** | **Description**              |
| --------- | -------- | ---------------------------- |
| `name`    | string   | Canonical app identifier |
| `version` | string   | App version              |

Example declaration in `wide_log!`:

```rust
wide_log!({
    "app": {
        "name": null,
        "version": "1.0.0",
    },
    // ...
});
```

Example output:

```
{
  "app": {
    "name": "claims-manager",
    "version": "1.1.0"
  }
}
```

### Duration Data

Timing is critical in our event processing. The `duration` object is
auto-added as `"duration": { "total_ms": duration! }` if the user does not
declare it. The guard sets `duration.total_ms` to the elapsed milliseconds
(measured from guard creation to drop) on drop.

The crate requires exactly one `duration!` marker leaf in the `"duration"`
subtree. If absent, the macro defaults to `total_ms`. Any other duration may be
recorded here as long as it follows the naming convention: `<name>_<unit>`
with valid `<unit>` values: `ns`, `ms`, `s`, `m`, `h` for nanoseconds,
milliseconds, seconds, minutes, and hours respectively.

| **Field**  | **Type** | **Description**                       |
| ---------- | -------- | ------------------------------------- |
| `total_ms` | u64      | Total duration in milliseconds (default leaf) |
| `<any>_ns` | u64      | Any duration measured in nanoseconds  |
| `<any>_ms` | u64      | Any duration measured in milliseconds |
| `<any>_s`  | u64      | Any duration measured in seconds      |
| `<any>_m`  | u64      | Any duration measured in minutes      |
| `<any>_h`  | u64      | Any duration measured in hours        |

The user may declare a custom duration leaf name (e.g., `"wall_ms": duration!`)
in which case the guard sets that leaf instead of `total_ms`.

Example:

```
{
  "duration": {
    "total_ms": 33.78,
    "auth_ms": 0.18,
    "proof_build_ms": 32.12,
    "response_build_ms": 1.48
  }
}
```

## Implementation: The `wide-log` Crate

The strategy is implemented by the `wide-log` crate (v0.3.0), a high-speed wide
logging system for Rust. A single `wide_log!` proc-macro generates everything
from a JSON object literal.

### Value Markers

The `wide_log!` JSON supports the following value markers:

| Marker | Meaning | Guard Behavior |
| --- | --- | --- |
| `duration!` | This key is the duration leaf. Value is elapsed ms, computed on drop. | Set on drop via `DURATION_PATH`. |
| `counter!` | This key is an incrementable counter. Initialized to 0 (absent). | No auto-set; `wl_inc!` initializes to 1. |
| `null` | This key exists but has no default value. | No auto-set. |
| `"literal"` | A string default value. | Set on creation as a `FastStr`. |
| `123` | A numeric default value. | Set on creation as a `U64` or `I64`. |
| `true`/`false` | A boolean default value. | Set on creation as `Bool`. |

### Macros

| Macro | Description |
| --- | --- |
| `wl_set!(path, val)` | Set/replace a field value at a nested path |
| `wl_inc!(path)` | Increment a numeric field by 1 at a nested path (init to 1 if absent) |
| `wl_dec!(path)` | Decrement a numeric field by 1 at a nested path (init to -1 if absent) |
| `wl_add!(path, n)` | Add a number to a numeric field at a nested path |
| `wl_null!(path)` | Set a field to null at a nested path |
| `info!(msg)` / `info!(fmt, ...)` | Append info-level log entry (shadows `tracing::info!`) |
| `warn!(msg)` / `warn!(fmt, ...)` | Append warn-level log entry (shadows `tracing::warn!`) |
| `error!(msg)` / `error!(fmt, ...)` | Append error-level log entry (shadows `tracing::error!`) |
| `debug!(msg)` / `debug!(fmt, ...)` | Append debug-level log entry (shadows `tracing::debug!`) |
| `trace!(msg)` / `trace!(fmt, ...)` | Append trace-level log entry (shadows `tracing::trace!`) |

All macros are no-ops when no guard is active (`current()` returns `None`).

### Guard and Builder

The `WideLogGuard` is an RAII guard that owns a `WideEvent` and emits it on
drop. It is constructed via `WideLogGuard::builder().build()`:

| Method | Description | Default |
| --- | --- | --- |
| `with_timezone(tz: chrono_tz::Tz)` | Timezone for timestamp formatting | `chrono_tz::Tz::UTC` |
| `with_id(F: FnOnce() -> String)` | Custom ID generator closure | ULID via `ulid` crate |
| `with_uuid()` | Use UUIDv4 for ID (requires `uuid` feature) | — |
| `with_emit(F: FnOnce(&WideEvent))` | Custom emit function | `default_emit` (direct serialize + non-blocking stdout) |
| `build()` | Construct the guard | — |

On drop, the guard:
1. Sets `DURATION_PATH` to the elapsed milliseconds (u64).
2. Sets `TIMESTAMP_PATH` to the current time as an RFC 3339 string.
3. Calls the emit function with a reference to the event.

### Sync and Async Usage

- **Sync**: `WideLogGuard::builder().build()` sets a `thread_local!` pointer.
  The guard is dropped at end of scope.
- **Async single-thread** (requires `tokio` feature): `scope_default(async { ... }).await`
  uses `tokio::task_local!` so the event is available across `.await` points.
- **Async multi-thread** (requires `tokio` feature): `scope_default(async { ... }).await`
  — `task_local!` moves with the task across threads.
- **Axum / Tower middleware** (requires `tokio` feature): `WideLogLayer` wraps
  every request in `scope_default()` automatically.

### Features

- `tokio` — enables async support: `scope()`, `scope_default()`,
  `WideLogLayer` tower middleware, and `tokio::task_local!` storage.
- `uuid` — enables `WideLogGuardBuilder::with_uuid()` for UUIDv4 ID
  generation instead of the default ULID.

### Performance Characteristics

The crate is designed for high-throughput logging:

- **Enum keys** — each JSON key is a `#[repr(u8)]` enum variant; a key is a
  single byte on the stack.
- **O(1) indexed storage** — values are stored in a `SmallVec<[Option<Value>; 32]>`
  indexed by `key.as_index()`. No linear scan for `add`, `inc`, `dec`, or `add_n`.
- **Tag + union Value** — `Value<K>` is a `#[repr(C)]` struct with a 1-byte tag
  and a 32-byte union, totaling 40 bytes (down from 80 bytes in the original enum
  design).
- **StaticStr variant** — `&'static str` literals are stored zero-copy as
  `Value::StaticStr`. Literal `info!("literal")` messages use `LogMsg::Static`
  for zero-copy log messages.
- **Direct serializer** — `serialize_to<W: io::Write>` writes JSON directly,
  bypassing `serde` entirely. Uses `itoa` for integer formatting and `ryu` for
  float formatting (zero-allocation number-to-ASCII).
- **KEY_STRS lookup table** — `Key::as_str()` uses a `&'static [&'static str]`
  array indexed by `as_index()`, replacing a multi-arm `match` with a branchless
  array index.
- **Thread-local reusable emit buffer** — `default_emit` writes into a
  thread-local `Vec<u8>` that is cleared (not freed) on each emit. The
  serialized JSON is then handed to a single dedicated non-blocking stdout
  writer thread via an unbounded `std::sync::mpsc` channel — the calling
  thread never blocks on I/O.
- **`#[inline(always)]` on `current()`** — the TLS/task-local pointer lookup
  is fully inlined at every call site.
- **Zero shared state** — the hot path uses thread-local/task-local pointers,
  no `Mutex`, no `Arc`, no atomics.
