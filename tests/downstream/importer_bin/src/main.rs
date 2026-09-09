//! Schema-less binary crate: imports the level macros from the `wide-log`
//! crate root, never invokes `wide_log!`, and drives both libraries.
//!
//! Compiling and running this crate is the executable proof of the
//! cross-crate import surface: the crate-root macros must resolve in a
//! crate that never invoked `wide_log!` (they failed with E0432 before
//! the surface existed), and their appends must reach an event owned by
//! a different crate's schema.

use importer_lib::{log_all_levels_literal, log_format_args};
use sonic_rs::{JsonContainerTrait, JsonValueTrait};
use wide_log::{debug, error, info, trace, warn};

fn main() {
    // In tracing mode the capture layer must be in the subscriber stack
    // (the re-exported macros emit canonical tracing records; without the
    // layer they reach other layers but never the wide event). In
    // default-feature mode the crate-root macros append directly and no
    // subscriber is involved.
    #[cfg(feature = "tracing")]
    init_capture();

    no_guard_active_no_ops();
    guard_from_schema_lib_collects_entries_from_all_crates();
    nested_guards_innermost_hook_serves();
}

#[cfg(feature = "tracing")]
fn init_capture() {
    use tracing_subscriber::prelude::*;
    // Capture-only stack: the capture layer routes every canonical
    // tracing record (from any crate) into the active wide event, and no
    // formatting layer is installed, so the subscriber prints nothing.
    // The capture layer is generated at the schema crate's `wide_log!`
    // invocation site, so it resolves through `schema_lib`.
    tracing_subscriber::registry()
        .with(schema_lib::WideLogCaptureLayer::new())
        .init();
}

/// With no guard active, all five levels are silent no-ops that neither
/// panic nor emit.
fn no_guard_active_no_ops() {
    info!("bin info");
    warn!("bin warn");
    error!("bin error");
    debug!("bin debug");
    trace!("bin trace");
    log_all_levels_literal();
    log_format_args(7);
}

/// With a guard from `schema_lib` active, entries appended by all three
/// crates (the binary and the schema-less lib through the crate-root
/// macros; the schema lib through its generated macros) land in one
/// event's `log` array.
///
/// In tracing mode the entry text is the same because the capture layer
/// renders the message field verbatim; the guard's `with_emit` sink
/// collects the JSON without a formatting layer printing anything.
fn guard_from_schema_lib_collects_entries_from_all_crates() {
    let json = schema_lib::with_guard_emit_json(|| {
        schema_lib::count_request();
        info!("bin info inside guard");
        warn!("bin warn inside guard");
        error!("bin error inside guard");
        debug!("bin debug inside guard");
        trace!("bin trace inside guard");
        log_all_levels_literal();
        log_format_args(42);
        schema_lib::log_via_generated_macros("from schema lib");
    });

    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).expect("emitted line parses as JSON");

    let log = parsed["log"]
        .as_array()
        .unwrap_or_else(|| panic!("log array missing in {json}"));

    let messages: Vec<&str> = log.iter().filter_map(|e| e["message"].as_str()).collect();

    // Entries from all three crates are present.
    let expected = [
        // From the binary through the crate-root macros.
        "bin info inside guard",
        "bin warn inside guard",
        "bin error inside guard",
        "bin debug inside guard",
        "bin trace inside guard",
        // From the schema-less lib through the same import surface.
        "importer-lib info",
        "importer-lib warn",
        "importer-lib error",
        "importer-lib debug",
        "importer-lib trace",
        "importer-lib formatted: 42",
        // From the schema lib through its own generated macros.
        "schema-lib direct: from schema lib",
    ];
    for exp in expected {
        assert!(
            messages.contains(&exp),
            "missing entry {exp:?} in {messages:?}"
        );
    }

    assert_eq!(
        parsed["requests"].as_u64(),
        Some(1),
        "counter bumped once in {json}"
    );
    assert!(
        parsed["event"]["id"].as_str().is_some(),
        "event.id auto-populated in {json}"
    );
    assert!(
        parsed["duration"]["total_ms"].is_number(),
        "duration.total_ms auto-populated in {json}"
    );
}

/// Nested guards across crates: the innermost guard's hook serves
/// appends while it is active, and popping it on drop restores the outer
/// hook so the outer event resumes receiving entries.
fn nested_guards_innermost_hook_serves() {
    let outer_line = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let inner_line = std::sync::Arc::new(std::sync::Mutex::new(String::new()));

    let outer_sink = outer_line.clone();
    let inner_sink = inner_line.clone();

    {
        let outer_guard = schema_lib::WideLogGuard::builder()
            .with_emit(move |ev| {
                if let Ok(line) = ev.to_json() {
                    *outer_sink.lock().unwrap() = line;
                }
            })
            .build();

        {
            let inner_guard = schema_lib::WideLogGuard::builder()
                .with_emit(move |ev| {
                    if let Ok(line) = ev.to_json() {
                        *inner_sink.lock().unwrap() = line;
                    }
                })
                .build();

            // While both guards are active the innermost hook serves: this
            // entry must land in the inner event only.
            info!("entry while inner guard is active");
            drop(inner_guard);
        }

        // The outer hook is restored after the inner guard pops: this
        // entry must land in the outer event.
        info!("entry after inner guard dropped");
        drop(outer_guard);
    }

    let inner = inner_line.lock().unwrap().clone();
    let outer = outer_line.lock().unwrap().clone();

    let inner_parsed: sonic_rs::Value =
        sonic_rs::from_str(&inner).expect("inner line parses as JSON");
    let outer_parsed: sonic_rs::Value =
        sonic_rs::from_str(&outer).expect("outer line parses as JSON");

    let inner_messages: Vec<String> = inner_parsed["log"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e["message"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        inner_messages,
        vec!["entry while inner guard is active".to_string()],
        "inner event holds exactly the entry logged under it: {inner}"
    );

    let outer_messages: Vec<String> = outer_parsed["log"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e["message"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        outer_messages,
        vec!["entry after inner guard dropped".to_string()],
        "outer event resumes after the inner guard pops: {outer}"
    );
}
