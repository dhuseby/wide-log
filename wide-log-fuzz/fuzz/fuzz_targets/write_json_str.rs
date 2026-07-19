//! Fuzz target for the JSON serializer (write_json_str / to_json / serialize_to).
//!
//! `write_json_str` is private; the public surface that exercises it is
//! `WideEvent::to_json` (which calls `serialize_to` → `write_event` →
//! `write_json_str` for every string leaf).
//!
//! Strategy: build a `WideEvent` via the `wide_log!` macro API, set a
//! string field to the fuzzer-provided bytes, drop the guard, capture
//! the emitted JSON, and verify that:
//!
//! 1. `to_json()` does not panic on any input.
//! 2. The output is valid JSON (round-trips through `sonic_rs::from_str`).
//! 3. The string leaf round-trips: the string we set is character-equal
//!    to the string we get back from parsing the JSON output.
//!
//! U+2028 / U+2029 are the trickiest inputs (Phase 1 §4.1 added explicit
//! escaping for them); the fuzzer will explore that path naturally.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sonic_rs::JsonValueTrait;
use std::sync::{Arc, Mutex};

wide_log::wide_log!({
    "service": { "name": null, "version": null },
    "requests": counter!,
    "status": null,
    "blob": null,
    "tag": null,
});

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    // Treat the input as a UTF-8 string (lossy: invalid UTF-8 is replaced
    // with U+FFFD). This is exactly what a JSON string body is allowed
    // to contain — the serializer must produce valid escapes for the
    // control characters and the U+2028 / U+2029 line terminators.
    let s = String::from_utf8_lossy(data).into_owned();

    let slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let s_clone = slot.clone();
    let emit = move |ev: &wide_log::WideEvent<EventKey>| {
        if let Ok(json) = ev.to_json() {
            *s_clone.lock().unwrap() = Some(json);
        }
    };

    {
        let _g = WideLogGuard::builder()
            .with_emit(emit)
            .build();
        wl_set!("blob", s.as_str());
        wl_set!("tag", s.clone());
    }

    // The emit closure was called when the guard dropped. The JSON
    // should round-trip through sonic_rs without panic.
    if let Some(json) = slot.lock().unwrap().clone() {
        let parsed: sonic_rs::Value = sonic_rs::from_str(&json)
            .expect("emitted JSON must parse");
        // Both string leaves must round-trip. Use lossy comparison: the
        // JSON-escaped form is decoded back to the same characters by
        // sonic_rs, so a strict string equality check is valid here.
        let blob_str = parsed["blob"]
            .as_str()
            .expect("blob field should be a string");
        let tag_str = parsed["tag"]
            .as_str()
            .expect("tag field should be a string");
        assert_eq!(blob_str, s, "blob string round-trip mismatch");
        assert_eq!(tag_str, s, "tag string round-trip mismatch");
    }
});
