//! Regression test for the 0.6.0 cross-module `FMT_BUF` visibility bug.
//!
//! The `wide_log!` macro emits a `thread_local!` block at its invocation
//! site. In 0.6.0 those thread-locals (`FMT_BUF`, `ULID_BUF`, `EMIT_BUF`,
//! `CURRENT_EVENT`) were emitted as private `static`s, so the
//! `#[macro_export]`-ed format-arg branches of `info!`/`warn!`/
//! `error!`/`debug!`/`trace!` — which reference `FMT_BUF.with(|buf| ...)`
//! by bare name — could not resolve `FMT_BUF` when called from a module
//! other than the one that invoked `wide_log!`. This caused 15+
//! `E0425: cannot find value 'FMT_BUF' in this scope` errors in any
//! downstream crate that used the format-arg variants of those macros
//! from sub-modules.
//!
//! 0.6.1 makes the thread-locals `pub(crate)` so the format-arg
//! branches resolve from any module in the same crate. This test
//! exercises the fix: it invokes `wide_log!` at the crate root, then
//! calls `info!("fmt {}", x)`, `warn!`, `error!`, `debug!`, `trace!`
//! from a child `mod child;` and asserts the formatted strings appear
//! in the emitted JSON's `log` array.

use sonic_rs::{JsonContainerTrait, JsonValueTrait};
use std::sync::{Arc, Mutex};
use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
    },
    "requests": counter!,
    "status": null,
});

// The `#[macro_export]`-ed log macros are at the crate root, not in
// `wide_log::*`. The 0.6.0 "FMT_BUF" bug surfaced in sub-modules
// (anywhere other than the module that invoked `wide_log!`), so this
// test only needs to import the macros at the root and then call them
// from `mod child`.

type CaptureSlot = Arc<Mutex<Option<String>>>;

fn capture() -> (CaptureSlot, impl FnOnce(&wide_log::WideEvent<EventKey>) + Send + 'static) {
    let slot: CaptureSlot = Arc::new(Mutex::new(None));
    let s = slot.clone();
    let emit = move |ev: &wide_log::WideEvent<EventKey>| {
        *s.lock().unwrap() = Some(ev.to_json().unwrap());
    };
    (slot, emit)
}

fn parse(slot: &CaptureSlot) -> sonic_rs::Value {
    let json = slot.lock().unwrap().clone().unwrap();
    sonic_rs::from_str(&json).unwrap()
}

// A child module of the integration test. The `wide_log!` invocation
// lives at the crate root above. Before 0.6.1's `$crate::` qualified
// macro references (and `pub(crate)` thread-locals), the format-arg
// macros would fail to compile here with E0425 because `FMT_BUF`
// was private to the root module and the macros referenced it by
// bare name.
mod child {
    use super::EventKey;

    pub fn emit_all_with_format_args() -> String {
        let _guard = super::WideLogGuard::builder().build();
        // Literal string variant (no format args) — always worked.
        info!("literal-only from child");
        // Format-arg variants — failed to compile in 0.6.0 from
        // any module other than the one that invoked `wide_log!`.
        let name = "qb2";
        let n = 42u64;
        info!("info fmt from child: name={} n={}", name, n);
        warn!("warn fmt from child: name={} n={}", name, n);
        debug!("debug fmt from child: name={} n={}", name, n);
        trace!("trace fmt from child: name={} n={}", name, n);
        error!("error fmt from child: name={} n={}", name, n);
        // We don't capture the guard's output here; the parent test
        // uses its own guard and a custom emit to inspect the JSON.
        // The sole purpose of this function is to verify that the
        // format-arg macros COMPILE from a child module.
        String::from("ok")
    }

    pub fn emit_into_guard_with_format_args(
        emit: impl FnOnce(&wide_log::WideEvent<EventKey>) + Send + 'static,
    ) -> String {
        let _guard = super::WideLogGuard::builder().with_emit(emit).build();
        let name = "qb2";
        let n = 42u64;
        info!("from child info: name={} n={}", name, n);
        warn!("from child warn: name={} n={}", name, n);
        String::from("ok")
    }
}

#[test]
fn format_arg_macros_compile_from_child_module() {
    // The mere fact that this test compiles proves the fix: before
    // 0.6.1, `child::emit_all_with_format_args` would have failed
    // with `E0425: cannot find value 'FMT_BUF' in this scope`. The
    // call below would be a compile error.
    let result = child::emit_all_with_format_args();
    assert_eq!(result, "ok");
}

#[test]
fn format_arg_macros_emit_formatted_strings_from_child_module() {
    // Stronger test: the format-arg branches in the child module
    // must not only compile, they must also format the args
    // correctly and append the result to the `log` array.
    let (slot, emit) = capture();
    child::emit_into_guard_with_format_args(emit);
    let json = parse(&slot);

    // The `log` array carries the entries appended by `info!` and
    // `warn!` from the child module. Assert each formatted string
    // is present, with the format-arg values interpolated.
    let log = json["log"].as_array().expect("log is an array");
    let mut found_info = false;
    let mut found_warn = false;
    for entry in log {
        let msg = entry["message"].as_str().expect("entry.message is str");
        if msg == "from child info: name=qb2 n=42" {
            found_info = true;
            assert_eq!(entry["level"].as_str(), Some("info"));
        } else if msg == "from child warn: name=qb2 n=42" {
            found_warn = true;
            assert_eq!(entry["level"].as_str(), Some("warn"));
        }
    }
    assert!(found_info, "missing formatted info entry: {json}");
    assert!(found_warn, "missing formatted warn entry: {json}");
}
