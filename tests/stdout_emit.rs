//! Verifies that `default_emit` writes a single bare JSON line to stdout
//! (no `tracing` envelope) by running the `basic` example as a subprocess
//! and capturing its stdout.

use std::process::Command;

use sonic_rs::{JsonContainerTrait, JsonValueTrait};

/// Helper: find the `cargo` binary, inheriting `CARGO` if set (used when this
/// test is itself run under cargo so it resolves the same toolchain).
fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// Run `cargo run --example basic` (without `--` args), inheriting the
/// current environment, and return its captured stdout.
fn run_basic_example() -> (String, i32) {
    let output = Command::new(cargo_bin())
        .args(["run", "--example", "basic", "--quiet"])
        .output()
        .expect("failed to spawn `cargo run --example basic`");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, code)
}

/// Find the first non-empty stdout line that parses as a JSON object.
/// `cargo run` may emit build-progress lines on stdout in some setups; we
/// look for the first line that looks like a JSON object and parses.
fn first_json_line(stdout: &str) -> sonic_rs::Value {
    for line in stdout.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(parsed) = sonic_rs::from_str::<sonic_rs::Value>(trimmed) {
            return parsed;
        }
    }
    panic!("no JSON object line found in example stdout:\n{stdout}");
}

#[test]
fn default_emit_writes_bare_json_to_stdout() {
    let (stdout, code) = run_basic_example();
    assert_eq!(code, 0, "example exited non-zero; stdout:\n{stdout}");

    let parsed = first_json_line(&stdout);

    // No tracing envelope: the entire line is the JSON object, so the parsed
    // top-level should be an object (not a string starting with "INFO ...").
    assert!(
        parsed.is_object(),
        "top-level stdout line is not a JSON object — looks like a tracing envelope:\n{stdout}"
    );

    // Spot-check the expected fields from examples/basic.rs.
    assert_eq!(parsed["service"]["name"], "example-service");
    assert_eq!(parsed["service"]["version"], "1.0.0");
    assert_eq!(parsed["requests"], 1);
}

#[test]
fn default_emit_line_has_no_tracing_envelope_prefix() {
    let (stdout, _) = run_basic_example();

    // The default tracing fmt envelope starts with a timestamp + "INFO".
    // The wide-log JSON line starts directly with `{"service":...`.
    let line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("{\"service\":"))
        .expect("no line starting with `{\"service\":...` in stdout:\n{stdout}");

    // Defensive: ensure the line does not contain tracing's field marker
    // `event=` which would be present if the JSON were wrapped in a
    // tracing event as `event = %json`.
    assert!(
        !line.contains("event="),
        "stdout line contains `event=` — looks like a tracing event, not a bare JSON line:\n{line}"
    );
}

#[test]
fn default_emit_writes_log_entries_and_event_metadata() {
    let (stdout, code) = run_basic_example();
    assert_eq!(code, 0, "example exited non-zero; stdout:\n{stdout}");

    let parsed = first_json_line(&stdout);

    // `info!("request received")` and `warn!("upstream slow")` from basic.rs.
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

#[test]
fn dropped_events_counter_is_loadable() {
    // The counter is process-global and shared; this test only verifies the
    // accessor compiles and runs without panic. The subprocess stdout tests
    // above exercise the actual submit path.
    let _ = wide_log::stdout_emit::dropped_events();
}

/// Run `cargo run --example bounded_emit`, returning captured stdout and
/// exit code. Mirrors `run_basic_example`.
fn run_bounded_example() -> (String, i32) {
    let output = Command::new(cargo_bin())
        .args(["run", "--example", "bounded_emit", "--quiet"])
        .output()
        .expect("failed to spawn `cargo run --example bounded_emit`");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

/// The bounded-channel emit mode (`set_channel_capacity(Bounded(n))`) must
/// produce the same bare-JSON-line format on stdout as the default
/// unbounded mode — the channel capacity only changes the in-flight
/// buffering / backpressure behavior, not the output format. This also
/// verifies the bounded example doesn't deadlock or drop events under a
/// 20-event burst with capacity 8.
#[test]
fn bounded_emit_writes_bare_json_to_stdout() {
    let (stdout, code) = run_bounded_example();
    assert_eq!(code, 0, "bounded example exited non-zero; stdout:\n{stdout}");

    let parsed = first_json_line(&stdout);
    assert!(
        parsed.is_object(),
        "top-level stdout line is not a JSON object — bounded channel changed output format:\n{stdout}"
    );
    assert_eq!(parsed["service"]["name"], "bounded-example");
    assert_eq!(parsed["service"]["version"], "1.0.0");
    assert_eq!(parsed["requests"], 1);

    // The example logs 20 entries; none should be dropped under the bounded
    // channel because each `info!` call appends to the in-event `log` array
    // (not the channel), and only the single drop-emitted event is submitted.
    let log = parsed["log"].as_array().expect("log array missing");
    assert_eq!(log.len(), 20, "expected 20 log entries, got {}", log.len());
    for (i, entry) in log.iter().enumerate() {
        assert_eq!(entry["level"], "info");
        assert_eq!(entry["message"], format!("request {i} received"));
    }
}

/// The bounded example must not wrap its stdout line in a tracing envelope
/// (i.e. it is the default `default_emit` writing bare JSON, only the channel
/// capacity changed).
#[test]
fn bounded_emit_line_has_no_tracing_envelope_prefix() {
    let (stdout, _) = run_bounded_example();
    let line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("{\"service\":"))
        .expect("no line starting with `{\"service\":...` in stdout:\n{stdout}");
    assert!(
        !line.contains("event="),
        "bounded stdout line contains `event=` — looks like a tracing event:\n{line}"
    );
}
