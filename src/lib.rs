//! # wide-log
//!
//! A high-speed wide-event logging system for Rust. A single structured event
//! accumulates fields throughout a request/task lifecycle and is emitted as
//! one JSON line on completion.
//!
//! ## Quick Start
//!
//! The [`wide_log!`] macro takes a JSON object literal and generates the key
//! enum, `Key` trait impl, thread-local storage, guard builder, `current()`
//! accessor, `scope()` / `scope_default()` async functions (behind the
//! `tokio` feature), `WideLogLayer` tower middleware (behind the `tokio`
//! feature), the `WideLogCaptureLayer` capture layer (behind the `tracing`
//! feature), and the logging macros (`wl_set!`, `wl_inc!`, and the level
//! macros `info!` etc. — generated when the `tracing` feature is off,
//! re-exported from `tracing` when it is on) in one invocation.
//!
//! ```
//! use wide_log::wide_log;
//! // With the feature on the generated macros do not exist; the crate
//! // re-exports tracing's macros and the import keeps `info!` resolving
//! // in both modes. With the feature off the import is unnecessary (the
//! // generated macro is at the crate root via `#[macro_export]`) and a
//! // plain `use` would collide with it, hence the cfg gate.
//! #[cfg(feature = "tracing")]
//! #[allow(unused_imports)]
//! use wide_log::info;
//!
//! wide_log!({
//!     "service": {
//!         "name": null,
//!         "version": "1.0.0",
//!     },
//!     "requests": counter!,
//! });
//!
//! # fn main() {
//! let _guard = WideLogGuard::builder().build();
//! wl_set!("service.name", "example-service");
//! wl_inc!("requests");
//! info!("request received");
//! // _guard drops → duration.total_ms set, timestamp set,
//! // event emitted as a JSON line to non-blocking stdout.
//! # wide_log::stdout_emit::flush();
//! # }
//! ```
//!
//! ## Auto-Added Keys
//!
//! - **`"log"`** — log entries from `info!()`, `warn!()`, etc. Handled
//!   internally; never declared by the user.
//! - **`"duration"`** — elapsed time as an f64 in the unit indicated by the
//!   leaf suffix (default ms). Auto-added as
//!   `"duration": { "total_ms": duration! }` if not declared.
//! - **`"event"`** — event metadata. Auto-added as
//!   `"event": { "timestamp": null, "id": null }` if not declared.
//!   The `timestamp` is set to an RFC 3339 string on drop. The `id` is
//!   set to a ULID string (or UUIDv4 with the `uuid` feature) on `build()`.
//!
//! ## Customizable Key Strings
//!
//! All 8 built-in key strings can be renamed using an optional bracketed
//! override list before the JSON object:
//!
//! ```
//! use wide_log::wide_log;
//! #[cfg(feature = "tracing")]
//! #[allow(unused_imports)]
//! use wide_log::info;
//!
//! wide_log!([
//!   Event.Id => "correlation_id",
//!   Log.Level => "severity",
//! ], {
//!   "service": { "name": null },
//!   "requests": counter!,
//! });
//! # fn main() {
//! # let _guard = WideLogGuard::builder().build();
//! # wl_inc!("requests");
//! # info!("request received");
//! # wide_log::stdout_emit::flush();
//! # }
//! ```
//!
//! See the README for the full list of override paths.
//!
//! ## Builder Pattern
//!
//! Use `WideLogGuard::builder()` to construct a guard. The builder allows
//! specifying a custom timezone, ID generator, and emit function:
//!
//! ```
//! use wide_log::wide_log;
//!
//! wide_log!({ "service": { "name": null }, "requests": counter! });
//!
//! # fn main() {
//! // Default: UTC timezone, ULID ID, non-blocking stdout emit
//! let _guard = WideLogGuard::builder().build();
//!
//! // Custom timezone
//! use chrono_tz::Tz;
//! let _guard = WideLogGuard::builder()
//!     .with_timezone(Tz::America__New_York)
//!     .build();
//!
//! // Custom ID generator
//! let _guard = WideLogGuard::builder()
//!     .with_id(|| "my-custom-id".to_string())
//!     .build();
//!
//! // Custom emit function
//! let _guard = WideLogGuard::builder()
//!     .with_emit(|ev| { println!("{}", ev.to_json().unwrap()); })
//!     .build();
//! # wide_log::stdout_emit::flush();
//! # }
//! ```
//!
//! UUIDv4 IDs are available with the `uuid` feature: `WideLogGuard::builder().with_uuid().build()`.
//!
//! ## Log-Message Modes
//!
//! The `tracing` feature selects how log messages reach the wide event:
//!
//! - **Feature off (default)** — the generated `info!`, `warn!`,
//!   `error!`, `debug!`, `trace!` macros append
//!   `{level, message}` entries to the active event's `log` array
//!   directly. These macros shadow `tracing::info!` etc. when both
//!   are in scope; to call the real tracing macros, use the fully
//!   qualified path: `::tracing::info!(...)`. The default
//!   `default_emit` writes the serialized JSON line to non-blocking
//!   stdout via [`stdout_emit::submit`].
//! - **Feature on** — the generated level macros are not compiled;
//!   the crate re-exports `tracing`'s level macros instead, so
//!   unqualified `info!` etc. resolve to canonical tracing. Records
//!   emitted through the tracing subscriber (from the application
//!   or any dependency crate) are appended to the active event's
//!   `log` array by the generated `WideLogCaptureLayer`. See
//!   "Capturing tracing records" below.
//!
//! ## Using wide-log from downstream crates
//!
//! A crate that depends on `wide-log` but never invokes [`wide_log!`]
//! can still log into the active wide event: the five level macros are
//! exported at the crate root, and both the item-import and path-call
//! spellings resolve. The import compiles in either feature mode:
//! feature-off binds the hook-backed `#[macro_export]` macro rules;
//! `--features tracing` binds `tracing`'s macros through the re-export.
//!
//! The contract:
//!
//! - **A schema must exist somewhere in the binary's dependency
//!   graph.** Some crate must invoke [`wide_log!`] and hold an active
//!   guard; the macros append to the innermost active event through a
//!   hook that guard installs.
//! - **No guard, no output.** When no guard is active on the calling
//!   thread or task, every level macro is a silent no-op.
//! - Inside a crate that invoked [`wide_log!`], the schema crate's
//!   generated macros shadow the crate-root import (text-proximity
//!   rule); both paths append identical `{level, message}` entries.
//!
//! The runnable three-crate demonstration lives in `tests/downstream/`
//! in the repository (a schema lib, a schema-less lib, and a schema-less
//! binary that log from all three into one event).
//!
//! ## Capturing tracing records
//!
//! With the `tracing` feature enabled, add the generated
//! `WideLogCaptureLayer` to the tracing subscriber stack:
//!
//! ```text
//! tracing_subscriber::registry()
//!     .with(...)
//!     .with(WideLogCaptureLayer)
//!     .init();
//! ```
//!
//! While a wide-log guard is active, every tracing record with level
//! at or below the layer's configured floor (all levels by default;
//! set a floor with `WideLogCaptureLayer::new().with_max_level(...)`)
//! is appended to the active event's `log` array as a
//! `{level, message}` entry and still forwarded to the other layers.
//! A record with no `message` field renders its other fields as
//! `name=value` text; a record with both appends the fields after
//! the message. Records with no message and no fields are skipped.
//!
//! The generated `default_emit` routes the serialized event through
//! `::tracing::info!(target: "wide_log", event = %json)`; the capture
//! layer skips records with that reserved target, so the finished
//! event is not re-captured into itself. With the default emit in
//! this mode a formatting layer is needed to see output. The
//! recommended pattern is a capture-only stack (no formatting layer,
//! so the subscriber prints nothing) plus a custom emit that prints
//! the bare JSON line — the stdout output is then identical in both
//! feature modes. See "Capturing tracing records" below.
//!
//! ## Features
//!
//! - `tokio` — enables async support: `scope()`, `scope_default()`,
//!   `WideLogLayer` tower middleware, and `tokio::task_local!` storage.
//! - `uuid` — enables `WideLogGuardBuilder::with_uuid()` for UUIDv4 ID
//!   generation instead of the default ULID.
//! - `tracing` — capture mode: adds optional `tracing` and
//!   `tracing-subscriber` dependencies, stops compiling the generated
//!   level macros (re-exports `tracing`'s instead), routes
//!   `default_emit` through `::tracing::info!`, and provides the
//!   `WideLogCaptureLayer` capture layer. See "Log-Message Modes".

pub(crate) mod context;
pub(crate) mod error;
pub(crate) mod guard;
#[cfg(not(feature = "tracing"))]
pub(crate) mod hook_registry;
pub(crate) mod key;
pub(crate) mod log;
pub(crate) mod value;
pub(crate) mod wide_event;

#[cfg(feature = "tokio")]
pub(crate) mod middleware;

pub use error::Error;
pub use guard::ScopedGuard;
pub use key::Key;
pub use value::Value;
pub use wide_event::WideEvent;

pub use context::ContextCell;
pub use context::RestoreOnDrop;

/// Public API surface used by the `wide_log!` macro expansion. The macro
/// needs to construct a [`WideEvent`], call mutators like
/// [`WideEvent::add_path`], and read `values` / `present_count` to
/// implement the inlined fast path for the guard's drop. The mutators
/// are `pub(crate)` for end users (so they don't bypass the
/// schema-first API), but the macro is expanded in user crates and
/// therefore needs a public surface.
///
/// Stability: the contents of this module are an internal
/// implementation detail of the `wide_log!` macro and may change
/// between releases. Do not call these directly.
#[doc(hidden)]
pub mod __macro_internals {
    pub use crate::value::Value;
    // The hook registry backs the crate-root level macros below. Schema
    // crates' generated guards install and pop hooks through this surface
    // so appends from any dependent crate reach the active event.
    #[cfg(not(feature = "tracing"))]
    pub use crate::hook_registry::{
        LogHook, LogHookGuard, append_log_entry, append_log_entry_fmt, log_hook_stack_depth,
        pop_log_hook, push_log_hook,
    };
    // The generated code references its own hook shim through
    // `__macro_internals` (so a hook path exists even when the shim itself
    // is a schema crate's local item). Re-exporting the type-erased
    // signature here keeps the shim assignable from any schema crate.
    #[cfg(not(feature = "tracing"))]
    pub use crate::hook_registry::LogHook as __wl_log_hook;
    // The async `scope()` family wraps its future in this call to seed the
    // task's hook stack.
    #[cfg(all(not(feature = "tracing"), feature = "tokio"))]
    pub use crate::hook_registry::scope_log_hook;
}

pub mod stdout_emit;

/// The `wide_log!` proc-macro. See the [crate-level documentation](crate)
/// for syntax and usage details.
///
/// Takes a JSON object literal as its only parameter. The JSON structure
/// defines all keys, their nesting/paths, default values, and which keys
/// are counters or durations. The macro generates:
///
/// - The `EventKey` enum (`#[repr(u8)]`, one variant per unique JSON key)
/// - The `Key` trait impl (`as_str`, `MAX_KEYS`, `as_index`,
///   `DURATION_PATH`, `TIMESTAMP_PATH`, `ID_PATH`)
/// - `__wl_resolve_path` — compile-time path resolution function
/// - Thread-local storage (`CURRENT_EVENT: ContextCell<WideEvent<EventKey>>`)
/// - `default_emit` — serializes via `sonic_rs` and writes the JSON line to
///   non-blocking stdout via `stdout_emit::submit`
/// - `WideLogGuardBuilder` — builder for constructing guards with timezone,
///   ID generator, and emit function
/// - `WideLogGuard` — guard type (constructed via `builder().build()`)
/// - `current()` — returns the innermost active event
/// - `scope()` / `scope_default()` (behind `tokio` feature)
/// - `WideLogLayer` tower middleware (behind `tokio` feature)
/// - `WideLogCaptureLayer` capture layer (behind `tracing` feature)
/// - Logging macros: `wl_set!`, `wl_inc!`, `wl_dec!`, `wl_add!`,
///   `wl_null!` always; the level macros `info!`, `warn!`, `error!`,
///   `debug!`, `trace!` only when the `tracing` feature is off
///   (with the feature on, the crate re-exports `tracing`'s level
///   macros and records are captured via `WideLogCaptureLayer`)
///
/// # Value Markers
///
/// | Marker | Meaning |
/// |---|---|
/// | `duration!` | Duration leaf; set to elapsed time as an f64 in the unit indicated by the leaf suffix (default ms) on drop |
/// | `counter!` | Incrementable counter; init to 0 (absent) |
/// | `null` | No default value; set via `wl_set!` |
/// | `"literal"` | String default; set on guard creation |
/// | `123` | Numeric default; set on guard creation |
/// | `true`/`false` | Boolean default; set on guard creation |
///
/// # Duration Auto-Add Rules
///
/// If no `"duration"` key is declared, the macro adds
/// `"duration": { "total_ms": duration! }` automatically. See the README for
/// the full duration resolution table.
///
/// # Event Auto-Add Rules
///
/// If no `"event"` key is declared, the macro adds
/// `"event": { "timestamp": null, "id": null }` automatically.
/// The `timestamp` is set to an RFC 3339 string on drop.
/// The `id` is set to a ULID string (or UUIDv4 with the `uuid` feature)
/// on `build()`.
pub use wide_log_macros::wide_log;

#[cfg(feature = "tokio")]
pub mod __re_exports {
    pub use tokio;
    pub use tower;
}

pub mod __re_exports_core {
    pub use chrono;
    pub use chrono_tz;
    pub use ulid;
}

#[cfg(feature = "uuid")]
pub mod __re_exports_uuid {
    pub use uuid;
}

// With the `tracing` feature on, the generated level macros are not
// compiled. Re-export `tracing`'s level macros at the crate root so
// unqualified `info!`/`warn!`/`error!`/`debug!`/`trace!` call sites
// resolve to canonical tracing (captured into the active wide event
// through the `WideLogCaptureLayer` layer) without app-code edits.
#[cfg(feature = "tracing")]
pub use ::tracing::{debug, error, info, trace, warn};

// With the `tracing` feature off, the crate root exports its own level
// macros so dependent crates that never invoke `wide_log!` can import
// them (`use wide_log::info;`). Each macro appends a `{level, message}`
// entry to the innermost active wide event through the hook registry in
// `__macro_internals`; with no guard active anywhere in the dependency
// graph, the call is a silent no-op.
//
// In a crate that invoked `wide_log!`, the schema crate's generated
// macros shadow these by text-proximity — both paths append identically,
// the generated ones through the typed `CURRENT_EVENT` directly.
#[cfg(not(feature = "tracing"))]
#[macro_export]
macro_rules! info {
    ($msg:literal) => {
        ::wide_log::__macro_internals::append_log_entry("info", $msg)
    };
    ($fmt:literal, $($arg:tt)*) => {
        ::wide_log::__macro_internals::append_log_entry_fmt("info", ::std::format_args!($fmt, $($arg)*))
    };
}

#[cfg(not(feature = "tracing"))]
#[macro_export]
macro_rules! warn {
    ($msg:literal) => {
        ::wide_log::__macro_internals::append_log_entry("warn", $msg)
    };
    ($fmt:literal, $($arg:tt)*) => {
        ::wide_log::__macro_internals::append_log_entry_fmt("warn", ::std::format_args!($fmt, $($arg)*))
    };
}

#[cfg(not(feature = "tracing"))]
#[macro_export]
macro_rules! error {
    ($msg:literal) => {
        ::wide_log::__macro_internals::append_log_entry("error", $msg)
    };
    ($fmt:literal, $($arg:tt)*) => {
        ::wide_log::__macro_internals::append_log_entry_fmt("error", ::std::format_args!($fmt, $($arg)*))
    };
}

#[cfg(not(feature = "tracing"))]
#[macro_export]
macro_rules! debug {
    ($msg:literal) => {
        ::wide_log::__macro_internals::append_log_entry("debug", $msg)
    };
    ($fmt:literal, $($arg:tt)*) => {
        ::wide_log::__macro_internals::append_log_entry_fmt("debug", ::std::format_args!($fmt, $($arg)*))
    };
}

#[cfg(not(feature = "tracing"))]
#[macro_export]
macro_rules! trace {
    ($msg:literal) => {
        ::wide_log::__macro_internals::append_log_entry("trace", $msg)
    };
    ($fmt:literal, $($arg:tt)*) => {
        ::wide_log::__macro_internals::append_log_entry_fmt("trace", ::std::format_args!($fmt, $($arg)*))
    };
}
