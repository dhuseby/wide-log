//! Schema-less library crate: imports all five level macros from the
//! `wide-log` crate root and never invokes `wide_log!`.
//!
//! Compiling this crate at all is the regression test for the pre-change
//! `E0432: no 'info' in the root` failure — the level macros exist only as
//! `#[macro_export]` rules inside crates that invoked `wide_log!` before
//! the crate-root surface was added.

use wide_log::{debug, error, info, trace, warn};

/// Appends one entry per level (literal form) to the active event.
pub fn log_all_levels_literal() {
    info!("importer-lib info");
    warn!("importer-lib warn");
    error!("importer-lib error");
    debug!("importer-lib debug");
    trace!("importer-lib trace");
}

/// Appends an entry through the format-arg form. The format buffer lives
/// in the `wide-log` crate, so this also proves the crate-owned buffer
/// path works from a dependent crate.
pub fn log_format_args(value: u64) {
    info!("importer-lib formatted: {}", value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_macros_are_silent_no_ops_without_a_guard() {
        // No wide-log guard is active in this test: all five levels and
        // the format-arg variant must be silent no-ops that neither panic
        // nor emit.
        log_all_levels_literal();
        log_format_args(1);
    }

    #[test]
    fn macro_paths_resolve_through_the_wide_log_crate_root() {
        // Path-call spelling (`wide_log::info!`) is the other half of the
        // import surface: `#[macro_export]` puts the rules at the
        // `wide-log` root, so both spellings must resolve.
        wide_log::info!("path-called info");
        wide_log::warn!("path-called warn");
        wide_log::error!("path-called error");
        wide_log::debug!("path-called debug");
        wide_log::trace!("path-called trace");
    }
}
