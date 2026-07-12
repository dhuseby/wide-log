//! # wide-log
//!
//! A high-speed wide-event logging system for Rust. A single structured event
//! accumulates fields throughout a request/task lifecycle and is emitted as
//! one JSON line on completion.
//!
//! ## Quick Start
//!
//! The [`wide_log!`] macro takes a JSON object literal and generates the key
//! enum, `Key` trait impl, thread-local storage, guard type, `current()`
//! accessor, `scope()` / `scope_default()` async functions (behind the `tokio`
//! feature), `WideLogLayer` tower middleware (behind the `tokio` feature),
//! and all logging macros (`wl_set!`, `wl_inc!`, `info!`, etc.) in one
//! invocation.
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
//!     let _guard = WideLogGuard::new();
//!     wl_set!("service.name", "example-service");
//!     wl_inc!("requests");
//!     info!("request received");
//!     // _guard drops → duration.total_ms set, event emitted as JSON.
//! }
//! ```
//!
//! ## Auto-Added Keys
//!
//! - **`"log"`** — log entries from `info!()`, `warn!()`, etc. Handled
//!   internally; never declared by the user.
//! - **`"duration"`** — elapsed time in ms. Auto-added as
//!   `"duration": { "total_ms": duration! }` if not declared.
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

pub mod context;
pub mod error;
pub mod guard;
pub mod key;
pub(crate) mod log;
pub mod value;
pub mod wide_event;

#[cfg(feature = "tokio")]
pub mod middleware;

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
/// - The `Key` trait impl (`as_str`, `MAX_KEYS`, `as_index`, `DURATION_PATH`)
/// - `__wl_resolve_path` — compile-time path resolution function
/// - Thread-local storage (`CURRENT_EVENT: ContextCell<WideEvent<EventKey>>`)
/// - `default_emit` — serializes via `sonic_rs` and emits via `::tracing::info!`
/// - `WideLogGuard` — guard type with `new()` and `new_with_emit()`
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
pub use wide_log_macros::wide_log;

#[cfg(feature = "tokio")]
pub mod __re_exports {
    pub use tokio;
    pub use tower;
}
