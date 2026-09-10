//! Verifies the `pretty_print` example end to end. The example's
//! subscriber stack combines `WideLogCaptureLayer` (capture) with an
//! application-owned `PrettyPrintLayer`, and the guard uses the default
//! emit, so the finished event flows through the stack as the
//! reserved-target `wide_log` record and the custom layer prints it as
//! indented JSON.
//!
//! This test runs the example with `--features tracing` as a subprocess,
//! captures its stdout, reassembles the pretty-printed JSON block, and
//! asserts on the parsed structure.
//!
//! Feature-gated: the test target only builds with `--features tracing`,
//! because that is when the example target exists (its
//! `required-features = ["tracing"]`).

#![cfg(feature = "tracing")]

use std::process::Command;

use sonic_rs::{JsonContainerTrait, JsonValueTrait};

/// Helper: find the `cargo` binary, inheriting `CARGO` if set (used when
/// this test is itself run under cargo so it resolves the same toolchain).
fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// Run the `pretty_print` example (with `--features tracing`, because the
/// example target requires the feature), returning captured stdout and
/// exit code. Mirrors the subprocess pattern in `tests/stdout_emit.rs`.
fn run_pretty_print_example() -> (String, i32) {
    let output = Command::new(cargo_bin())
        .args([
            "run",
            "--example",
            "pretty_print",
            "--features",
            "tracing",
            "--quiet",
        ])
        .output()
        .expect("failed to spawn `cargo run --example pretty_print --features tracing`");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, code)
}

/// Pretty-printed JSON spans many lines. Reassemble the block that spans
/// from the first line equal to `{` through the final line equal to `}`,
/// and parse it. Build-progress lines from the nested `cargo run` are
/// skipped because the scan starts at the opening-brace line.
fn first_pretty_block(stdout: &str) -> sonic_rs::Value {
    let lines: Vec<&str> = stdout.lines().collect();
    let start = lines
        .iter()
        .position(|line| *line == "{")
        .expect("no line equal to `{` in example stdout:\n{stdout}");
    let end = lines
        .iter()
        .rposition(|line| *line == "}")
        .expect("no line equal to `}` in example stdout:\n{stdout}");
    assert!(end > start, "empty JSON block in example stdout:\n{stdout}");
    let block = &lines[start..=end];
    sonic_rs::from_str(&block.join("\n")).expect("reassembled pretty block is not valid JSON")
}

#[test]
fn pretty_print_example_prints_indented_wide_event() {
    let (stdout, code) = run_pretty_print_example();
    assert_eq!(code, 0, "example exited non-zero; stdout:\n{stdout}");

    // The output is pretty-printed, not a compact single line: at least
    // one stdout line starts with two spaces followed by a JSON key.
    assert!(
        stdout.lines().any(|l| l.starts_with("  \"")),
        "no indented line found; output is not pretty-printed:\n{stdout}"
    );

    let parsed = first_pretty_block(&stdout);
    assert!(
        parsed.is_object(),
        "reassembled block is not a JSON object:\n{stdout}"
    );

    // Fields from examples/pretty_print.rs.
    assert_eq!(parsed["service"]["name"], "pretty-print-example");
    assert_eq!(parsed["service"]["version"], "1.0.0");
    assert_eq!(parsed["requests"], 1);

    // The log entries the example emits, captured by
    // `WideLogCaptureLayer` into the event.
    let log = parsed["log"].as_array().expect("log array missing");
    assert_eq!(log.len(), 2, "expected 2 log entries, got {log:?}");
    assert_eq!(log[0]["level"], "info");
    assert_eq!(log[0]["message"], "request received");
    assert_eq!(log[1]["level"], "warn");
    assert_eq!(log[1]["message"], "upstream slow");

    // Auto-added event metadata.
    let timestamp = parsed["event"]["timestamp"].as_str().unwrap_or("");
    assert!(!timestamp.is_empty(), "event.timestamp is empty");
    assert!(
        timestamp.contains('T'),
        "timestamp not RFC 3339: {timestamp}"
    );
    let id = parsed["event"]["id"].as_str().unwrap_or("");
    assert!(!id.is_empty(), "event.id is empty");

    // Auto-added duration.
    assert!(
        parsed["duration"]["total_ms"].is_number(),
        "duration.total_ms missing or not a number"
    );
}
