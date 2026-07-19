//! Fuzz target for `Value::from_*` conversions.
//!
//! Exercises every `From` impl on `Value<K>`:
//! - `From<bool> for Value<K>`
//! - `From<i64> for Value<K>`
//! - `From<u64> for Value<K>`
//! - `From<f64> for Value<K>`
//! - `From<&str> for Value<K>`
//! - `From<String> for Value<K>`
//! - `From<FastStr> for Value<K>`
//! - `From<()> for Value<K>` (null)
//!
//! Strategy: the first byte picks the `From` variant; the rest of the
//! input is interpreted as the source value. For strings, the bytes
//! are taken as-is (lossy UTF-8 conversion). For numbers, the bytes
//! are interpreted as the numeric type's `from_le_bytes` (so the fuzzer
//! can drive every representable value of `i64` / `u64` / `f64`).
//!
//! Invariant: every `Value` constructed this way must serialize to
//! valid JSON via the `wide_log!` macro API.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sonic_rs::JsonContainerTrait;
use std::sync::{Arc, Mutex};

wide_log::wide_log!({
    "service": { "name": null },
    "requests": counter!,
    "status": null,
    "flag": null,
    "count": null,
    "ratio": null,
    "label": null,
    "blob": null,
    "none": null,
});

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    // 9 variants: 0=bool, 1=i64, 2=u64, 3=f64, 4=&str, 5=String,
    //             6=FastStr, 7=null, 8=past-end (no-op)
    let variant = data[0] % 9;
    let body = &data[1..];

    let slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let s_clone = slot.clone();
    let emit = move |ev: &wide_log::WideEvent<EventKey>| {
        if let Ok(json) = ev.to_json() {
            *s_clone.lock().unwrap() = Some(json);
        }
    };

    match variant {
        0 => {
            // From<bool>: any nonzero byte → true, zero → false.
            let b = body.first().map_or(false, |&x| x != 0);
            let _g = WideLogGuard::builder().with_emit(emit).build();
            wl_set!("flag", b);
        }
        1 => {
            // From<i64>: 8 bytes, little-endian.
            let mut buf = [0u8; 8];
            for (i, b) in body.iter().take(8).enumerate() {
                buf[i] = *b;
            }
            let n = i64::from_le_bytes(buf);
            let _g = WideLogGuard::builder().with_emit(emit).build();
            wl_set!("count", n);
        }
        2 => {
            // From<u64>: 8 bytes, little-endian.
            let mut buf = [0u8; 8];
            for (i, b) in body.iter().take(8).enumerate() {
                buf[i] = *b;
            }
            let n = u64::from_le_bytes(buf);
            let _g = WideLogGuard::builder().with_emit(emit).build();
            wl_set!("count", n);
        }
        3 => {
            // From<f64>: 8 bytes, little-endian.
            let mut buf = [0u8; 8];
            for (i, b) in body.iter().take(8).enumerate() {
                buf[i] = *b;
            }
            let f = f64::from_le_bytes(buf);
            let _g = WideLogGuard::builder().with_emit(emit).build();
            wl_set!("ratio", f);
        }
        4 => {
            // From<&str>: borrow a String to get a &str.
            let s = String::from_utf8_lossy(body).into_owned();
            let _g = WideLogGuard::builder().with_emit(emit).build();
            wl_set!("label", s.as_str());
        }
        5 => {
            // From<String>.
            let s = String::from_utf8_lossy(body).into_owned();
            let _g = WideLogGuard::builder().with_emit(emit).build();
            wl_set!("label", s);
        }
        6 => {
            // From<FastStr>.
            let s = String::from_utf8_lossy(body).into_owned();
            let fs = faststr::FastStr::new(s);
            let _g = WideLogGuard::builder().with_emit(emit).build();
            wl_set!("blob", fs);
        }
        7 => {
            // From<()>: null.
            let _g = WideLogGuard::builder().with_emit(emit).build();
            wl_null!("none");
        }
        _ => return,
    }

    if let Some(json) = slot.lock().unwrap().clone() {
        // The emitted JSON must parse.
        let parsed: sonic_rs::Value = sonic_rs::from_str(&json)
            .expect("emitted JSON must parse");
        // The set field must appear in the JSON. The value may be
        // `null` for non-finite floats (NaN / ±Inf): JSON has no
        // representation for those, and the `to_json` path
        // (sonic-rs) and the direct `serialize_to` path both
        // emit `null` for them. So the correct invariant is
        // "key is present in the object", not "key is non-null".
        let key = match variant {
            0 => "flag",
            1 | 2 => "count",
            3 => "ratio",
            4 | 5 => "label",
            6 => "blob",
            7 => "none",
            _ => return,
        };
        let obj = parsed.as_object().expect("event must serialize as a JSON object");
        assert!(obj.contains_key(&key),
                "expected field {key:?} to be present after set; json was {json}");
    }
});
