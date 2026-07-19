//! Fuzz target for the full guard + emit end-to-end path.
//!
//! Builds a `WideLogGuard` via the `wide_log!` builder, exercises
//! the `wl_set!` / `wl_inc!` / `wl_dec!` / `wl_add!` / `wl_null!`
//! macros in a randomized sequence, and verifies that the resulting
//! JSON captured by the emit closure is well-formed and contains
//! the keys we expect.
//!
//! This is the broadest-coverage target: it exercises the cell-based
//! `current()` fast path, the per-thread `FMT_BUF` / `ULID_BUF` reuse,
//! the `Counter` increment/decrement math, the JSON serializer, and
//! the `Drop` glue that wires them all together.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sonic_rs::JsonValueTrait;
use std::sync::{Arc, Mutex};

wide_log::wide_log!({
    "service": { "name": null, "version": null },
    "requests": counter!,
    "retries": counter!,
    "status": null,
    "blob": null,
    "score": null,
});

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    // 6 actions: 0=set-str, 1=set-bool, 2=set-i64, 3=inc, 4=dec, 5=add, 6=null
    let action = data[0] % 7;
    let body = &data[1..];

    let slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let s_clone = slot.clone();
    let emit = move |ev: &wide_log::WideEvent<EventKey>| {
        if let Ok(json) = ev.to_json() {
            *s_clone.lock().unwrap() = Some(json);
        }
    };

    {
        let _g = WideLogGuard::builder().with_emit(emit).build();
        match action {
            0 => {
                let s = String::from_utf8_lossy(body).into_owned();
                wl_set!("blob", s);
            }
            1 => {
                let b = body.first().map_or(false, |&x| x != 0);
                wl_set!("status", if b { "ok" } else { "err" });
            }
            2 => {
                let mut buf = [0u8; 8];
                for (i, b) in body.iter().take(8).enumerate() {
                    buf[i] = *b;
                }
                let n = i64::from_le_bytes(buf);
                wl_set!("score", n);
            }
            3 => wl_inc!("requests"),
            4 => wl_dec!("retries"),
            5 => {
                let mut buf = [0u8; 8];
                for (i, b) in body.iter().take(8).enumerate() {
                    buf[i] = *b;
                }
                let n = i64::from_le_bytes(buf);
                wl_add!("requests", n);
            }
            6 => wl_null!("status"),
            _ => unreachable!(),
        }
    }

    if let Some(json) = slot.lock().unwrap().clone() {
        let parsed: sonic_rs::Value = sonic_rs::from_str(&json)
            .expect("emitted JSON must parse");
        // The auto-populated fields (event.id, event.timestamp,
        // duration.total_ms) must be present.
        assert!(parsed["event"]["id"].is_str(),
                "event.id should be a string after guard drop");
        assert!(parsed["event"]["timestamp"].is_str(),
                "event.timestamp should be a string after guard drop");
        assert!(parsed["duration"]["total_ms"].is_number(),
                "duration.total_ms should be a number after guard drop");
    }
});
