# `wide-log` Crate

A high-speed wide-event logging system for Rust. A single structured event
accumulates fields throughout a request/task lifecycle and is emitted as one
JSON line on completion.

## Design and Purpose

Wide-event logging gives you one row per request in your log aggregator with
every dimension attached — perfect for high-cardinality exploratory analysis.
`wide-log` makes this ergonomic with a single `wide_log!` macro that generates
everything from a JSON object literal.

### Performance Philosophy

- **Enum keys** — each JSON key becomes a `#[repr(u8)]` enum variant; a key is
  a single byte on the stack, not a heap-allocated string.
- **O(1) indexed storage** — values are stored in a `SmallVec<[Option<Value>; 32]>`
  indexed by `key.as_index()`. No linear scan for `add`, `inc`, `dec`, or `add_n`.
  The SmallVec is lazily initialized — grows on demand, never zeroed upfront.
- **Plain enum Value** — `Value<K>` is a Rust enum with 9 variants. The
  compiler handles destructors automatically — no `unsafe`, no `ManuallyDrop`,
  no manual `Drop` impls.
- **StaticStr variant** — `&'static str` literals are stored zero-copy as
  `Value::StaticStr`. The `info!("literal")` macro path uses `LogMsg::Static`
  for zero-copy log messages.
- **Direct serializer** — the `serialize_to<W: io::Write>` method writes JSON
  directly, bypassing `serde` entirely. Uses `itoa` for integer formatting and
  `ryu` for float formatting (zero-allocation number-to-ASCII).
- **KEY_STRS lookup table** — `Key::as_str()` uses a `&'static [&'static str]`
  array indexed by `as_index()`, replacing a multi-arm `match` with a branchless
  array index.
- **Thread-local reusable emit buffer** — the `default_emit` function writes
  into a thread-local `Vec<u8>` that is cleared (not freed) on each emit. The
  buffer grows once to the high-water mark and reuses that allocation for all
  subsequent emits. The serialized JSON is then handed to a single dedicated
  non-blocking stdout writer thread via an unbounded `std::sync::mpsc`
  channel — the calling thread never blocks on I/O.
- **`#[inline(always)]` on `current()`** — the TLS pointer lookup is fully
  inlined at every `wl_set!`/`info!` call site. Uses `ContextCell::get_ptr()`
  which returns a raw pointer (no `Option` wrapping inside the closure).
- **Lazy SmallVec init** — `WideEvent::new()` starts with an empty `SmallVec`
  (zero allocation, zero zeroing). The `ensure_capacity` method grows the SmallVec
  only when a key at a given index is first accessed.
- **fn pointer for type-conflict callback** — `on_type_conflict` is an
  `Option<fn(&mut WideEvent, K)>` (8 bytes, `Copy`, no Arc, no clone).
- **Zero shared state** — the hot path uses thread-local/task-local pointers,
  no `Mutex`, no `Arc`, no atomics.

## Quick Start

```rust
use wide_log::wide_log;

wide_log!({
    "service": {
        "name": "example-service",
        "version": "1.0.0",
    },
    "requests": counter!,
});

fn main() {
    let _guard = WideLogGuard::builder().build();

    wl_inc!("requests");

    info!("request received");
    warn!("upstream slow");

    // _guard drops here → duration.total_ms and event.timestamp are set
    // automatically, event is serialized to JSON and written to
    // non-blocking stdout as a single line:
    //
    // {"service":{"name":"example-service","version":"1.0.0"},
    //  "duration":{"total_ms":42},"requests":1,
    //  "event":{"timestamp":"2026-07-12T12:00:00.000Z","id":"01J6XK5R..."},
    //  "log":[{"level":"info","message":"request received"},
    //         {"level":"warn","message":"upstream slow"}]}
}
```

### Customizable Built-in Key Strings

All 8 built-in key strings can be renamed using an optional bracketed override
list before the JSON object. This is useful when your log aggregator expects
different key names (e.g., `"correlation_id"` instead of `"id"`):

```rust
wide_log!([
    Log             => "log",
    Log.Level       => "severity",
    Log.Message     => "msg",
    Event           => "event",
    Event.Id        => "correlation_id",
    Event.Timestamp => "timestamp",
    Duration        => "duration",
    Duration.TotalMs => "total_ms"
], {
    "service": { "name": "example" },
    "requests": counter!,
});
```

The override paths use a dotted convention that mirrors the nesting structure.
Omit the override list entirely to use all defaults (backwards compatible).

#### Override Paths

| Override Path | Default | Description |
|---|---|---|
| `Log` | `"log"` | Top-level key for the log entries array |
| `Log.Level` | `"level"` | Level field within each log entry |
| `Log.Message` | `"message"` | Message field within each log entry |
| `Event` | `"event"` | Top-level key for event metadata |
| `Event.Id` | `"id"` | ID field within the event object |
| `Event.Timestamp` | `"timestamp"` | Timestamp field within the event object |
| `Duration` | `"duration"` | Top-level key for duration |
| `Duration.TotalMs` | `"total_ms"` | Milliseconds field within the duration object |

### Auto-Added Keys

The macro automatically adds three keys that every wide event needs:

1. **`"log"`** (or your custom `Log` override) — the list of log entries
   accumulated by `info!()`, `warn!()`, etc. Handled entirely internally; the
   user never declares it in the JSON. It appears in the serialized output
   automatically.

2. **`"duration"`** (or your custom `Duration` override) — the duration of the
   wide event lifecycle. If the user does not declare it in the JSON, the macro
   automatically adds `"duration": { "total_ms": duration! }`. The guard sets
   `duration.total_ms` to the elapsed time in milliseconds (f64) on drop.

3. **`"event"`** (or your custom `Event` override) — event metadata. If the
   user does not declare it in the JSON, the macro automatically adds
   `"event": { "timestamp": null, "id": null }`. The guard sets
   `event.timestamp` to the current time as an RFC 3339 string on drop. The
   builder sets `event.id` to a ULID string (or UUIDv4 with the `uuid` feature)
   on `build()`.

## Builder Pattern

Use `WideLogGuard::builder()` to construct a guard. The builder allows
specifying a custom timezone, ID generator, and emit function:

```rust
// Default: UTC timezone, ULID ID, non-blocking stdout emit
let _guard = WideLogGuard::builder().build();

// Custom timezone
use chrono_tz::Tz;
let _guard = WideLogGuard::builder()
    .with_timezone(Tz::America__New_York)
    .build();

// Custom ID generator
let _guard = WideLogGuard::builder()
    .with_id(|| "my-custom-id".to_string())
    .build();

// UUIDv4 ID (requires `uuid` feature)
let _guard = WideLogGuard::builder()
    .with_uuid()
    .build();

// Custom emit function
let _guard = WideLogGuard::builder()
    .with_emit(|ev| { println!("{}", ev.to_json().unwrap()); })
    .build();

// Combined
let _guard = WideLogGuard::builder()
    .with_timezone(Tz::America__New_York)
    .with_id(|| "custom-id".to_string())
    .with_emit(|ev| { println!("{}", ev.to_json().unwrap()); })
    .build();
```

### Builder Methods

| Method | Description | Default |
|---|---|---|
| `with_timezone(tz: chrono_tz::Tz)` | Timezone for timestamp formatting | `chrono_tz::Tz::UTC` |
| `with_id(F: FnOnce() -> String)` | Custom ID generator closure | ULID via `ulid` crate |
| `with_uuid()` | Use UUIDv4 for ID (requires `uuid` feature) | — |
| `with_emit(F: FnOnce(&WideEvent)>)` | Custom emit function | `default_emit` (serialize + non-blocking stdout) |
| `build()` | Construct the guard | — |

## Usage

### Non-Async Code

```rust
use wide_log::wide_log;

wide_log!({
    "service": {
        "name": "example-service",
        "version": "1.0.0",
    },
    "requests": counter!,
});

fn main() {
    let _guard = WideLogGuard::builder().build();

    wl_inc!("requests");

    info!("request received");
    warn!("upstream slow");

    // _guard drops here → duration.total_ms and event.timestamp are set
    // automatically, event is serialized to JSON and written to
    // non-blocking stdout.
}
```

### Async with Single-Threaded Runtime

```rust
use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

#[tokio::main(flavor = "current_thread")]
async fn main() {
    handle_request().await;
}

async fn handle_request() {
    scope_default(async {
        wl_set!("service.name", "example-service");
        wl_inc!("requests");
        info!("request received");

        fetch_upstream().await;  // log macros work across .await

        info!("request completed");
    }).await;
    // guard drops here → duration.total_ms set, event written to stdout
}

async fn fetch_upstream() {
    warn!("upstream slow");
}
```

`scope_default` uses `tokio::task_local!` so the event is available across
`.await` points.

### Async with Multi-Threaded Runtime

```rust
use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let mut handles = vec![];
    for i in 0..10 {
        handles.push(tokio::spawn(handle_request(i)));
    }
    for h in handles {
        h.await.unwrap();
    }
}

async fn handle_request(id: u64) {
    scope_default(async {
        wl_set!("service.name", format!("worker-{id}"));
        wl_inc!("requests");
        info!("request {} started", id);

        // The task may be moved to another thread here.
        // task_local! ensures the event pointer moves with it.
        tokio::task::yield_now().await;

        info!("request {} completed", id);
    }).await;
    // guard drops → event written to stdout with duration and timestamp
}
```

`task_local!` moves with the task across threads, so the event remains
accessible regardless of which thread the runtime schedules the task on.

### Axum Server with `WideLogLayer`

*(requires the `tokio` feature)*

```rust
use axum::routing::get;
use axum::Router;
use wide_log::wide_log;

wide_log!({
    "service": {
        "name": "ok-service",
        "version": "1.0.0",
    },
    "http": {
        "method": null,
        "path": null,
        "status": null,
    },
});

async fn ok() -> &'static str {
    // Guard is already active via the middleware — no scope_default() needed.
    wl_set!("http.method", "GET");
    wl_set!("http.path", "/ok");
    wl_set!("http.status", 200u64);

    info!("request received");
    // ...
    info!("request completed");
    ""
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/ok", get(ok))
        .layer(WideLogLayer);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

**Why middleware, not `WideLogGuard::builder().build()`:** `tokio::task_local!`
has no imperative setter — you can only set a task-local value by wrapping a
future with `.scope(value, future)`. The middleware provides that wrapper
automatically. `WideLogGuard::builder().build()` (the sync API) sets
`thread_local!`, which is stale if a multi-threaded runtime moves the task to
another thread.

**No special setup in nested calls:** any `info!()`, `warn!()`, `wl_set!`,
etc. call — whether in the handler directly, in a called sync function, or
in a called async function — finds the event via `current()`, which checks
task-local first. No arguments or threading needed.

### Custom Emit

```rust
use wide_log::wide_log;

wide_log!({
    "service": { "name": "example-service", "version": "1.0.0" },
    "requests": counter!,
});

fn main() {
    let _guard = WideLogGuard::builder()
        .with_emit(|ev| {
            if let Ok(json) = ev.to_json() {
                println!("{json}");
            }
        })
        .build();

    wl_inc!("requests");
    info!("request received");
}
```

### Explicit Duration

```rust
wide_log!({
    "service": { "name": null, "version": "1.0.0" },
    "duration": { "wall_ms": duration! },
    "requests": counter!,
});
// DURATION_PATH = &[Duration, WallMs] → sets duration.wall_ms on drop
```

### Custom Key Names

```rust
use wide_log::wide_log;

wide_log!([
    Event.Id => "correlation_id"
], {
    "service": { "name": "example" },
    "requests": counter!,
});

fn main() {
    let _guard = WideLogGuard::builder().build();

    // The event ID is serialized as "correlation_id" instead of "id":
    // {"event":{"timestamp":"...","correlation_id":"01J6XK5R..."}, ...}
}
```

## Macro Reference

| Macro | Description |
|---|---|
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

## JSON Syntax Reference

### Value Markers

| Marker | Meaning | Guard Behavior |
|---|---|---|
| `duration!` | This key is the duration. Value is elapsed time in the unit indicated by the leaf suffix (default ms), as an f64, computed on drop. | Set on drop via `DURATION_PATH`. |
| `counter!` | This key is an incrementable counter. Initialized to 0 (absent). | No auto-set; `wl_inc!` initializes to 1. |
| `null` | This key exists but has no default value. | No auto-set. |
| `"literal"` | A string default value. | Set on creation as a `FastStr`. |
| `123` | A numeric default value. | Set on creation as a `U64` or `I64`. |
| `true`/`false` | A boolean default value. | Set on creation as `Bool`. |

The `duration!` marker is **optional** — if not used, the macro defaults to
`duration.total_ms`. Only use `duration!` when you want a custom duration leaf
name (e.g., `"wall_ms": duration!`).

### Duration Auto-Add Rules

| User declares | Macro result | `DURATION_PATH` |
|---|---|---|
| Nothing (no `"duration"`) | Adds `"duration": { "total_ms": duration! }` | `&[Duration, TotalMs]` |
| `"duration": {}` | Fills in `"total_ms": duration!` | `&[Duration, TotalMs]` |
| `"duration": { "total_ms": duration! }` | Uses as-is | `&[Duration, TotalMs]` |
| `"duration": { "wall_ms": duration! }` | Uses as-is | `&[Duration, WallMs]` |
| `"duration": { "total_ms": null }` | Fills in `duration!` for `total_ms` | `&[Duration, TotalMs]` |
| `"duration": { "secs": null, "nanos": null }` | **Error:** no `duration!` leaf, and multiple non-duration leaves are ambiguous. | — |

The rule: there must be exactly one `duration!` leaf in the `"duration"`
subtree. If absent, the macro adds `"total_ms": duration!`. If the user
declares a `"duration"` object with only `null`/literal leaves and no
`duration!`, the macro defaults `total_ms` to `duration!` (adding it if
missing, or converting `"total_ms": null` to `"total_ms": duration!`).

### Event Auto-Add Rules

| User declares | Macro result |
|---|---|
| Nothing (no `"event"`) | Adds `"event": { "timestamp": null, "id": null }` |
| `"event": {}` | Fills in `"timestamp": null, "id": null` |
| `"event": { "timestamp": null, "id": null }` | Uses as-is |
| `"event": { "source": null }` | Adds missing `"timestamp"` and `"id"`, keeps `"source"` |
| `"event": "foo"` | **Error:** `"event"` must be an object |

The guard sets `event.timestamp` to an RFC 3339 string on drop. The builder
sets `event.id` to a ULID string (or UUIDv4 with `with_uuid()`) on `build()`.

### JSON Key to Enum Variant Naming

The macro converts JSON key names to PascalCase enum variant names:
- `"service"` → `Service`
- `"name"` → `Name`
- `"total_ms"` → `TotalMs`

The conversion: split on `_` and `.`, capitalize each word, concatenate. If
the name conflicts with a Rust keyword, append `_` (e.g., `"type"` → `Type_`).

### Path Derivation

The JSON structure directly defines the paths:
- `"service": { "name": ... }` → path `[Service, Name]`, dotted string
  `"service.name"`.
- `"duration": { "total_ms": ... }` → path `[Duration, TotalMs]`, dotted
  string `"duration.total_ms"`.
- `"requests": ...` → path `[Requests]`, dotted string `"requests"`.

The macro generates `__wl_resolve_path` with entries for every path,
including intermediate paths (e.g., `"service"` → `&[Service]`) and full leaf
paths (e.g., `"service.name"` → `&[Service, Name]`). When the input to
`__wl_resolve_path` is a string literal (as in `wl_set!("service.name", ...)`),
the compiler constant-folds the match — zero runtime cost.

## `info!` Shadowing

The `info!`, `warn!`, `error!`, `debug!`, `trace!` macros are
`#[macro_export]`'d at the crate root. When the user does `use wide_log::*;`,
these shadow `tracing::info!` etc. To call the real `tracing` macros, use the
fully qualified path: `::tracing::info!(...)`.

The generated `default_emit` function does **not** route through `tracing` —
it writes the serialized JSON line directly to non-blocking stdout via
`stdout_emit::submit`. (Wide-log's `info!`/`warn!`/etc. still shadow tracing's
when both are in scope, which is why you must use the fully qualified path
when you want the real tracing macro.)

## Flush Policy and Durability Tradeoff

The `default_emit` function hands serialized JSON lines to a dedicated
writer thread that owns a `BufWriter<Stdout>`. The writer flushes
according to a `FlushPolicy`:

| Threshold       | Default | Effect                                            |
|-----------------|---------|---------------------------------------------------|
| `max_interval`  | 100 ms  | Flush at most every 100 ms                         |
| `max_bytes`     | 8 KiB   | Flush when 8 KiB are buffered                      |
| `max_lines`     | 1000    | Flush after 1000 lines                             |

The default policy achieves a **3.4–7× throughput improvement**
over per-line flushing on typical wide-log events (measured on
`/dev/null`, 100k lines per iteration, see `benches/writer.rs`):

| Line size | per_line      | default_batched | Speedup  |
|-----------|---------------|-----------------|----------|
| 32 B      | 7.7 Melem/s   | 53.6 Melem/s    | **7.0×** |
| 256 B     | 7.8 Melem/s   | 42.6 Melem/s    | **5.4×** |
| 1024 B    | 7.5 Melem/s   | 25.5 Melem/s    | **3.4×** |

The speedup decreases as line size grows because the
`write_all` to `BufWriter` starts to dominate the per-line
`flush` syscall. The actual `write`/`flush` syscall reduction
(which the benchmark doesn't measure directly) is likely higher,
because `/dev/null` swallows writes after the kernel buffer and
the real bottleneck on a `pipe` (stdout) is the kernel waking up
the reader.

The tradeoff is a small **durability window**: if the process is
killed between when a line is submitted and when the next batched
flush fires, that line is lost. The maximum loss is bounded by
`max_interval` (100 ms by default) of buffered output, plus any
bytes the kernel hasn't yet flushed to the pipe.

The writer thread wakes itself on its own timer — it does not
rely on the arrival of new events to check the flush clock. So a
consumer reading this process's stdout pipe (a test harness, a
log shipper, or a sidecar) observes the buffered line within
`max_interval` of the last `submit`, without the producer having
to call `flush()`. (Before 0.6.3 the writer only checked the
timer when a new job arrived, so an idle channel could keep
output buffered indefinitely — a reader of the pipe would hang.)

For maximum durability, call `set_flush_policy` with
`FlushPolicy::per_line()` (the pre-Phase-4 behavior):

```rust
wide_log::stdout_emit::set_flush_policy(
    wide_log::stdout_emit::FlushPolicy::per_line()
);
```

Or call [`wide_log::stdout_emit::flush`](crate::stdout_emit::flush)
at program exit (e.g. at the end of `main`) to block until all
pending events have been written and the `BufWriter` flushed.

`set_flush_policy` is **idempotent**: a second call is a silent
no-op (the first call wins). The policy must be set before any
`submit()` call to take effect on the writer thread; subsequent
`set_flush_policy` calls have no effect on the running writer.

### Inspecting the current policy

```rust
let p = wide_log::stdout_emit::current_flush_policy();
println!("max_interval = {:?}", p.max_interval);
println!("max_bytes = {}", p.max_bytes);
println!("max_lines = {}", p.max_lines);
```

This returns `FlushPolicy::default()` (100 ms / 8 KiB / 1000
lines) if `set_flush_policy` was never called.

## Features

- `tokio` — enables async support: `scope()`, `scope_default()`,
  `WideLogLayer` tower middleware, and `tokio::task_local!` storage.
- `uuid` — enables `WideLogGuardBuilder::with_uuid()` for UUIDv4 ID
  generation instead of the default ULID.
- `tracing` — *transition aid only*. When enabled, the
  macro-generated `default_emit` routes the serialized event through
  `::tracing::info!(event = %json)` instead of writing the bare
  JSON line to non-blocking stdout. A one-time `eprintln!` warning
  is emitted on first use. New code should disable this feature
  and use the default (bare JSON to stdout) or a custom
  `with_emit` closure. See `MIGRATING.md` for the full
  `tracing → wide-log` mapping.

## Migrating from `tracing`

See [MIGRATING.md](./MIGRATING.md) for a complete
`tracing → wide_log` mapping table (every concept, with code
snippets), plus a checklist for porting an existing
`tracing`-based service.

