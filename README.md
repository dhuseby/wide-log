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
- **SmallVec storage** — up to 24 entries and 8 log entries are inline on the
  stack; zero heap allocation in the common case.
- **FastStr SSO** — short strings (< ~23 bytes) use small-string optimization;
  zero heap allocation for short values and log messages.
- **sonic-rs SIMD** — serialization is SIMD-accelerated for fast JSON output.
- **Zero shared state** — the hot path uses thread-local/task-local pointers,
  no `Mutex`, no `Arc`, no atomics.

## Quick Start

```rust
use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

fn main() {
    tracing_subscriber::fmt().init();

    let _guard = EventKeyGuard::new();

    wl_set!("service.name", "example-service");
    wl_inc!("requests");

    info!("request received");
    warn!("upstream slow");

    // _guard drops here → duration.total_ms is set automatically,
    // event is serialized to JSON, emitted via ::tracing::info!:
    //
    // {"service":{"name":"example-service","version":"1.0.0"},
    //  "duration":{"total_ms":42},"requests":1,
    //  "log":[{"level":"info","message":"request received"},
    //         {"level":"warn","message":"upstream slow"}]}
}
```

### Auto-Added Keys

The macro automatically adds two keys that every wide event needs:

1. **`"log"`** — the list of log entries accumulated by `info!()`, `warn!()`,
   etc. Handled entirely internally; the user never declares `"log"` in the
   JSON. It appears in the serialized output automatically.

2. **`"duration"`** — the duration of the wide event lifecycle. If the user
   does not declare `"duration"` in the JSON, the macro automatically adds
   `"duration": { "total_ms": duration! }`. The guard sets
   `duration.total_ms` to the elapsed milliseconds on drop.

## Usage

### Non-Async Code

```rust
use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

fn main() {
    tracing_subscriber::fmt().init();

    // Create guard — takes no arguments. Sets default values from JSON
    // (service.version = "1.0.0"), starts the timer:
    let _guard = EventKeyGuard::new();

    // Set per-request field values:
    wl_set!("service.name", "example-service");
    wl_inc!("requests");

    // Add log messages — these accumulate in the "log" array:
    info!("request received");
    warn!("upstream slow");

    // _guard drops here → duration.total_ms is set automatically,
    // event is serialized to JSON, emitted via ::tracing::info!.
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
    tracing_subscriber::fmt().init();

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
    // guard drops here → duration.total_ms set, event emitted
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
    tracing_subscriber::fmt().init();

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
    // guard drops → event emitted with duration.total_ms
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
        "name": null,
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
    wl_set!("service.name", "ok-service");
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
    tracing_subscriber::fmt().init();

    let app = Router::new()
        .route("/ok", get(ok))
        .layer(WideLogLayer);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

**Why middleware, not `EventKeyGuard::new()`:** `tokio::task_local!` has no
imperative setter — you can only set a task-local value by wrapping a future
with `.scope(value, future)`. The middleware provides that wrapper
automatically. `EventKeyGuard::new()` (the sync API) sets `thread_local!`,
which is stale if a multi-threaded runtime moves the task to another thread.

**No special setup in nested calls:** any `info!()`, `warn!()`, `wl_set!`,
etc. call — whether in the handler directly, in a called sync function, or
in a called async function — finds the event via `current()`, which checks
task-local first. No arguments or threading needed.

### Custom Emit

```rust
use wide_log::wide_log;

wide_log!({
    "service": { "name": null, "version": "1.0.0" },
    "requests": counter!,
});

fn main() {
    let _guard = EventKeyGuard::new_with_emit(|ev| {
        if let Ok(json) = ev.to_json() {
            println!("{json}");
        }
    });

    wl_set!("service.name", "example-service");
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
| `duration!` | This key is the duration. Value is elapsed ms, computed on drop. | Set on drop via `DURATION_PATH`. |
| `counter!` | This key is an incrementable counter. Initialized to 0 (absent). | No auto-set; `wl_inc!` initializes to 1. |
| `null` | This key exists but has no default value. | No auto-set. |
| `"literal"` | A string default value. | Set on creation as a `FastStr`. |
| `123` | A numeric default value. | Set on creation as `U64` or `I64`. |
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

The generated `default_emit` function uses `::tracing::info!` (fully
qualified) to avoid calling the shadowing `info!` macro, which would append to
the log list instead of emitting the JSON line.
