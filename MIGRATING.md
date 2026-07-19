# Migrating from `tracing` to `wide-log`

This document maps every common `tracing` concept to its `wide-log`
equivalent, with code snippets. It is aimed at users who already know
`tracing` and want to understand the mental-model differences when
switching to `wide-log`'s schema-first, wide-event approach.

If you want a one-line summary: `tracing` is *event-stream* logging
(each `info!` is its own structured record); `wide-log` is *wide-event*
logging (one JSON object per request, accumulated over the request
lifetime and emitted as a single line on completion). The trade-off is
that `tracing` lets you scatter logs across many calls; `wide-log`
asks you to declare the full shape up front.

---

## 1. `tracing::info!` → `wide_log::info!`

`tracing::info!("request received")` and
`wide_log::info!("request received")` look identical at the call
site, but they mean different things:

- **`tracing::info!`** — emits one structured record immediately.
  Each call is a separate event in the log stream.
- **`wide_log::info!`** — appends an entry to the **log array** of
  the currently active wide event. Nothing is emitted until the
  surrounding guard drops. The wide event itself is one JSON line
  per request.

```rust
// tracing
tracing::info!(method = "GET", path = "/ok", "request received");
tracing::info!("response sent");

// wide-log — same call shape, but both messages are accumulated
// into the same wide event and emitted as one JSON line on guard drop.
let _guard = WideLogGuard::builder().build();
info!("request received");          // → log[0]
info!("response sent");              // → log[1]
// {"log":[{"level":"info","message":"request received"},
//         {"level":"info","message":"response sent"}], ...}
```

### Shadowing caveat

The generated `info!` / `warn!` / `error!` / `debug!` / `trace!`
macros are `#[macro_export]`'d at the crate root. When the user
does `use wide_log::*;` (or imports the macros via the generated
`wide_log!` invocation), these **shadow** the `tracing` macros.
To call the real `tracing::info!` from inside a file that also
`use`s `wide_log::*`, use the fully qualified path:

```rust
// Inside a `wide_log!`-using module:
::tracing::info!("a real tracing event");  // fully qualified → tracing
info!("a wide-log log entry");              // unqualified → wide-log
```

The same applies to `warn!`, `error!`, `debug!`, and `trace!`.

The generated `default_emit` (the function that runs when the guard
drops) does **not** route through `tracing` — it writes the
serialized JSON line directly to non-blocking stdout via
`stdout_emit::submit`. If you want to keep the `tracing` fmt
envelope, install a custom emit:

```rust
let _guard = WideLogGuard::builder()
    .with_emit(|ev| {
        if let Ok(json) = ev.to_json() {
            ::tracing::info!(event = %json);  // envelope-prefixed line
        }
    })
    .build();
```

See `examples/tracing_emit.rs` for a runnable example.

---

## 2. `tracing::Span::enter()` → `wide_log!` guard

`tracing` uses RAII guards (`Span::enter()`) to attach a span to the
current thread/task. `wide-log` uses the same RAII pattern, but the
guard is constructed from a **schema declaration** (a JSON literal
passed to the `wide_log!` macro), not from individual span fields.

```rust
// tracing
let span = tracing::info_span!("request", method = %method, path = %path);
let _enter = span.enter();
// ... do work, call info!(), etc. ...
drop(_enter);  // span closes

// wide-log — declare the schema once, at module scope, then construct
// a guard at each call site. The guard is #[must_use], so binding it
// to `_guard` is the idiomatic way to enforce drop-on-scope-exit.
wide_log!({
    "http": { "method": null, "path": null },
    "requests": counter!,
});

fn handle_request(method: &str, path: &str) {
    let _guard = WideLogGuard::builder().build();
    wl_set!("http.method", method);
    wl_set!("http.path", path);
    wl_inc!("requests");
    // ... do work, call info!(), etc. ...
    // _guard drops → duration.total_ms + event.timestamp set, JSON written
}
```

| Concept | `tracing` | `wide-log` |
|---|---|---|
| Span declaration | `info_span!(name, ...)` at each call site | `wide_log!({...})` at module scope, once per crate |
| Enter a span | `let _e = span.enter();` | `let _g = WideLogGuard::builder().build();` |
| Field updates | `tracing::Span::current().record("key", &value)` | `wl_set!("key", value)` |
| Increment counter | `tracing::Span::current().record("count", ...)` (manual) | `wl_inc!("count")` |
| Nested span | `let _e2 = inner_span.enter();` | `scope(|ev| ..., async { ... });` (async) or nested builder (sync, see §2.1) |
| Drop the guard | `drop(_e);` or end of scope | `drop(_g);` or end of scope — `#[must_use]` catches `let _ = _g;` |

### 2.1 Nested sync scopes

`tracing` lets you enter a nested span with `let _inner = inner_span.enter();`.
`wide-log` for async uses `scope()` / `scope_default()`. For **sync**
nested scopes, you can stack guards — the inner guard's `Drop` body
restores `CURRENT_EVENT` to the value the outer guard had, so the
outer event is resumed when the inner guard drops:

```rust
let outer = WideLogGuard::builder().build();
wl_set!("requests", 1u64);

{
    let inner = WideLogGuard::builder().build();
    wl_set!("requests", 99u64);   // sets on inner event
    // inner drops here → emits inner event, then restores outer
}

wl_inc!("requests");              // mutates outer event (2)
```

Caveat: each `WideLogGuard::builder().build()` emits a separate
JSON line on drop. For a "one line per request" model, prefer
using the `async` feature's `scope()` / `scope_default()` which
emit exactly one line for the outermost scope and accumulate
inner events via the task-local cell.

---

## 3. `#[instrument]` → `wide_log!` schema declaration

`tracing`'s `#[instrument]` attribute auto-creates a span on every
function entry. `wide-log` has no proc-macro attribute — the
schema is declared once at module scope and the function manually
constructs a guard from it.

```rust
// tracing
#[tracing::instrument(skip_all, fields(request_id = %req.id, method = %req.method))]
async fn handle(req: Request) -> Response {
    tracing::info!("handling");
    // ...
}

// wide-log — declare the shape at the crate root, then write
// the fields explicitly at the call site.
wide_log!({
    "http": { "method": null, "path": null, "request_id": null },
});

async fn handle(req: Request) -> Response {
    let _guard = WideLogGuard::builder().build();
    wl_set!("http.method", req.method);
    wl_set!("http.path", req.path);
    wl_set!("http.request_id", req.id);
    info!("handling");
    // ...
}
```

The advantage of `wide-log`'s approach is that the shape is
**declared once** and visible in one place. Refactoring the schema
(name changes, nesting, additions) is a single edit to the
`wide_log!` call; the function body is unchanged. The
disadvantage is that there's no `skip_all` equivalent — every
field that ends up in the JSON must be explicitly `wl_set!`'d.

---

## 4. `tracing::field::display` → explicit `wl_set!` calls

`tracing` lets you embed a value as a field marker, which is
formatted lazily:

```rust
// tracing
tracing::info!(user_id = %user.id, "user logged in");
tracing::debug!(request = ?req, "got request");  // Debug
tracing::info!(count = 42, "static count");
```

`wide-log` has no field-marker syntax — every value is a real
field in the schema. Use the `From` conversions on `Value<K>`:

```rust
// wide-log — declare the fields, then set them.
wide_log!({
    "user_id": null,
    "request": null,
    "count": null,
});

info!("user logged in");
wl_set!("user_id", user.id);          // i64, u64, etc. via From
wl_set!("request", format!("{:?}", req));  // Debug → String
wl_set!("count", 42u64);
```

For `tracing::Span::current().record("field", &value)`, the
`wide-log` equivalent is exactly the same as a `wl_set!`:

```rust
// tracing
let span = tracing::info_span!("req", user_id = tracing::field::Empty);
let _e = span.enter();
span.record("user_id", &user.id);

// wide-log
let _g = WideLogGuard::builder().build();
wl_set!("user_id", user.id);
```

If you have many `record()` calls in hot loops, you can
batch them via `ev.add_path(...)` directly on the `&mut
WideEvent` you get from `current()`:

```rust
use wide_log::Key;
if let Some(ev) = current() {
    ev.add_path(&[EventKey::UserId], user.id);
    ev.add_path(&[EventKey::RequestId], req.id);
}
```

---

## 5. `tracing-subscriber` filtering → schema-first

`tracing-subscriber` filters and dispatches events at runtime via
`EnvFilter` / `Layer` chains. `wide-log` has no equivalent because
**all filtering happens at compile time**, via the schema
declaration. You only emit fields you declared; there is no
runtime "drop a field if it matches a filter" path.

```rust
// tracing
tracing_subscriber::registry()
    .with(EnvFilter::from_default_env())  // RUST_LOG=info,wide_log=debug
    .with(tracing_subscriber::fmt::layer())
    .init();
tracing::info!(secret = "hidden", "user logged in");
```

```rust
// wide-log — decide at schema-write time what to emit. If you
// don't want `secret` in the JSON, just don't `wl_set!` it.
wide_log!({
    "user_id": null,
});

fn handle() {
    let _g = WideLogGuard::builder().build();
    wl_set!("user_id", user.id);
    // secret is never set → never appears in the JSON
}
```

If you want conditional fields (e.g., set `secret` only in debug
builds), gate the `wl_set!` call:

```rust
#[cfg(debug_assertions)]
wl_set!("secret", secret_value);
```

The "level" of a wide-log event is implicit in its position in
the log array (`log[].level`), not in a per-call site. The
`info!` / `warn!` / `error!` / `debug!` / `trace!` macros set
`log[].level` for each entry, and downstream aggregators can
filter on that. There is no equivalent of
`tracing_subscriber::EnvFilter` because wide-log is
**schema-first** — what you declare is what you get.

---

## 6. `tracing-actix-web` / `tracing-axum` → `wide_log` middleware

`tracing-actix-web` and `tracing-axum` create a per-request span
on every HTTP request. `wide-log` provides `WideLogLayer` (a
`tower::Layer`) for the same purpose, behind the `tokio` feature:

```rust
// tracing-axum
let app = Router::new()
    .route("/ok", get(handler))
    .layer(TraceLayer::new_for_http());

async fn handler() -> &'static str {
    tracing::info!("handling");
    "ok"
}
```

```rust
// wide-log
use wide_log::wide_log;
wide_log!({
    "service": { "name": "ok-service", "version": "1.0.0" },
    "http": { "method": null, "path": null, "status": null },
});

let app = Router::new()
    .route("/ok", get(handler))
    .layer(WideLogLayer);

async fn handler() -> &'static str {
    // Guard is already active via the middleware — no scope_default() needed.
    wl_set!("http.method", "GET");
    wl_set!("http.path", "/ok");
    wl_set!("http.status", 200u64);
    info!("handling");
    "ok"
}
```

**Why a middleware, not a guard at the top of each handler:**
`tokio::task_local!` has no imperative setter — you can only set
a task-local value by wrapping a future with `.scope(value,
future)`. The `WideLogLayer` middleware does that wrapping
automatically, so handlers don't need a `scope_default()` call
at the top.

**Why not `WideLogGuard::builder().build()` in the handler:** the
sync builder uses `thread_local!`, which is stale if a
multi-threaded runtime moves the task to another thread. The
middleware uses `tokio::task_local!`, which moves with the task.

The `WideLogLayer` is implemented in `src/middleware.rs` (under
`#[cfg(feature = "tokio")]`) and is approximately 40 lines of
boilerplate wrapping a tower `Service` impl. See
`examples/axum_ok.rs` for a runnable example.

---

## 7. What `tracing` can do that `wide-log` cannot

| Capability | `tracing` | `wide-log` |
|---|---|---|
| Multiple log streams / channels | ✓ (`Layer` chain) | ✗ — one event per guard |
| Runtime field filtering (e.g., `RUST_LOG`) | ✓ | ✗ — schema-first |
| Spans attached to existing types via `#[instrument]` | ✓ | ✗ — explicit guard at each call site |
| Cross-process / distributed context (OpenTelemetry) | ✓ (via `tracing-opentelemetry`) | ✗ — out of scope |
| Sampling / rate-limiting at the logging layer | ✓ | ✗ |
| Log levels with per-level `Layer`s | ✓ | ✗ — `log[].level` is just a field |
| Multiple subscriber instances in one process | ✓ | ✗ — one `CURRENT_EVENT` per thread |
| Sync and async APIs | ✓ | ✓ |
| Structured fields | ✓ | ✓ |
| Custom formatters | ✓ (subscriber fmt) | ✓ (custom emit closure) |
| JSON output | ✓ (via `tracing-subscriber::fmt::format::Json`) | ✓ (default emit, bare JSON line) |

---

## 8. What `wide-log` does better

| Property | `tracing` | `wide-log` |
|---|---|---|
| Wide-event ergonomics | Manual — must build a `tracing::Span` and remember to record every field on every branch | Schema-first — declare the full shape once, every field is set via `wl_set!` (compile-time-checked path) |
| Cardinality / log cost | O(n) structured records per request (n = `info!` call count) | O(1) per request — one JSON line regardless of log entry count |
| Aggregator query | `service:foo AND http.status:200 AND duration>100ms` requires pre-indexed fields; fields not in the schema are missing or stored as `unspecified` | Same — fields not in the schema are simply absent |
| Field type safety | Dynamic (formatter-typed at runtime) | Static (enum-keyed, `From`-converted) |
| Performance | ~100-200ns per `info!` call (with `tracing-subscriber` fmt) | ~10-30ns per `wl_set!` (no subscriber, direct field write); one final serialization on guard drop |
| Allocation per `info!` | 1-2 (string + record) | 0 (writes to a thread-local reusable buffer) |
| Final emit | One record per `info!` call, flushed individually by `tracing-subscriber` | One JSON line per request, batched via the writer thread (`FlushPolicy`) |
| Schema drift across services | Manual — each service declares its own spans | Centralized in one `wide_log!` call, shareable as a crate |
| Field name typos | Silent (the field doesn't exist, no error) | Compile error (the path doesn't resolve, no run) |

---

## 9. Code-side migration checklist

When porting an existing `tracing`-based service to `wide-log`:

1. **Inventory your spans.** For each `info_span!` / `#[instrument]`,
   decide whether it represents a *wide event* (one JSON line per
   occurrence) or an *event stream* (many records per occurrence).
   Only the first kind maps cleanly to `wide-log`. The second
   should keep `tracing` for now.

2. **Declare the schema.** For each wide event, write a
   `wide_log!({...})` block. The shape should match the
   aggregator's expected fields. If you're using `Log` /
   `Event` / `Duration` under different names, use the override
   list.

3. **Replace `info_span!` + `enter()` with `WideLogGuard`.** The
   guard goes at the top of the function; the `_guard` binding
   enforces drop-on-scope-exit. The schema declaration is at
   module scope, not at each call site.

4. **Replace `record("key", val)` with `wl_set!("key", val)`.**
   For `tracing` field markers (`%` Display, `?` Debug, plain
   value), use the appropriate `From` impl on `Value<K>` (or
   `format!("{:?}", val)` for Debug).

5. **Replace `#[instrument]` with manual `WideLogGuard`.** The
   `#[instrument]` attribute has no `wide-log` equivalent.

6. **For axum / actix-web servers, install `WideLogLayer`** (for
   axum) or wrap the request handler in `scope_default(async { ... })`
   (for actix-web). The middleware sets the task-local so
   `wl_set!` works anywhere in the request handler.

7. **If you need both `tracing` and `wide-log` in the same
   process** (e.g., during migration), use fully qualified
   `::tracing::info!(...)` for tracing calls and the unqualified
   `info!(...)` for wide-log calls. They share the macro name
   space.

8. **Run `wide_log::stdout_emit::flush()` at the end of
   `main`** to ensure all buffered events are written before
   the process exits. Without this, the default `FlushPolicy`'s
   100 ms batching can drop the last batch on shutdown if the
   process is killed before the timer fires. (Since 0.6.3 the
   writer thread self-wakes on its own timer, so an idle channel
   is no longer a hang risk; `flush()` is still recommended as a
   clean-shutdown guarantee.)

---

## 10. Quick reference

| `tracing` | `wide-log` |
|---|---|
| `tracing::info!(...)` | `info!(...)` (shadowed; fully qualify to call real tracing) |
| `tracing::info_span!(name, k = v)` | `wide_log!({ "k": null }); let _g = WideLogGuard::builder().build(); wl_set!("k", v);` |
| `#[tracing::instrument]` | No equivalent — write the guard explicitly |
| `span.record("k", v)` | `wl_set!("k", v)` |
| `tracing::Span::current().record(...)` | `if let Some(ev) = current() { ev.add_path(&[EventKey::K], v); }` |
| `tracing_subscriber::fmt::init()` | No equivalent — `default_emit` writes bare JSON to stdout |
| `tracing_subscriber::EnvFilter` | No equivalent — schema-first, no runtime filter |
| `TraceLayer::new_for_http()` (axum) | `WideLogLayer` |
| `tracing::field::display(v)` | `wl_set!("k", v)` (uses `From` for the value type) |
| `tracing::field::debug(v)` | `wl_set!("k", format!("{:?}", v))` |
| `let _e = span.enter();` | `let _g = WideLogGuard::builder().build();` |
| `tracing::Span::current()` (no context) | `current()` returns `None` (no active event) |
| `tracing::info!(target: "my_target", ...)` | Use a custom emit that targets a specific destination |
| `tracing::dispatcher::set_default(...)` | Not supported (one `CURRENT_EVENT` per thread) |
| `tracing::collector::Collect` | Not supported (no pluggable collector) |
| `#[instrument(err)]` / `#[instrument(ret)]` | Manually call `wl_set!("error", err.to_string())` before drop |
| `span.in_scope(|| { ... })` | Move the body into a block: `{ let _g = ...; ... }` |

---

## 11. See also

- [README.md](./README.md) — quick start, builder pattern, macro reference
- [CHANGELOG.md](./CHANGELOG.md) — full release notes (latest: 0.6.3)
- [baselines/phase9_final.md](./baselines/phase9_final.md) — performance benchmarks
- [`examples/basic.rs`](./examples/basic.rs) — minimal `wide_log!` usage
- [`examples/axum_ok.rs`](./examples/axum_ok.rs) — `WideLogLayer` with axum
- [`examples/tracing_emit.rs`](./examples/tracing_emit.rs) — custom emit that routes through `tracing`
