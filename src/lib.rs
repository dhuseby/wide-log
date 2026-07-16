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
//! feature), and all logging macros (`wl_set!`, `wl_inc!`, `info!`, etc.) in
//! one invocation.
//!
//! ```rust,ignore
//! use wide_log::wide_log;
//!
//! wide_log!({
//!     "service": {
//!         "name": null,
//!         "version": "1.0.0",
//!     },
//!     "requests": counter!,
//! });
//!
//! fn main() {
//!     tracing_subscriber::fmt().init();
//!     let _guard = WideLogGuard::builder().build();
//!     wl_set!("service.name", "example-service");
//!     wl_inc!("requests");
//!     info!("request received");
//!     // _guard drops → duration.total_ms set, timestamp set,
//!     // event emitted as JSON.
//! }
//! ```
//!
//! ## Auto-Added Keys
//!
//! - **`"log"`** — log entries from `info!()`, `warn!()`, etc. Handled
//!   internally; never declared by the user.
//! - **`"duration"`** — elapsed time in ms. Auto-added as
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
//! ```rust,ignore
//! wide_log!([
//!   Event.Id => "correlation_id",
//!   Log.Level => "severity",
//! ], {
//!   "service": { "name": null },
//!   "requests": counter!,
//! });
//! ```
//!
//! See the README for the full list of override paths.
//!
//! ## Builder Pattern
//!
//! Use `WideLogGuard::builder()` to construct a guard. The builder allows
//! specifying a custom timezone, ID generator, and emit function:
//!
//! ```rust,ignore
//! // Default: UTC timezone, ULID ID, tracing emit
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
//! // UUIDv4 ID (requires `uuid` feature)
//! let _guard = WideLogGuard::builder()
//!     .with_uuid()
//!     .build();
//!
//! // Custom emit function
//! let _guard = WideLogGuard::builder()
//!     .with_emit(|ev| { println!("{}", ev.to_json().unwrap()); })
//!     .build();
//! ```
//!
//! ## `info!` Shadowing
//!
//! The generated `info!`, `warn!`, `error!`, `debug!`, `trace!` macros shadow
//! `tracing::info!` etc. when both are in scope. To call the real tracing
//! macros, use the fully qualified path: `::tracing::info!(...)`.
//!
//! ## Features
//!
//! - `tokio` — enables async support: `scope()`, `scope_default()`,
//!   `WideLogLayer` tower middleware, and `tokio::task_local!` storage.
//! - `uuid` — enables `WideLogGuardBuilder::with_uuid()` for UUIDv4 ID
//!   generation instead of the default ULID.

pub(crate) mod context;
pub(crate) mod error;
pub(crate) mod guard;
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
/// - `default_emit` — serializes via `sonic_rs` and emits via `::tracing::info!`
/// - `WideLogGuardBuilder` — builder for constructing guards with timezone,
///   ID generator, and emit function
/// - `WideLogGuard` — guard type (constructed via `builder().build()`)
/// - `current()` — returns the innermost active event
/// - `scope()` / `scope_default()` (behind `tokio` feature)
/// - `WideLogLayer` tower middleware (behind `tokio` feature)
/// - All logging macros: `wl_set!`, `wl_inc!`, `wl_dec!`, `wl_add!`,
///   `wl_null!`, `info!`, `warn!`, `error!`, `debug!`, `trace!`
///
/// # Value Markers
///
/// | Marker | Meaning |
/// |---|---|
/// | `duration!` | Duration leaf; set to elapsed ms on drop |
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
