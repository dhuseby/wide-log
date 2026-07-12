use std::sync::Arc;

use faststr::FastStr;
use smallvec::SmallVec;
use serde::ser::{SerializeMap, Serialize, Serializer};

use crate::error::Error;
use crate::key::Key;
use crate::log::LogEntry;
use crate::value::Value;

type ConflictCb<K> = Arc<dyn Fn(&mut WideEvent<K>, K) + Send + Sync>;

#[derive(Clone)]
pub struct WideEvent<K: Key> {
    pub(crate) entries: SmallVec<[(K, Value<K>); 24]>,
    pub(crate) log_entries: SmallVec<[LogEntry; 8]>,
    pub(crate) on_type_conflict: Option<ConflictCb<K>>,
}

impl<K: Key + std::fmt::Debug> std::fmt::Debug for WideEvent<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WideEvent")
            .field("entries", &self.entries)
            .field("log_entries_len", &self.log_entries.len())
            .field("has_conflict_callback", &self.on_type_conflict.is_some())
            .finish()
    }
}

impl<K: Key> Serialize for WideEvent<K> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let total = self.entries.len() + if self.log_entries.is_empty() { 0 } else { 1 };
        let mut map = serializer.serialize_map(Some(total))?;
        for (key, value) in &self.entries {
            map.serialize_entry(key.as_str(), value)?;
        }
        if !self.log_entries.is_empty() {
            map.serialize_entry("log", &self.log_entries)?;
        }
        map.end()
    }
}

impl<K: Key> WideEvent<K> {
    #[inline]
    pub fn new() -> Self {
        Self {
            entries: SmallVec::new(),
            log_entries: SmallVec::new(),
            on_type_conflict: None,
        }
    }

    pub fn new_with_warnings<F: Fn(&mut WideEvent<K>, K) + Send + Sync + 'static>(f: F) -> Self {
        Self {
            entries: SmallVec::new(),
            log_entries: SmallVec::new(),
            on_type_conflict: Some(Arc::new(f)),
        }
    }

    pub(crate) fn with_callback(cb: Option<ConflictCb<K>>) -> Self {
        Self {
            entries: SmallVec::new(),
            log_entries: SmallVec::new(),
            on_type_conflict: cb,
        }
    }

    #[inline]
    pub fn add<V: Into<Value<K>>>(&mut self, key: K, value: V) {
        let value = value.into();
        for (k, v) in &mut self.entries {
            if *k == key {
                *v = value;
                return;
            }
        }
        self.entries.push((key, value));
    }

    pub fn object(&mut self, key: K) -> &mut WideEvent<K> {
        let pos = self.entries.iter().position(|(k, _)| *k == key);
        if let Some(i) = pos {
            let is_object = matches!(self.entries[i].1, Value::Object(_));
            if !is_object {
                let cb_opt = self.on_type_conflict.take();
                if let Some(ref arc_cb) = cb_opt {
                    arc_cb(self, key);
                }
                self.on_type_conflict = cb_opt;
                self.entries[i].1 = Value::Object(Box::new(
                    WideEvent::with_callback(self.on_type_conflict.clone()),
                ));
            }
            if let Value::Object(ref mut child) = self.entries[i].1 {
                return child;
            }
            unreachable!()
        } else {
            self.entries.push((
                key,
                Value::Object(Box::new(WideEvent::with_callback(
                    self.on_type_conflict.clone(),
                ))),
            ));
            let last = self.entries.len() - 1;
            if let Value::Object(ref mut child) = self.entries[last].1 {
                child
            } else {
                unreachable!()
            }
        }
    }

    /// Set a value at a nested path.
    /// Traverses/creates intermediate objects as needed.
    #[inline]
    pub fn add_path<V: Into<Value<K>>>(&mut self, path: &[K], value: V) {
        debug_assert!(!path.is_empty(), "path must have at least one segment");
        let value = value.into();
        if path.len() == 1 {
            self.add(path[0], value);
            return;
        }
        let target = self.descend_mut(path);
        target.add(path[path.len() - 1], value);
    }

    /// Get or create the nested object at `path[..len-1]`, return `&mut` to it.
    #[inline]
    fn descend_mut(&mut self, path: &[K]) -> &mut WideEvent<K> {
        let mut current: &mut WideEvent<K> = self;
        for &seg in &path[..path.len() - 1] {
            current = current.object(seg);
        }
        current
    }

    /// Increment a numeric field by 1. Initializes to 1 if absent.
    #[inline]
    pub fn inc(&mut self, key: K) {
        for (k, v) in &mut self.entries {
            if *k == key {
                *v = match v {
                    Value::U64(n) => Value::U64(*n + 1),
                    Value::I64(n) => Value::I64(*n + 1),
                    _ => Value::U64(1),
                };
                return;
            }
        }
        self.entries.push((key, Value::U64(1)));
    }

    /// Decrement a numeric field by 1. Initializes to -1 if absent.
    #[inline]
    pub fn dec(&mut self, key: K) {
        for (k, v) in &mut self.entries {
            if *k == key {
                *v = match v {
                    Value::U64(n) if *n > 0 => Value::U64(*n - 1),
                    Value::I64(n) => Value::I64(*n - 1),
                    _ => Value::I64(-1),
                };
                return;
            }
        }
        self.entries.push((key, Value::I64(-1)));
    }

    /// Add a number to a numeric field. Initializes to `n` if absent.
    #[inline]
    pub fn add_n(&mut self, key: K, n: i64) {
        for (k, v) in &mut self.entries {
            if *k == key {
                *v = match v {
                    Value::U64(x) => Value::U64(x.saturating_add_signed(n)),
                    Value::I64(x) => Value::I64(*x + n),
                    _ => Value::I64(n),
                };
                return;
            }
        }
        self.entries.push((key, if n >= 0 { Value::U64(n as u64) } else { Value::I64(n) }));
    }

    /// Increment a numeric field at a nested path by 1.
    #[inline]
    pub fn inc_path(&mut self, path: &[K]) {
        debug_assert!(!path.is_empty(), "path must have at least one segment");
        if path.len() == 1 {
            self.inc(path[0]);
            return;
        }
        let target = self.descend_mut(path);
        target.inc(path[path.len() - 1]);
    }

    /// Decrement a numeric field at a nested path by 1.
    #[inline]
    pub fn dec_path(&mut self, path: &[K]) {
        debug_assert!(!path.is_empty(), "path must have at least one segment");
        if path.len() == 1 {
            self.dec(path[0]);
            return;
        }
        let target = self.descend_mut(path);
        target.dec(path[path.len() - 1]);
    }

    /// Add a number to a numeric field at a nested path.
    #[inline]
    pub fn add_n_path(&mut self, path: &[K], n: i64) {
        debug_assert!(!path.is_empty(), "path must have at least one segment");
        if path.len() == 1 {
            self.add_n(path[0], n);
            return;
        }
        let target = self.descend_mut(path);
        target.add_n(path[path.len() - 1], n);
    }

    /// Append a log entry to the log list.
    #[inline]
    pub fn append_log_entry(&mut self, level: &'static str, message: &str) {
        self.log_entries.push(LogEntry {
            level,
            message: FastStr::new(message),
        });
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Number of log entries accumulated.
    #[inline]
    pub fn log_len(&self) -> usize {
        self.log_entries.len()
    }

    /// Whether any log entries have been accumulated.
    #[inline]
    pub fn has_logs(&self) -> bool {
        !self.log_entries.is_empty()
    }

    pub fn to_json(&self) -> Result<String, Error> {
        sonic_rs::to_string(self).map_err(|e| Error::Serialize(e.to_string()))
    }
}

impl<K: Key> Default for WideEvent<K> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::key::test_support::TestKey;

    // 24-variant key used to verify the inline SmallVec capacity.
    #[derive(Copy, Clone, PartialEq, Eq, Debug)]
    #[repr(u8)]
    enum BigKey {
        Duration,
        TotalMs,
        K2,  K3,  K4,  K5,  K6,  K7,  K8,  K9,
        K10, K11, K12, K13, K14, K15, K16,
        K17, K18, K19, K20, K21, K22, K23,
    }

    impl crate::key::Key for BigKey {
        fn as_str(self) -> &'static str {
            match self {
                BigKey::Duration => "duration",
                BigKey::TotalMs  => "total_ms",
                BigKey::K2  => "k2",  BigKey::K3  => "k3",
                BigKey::K4  => "k4",  BigKey::K5  => "k5",
                BigKey::K6  => "k6",  BigKey::K7  => "k7",
                BigKey::K8  => "k8",  BigKey::K9  => "k9",
                BigKey::K10 => "k10", BigKey::K11 => "k11",
                BigKey::K12 => "k12", BigKey::K13 => "k13",
                BigKey::K14 => "k14", BigKey::K15 => "k15",
                BigKey::K16 => "k16", BigKey::K17 => "k17",
                BigKey::K18 => "k18", BigKey::K19 => "k19",
                BigKey::K20 => "k20", BigKey::K21 => "k21",
                BigKey::K22 => "k22", BigKey::K23 => "k23",
            }
        }
        const MAX_KEYS: usize = 24;
        fn as_index(self) -> usize { self as usize }
        const DURATION_PATH: &'static [Self] = &[BigKey::Duration, BigKey::TotalMs];
    }

    #[test]
    fn new_is_empty() {
        let e = WideEvent::<TestKey>::new();
        assert!(e.is_empty());
        assert_eq!(e.len(), 0);
        assert_eq!(e.log_len(), 0);
        assert!(!e.has_logs());
    }

    #[test]
    fn default_is_empty() {
        let e = WideEvent::<TestKey>::default();
        assert!(e.is_empty());
    }

    #[test]
    fn add_single() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Status, "ok");
        assert_eq!(e.len(), 1);
    }

    #[test]
    fn add_updates_existing() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Status, "ok");
        e.add(TestKey::Status, "err");
        assert_eq!(e.len(), 1);
        assert_eq!(e.to_json().unwrap(), r#"{"status":"err"}"#);
    }

    #[test]
    fn add_multiple_keys() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Requests, 42u64);
        e.add(TestKey::Status,   "ok");
        e.add(TestKey::Tag,      "web");
        assert_eq!(e.len(), 3);
    }

    #[test]
    fn object_creates_nested() {
        let mut e = WideEvent::<TestKey>::new();
        e.object(TestKey::Details).add(TestKey::Status, "ok");
        let json = e.to_json().unwrap();
        assert!(json.contains("\"details\""));
        assert!(json.contains("\"status\""));
    }

    #[test]
    fn object_returns_existing() {
        let mut e = WideEvent::<TestKey>::new();
        e.object(TestKey::Details).add(TestKey::Requests, 1u64);
        e.object(TestKey::Details).add(TestKey::Status, "ok");
        let json = e.to_json().unwrap();
        assert!(json.contains("\"requests\""));
        assert!(json.contains("\"status\""));
        assert_eq!(e.len(), 1, "only one top-level entry");
    }

    #[test]
    fn object_type_conflict_fires_callback() {
        let counter = Arc::new(Mutex::new(0u32));
        let c = counter.clone();
        let mut e = WideEvent::new_with_warnings(move |_event, _key| {
            *c.lock().unwrap() += 1;
        });
        e.add(TestKey::Details, true);
        e.object(TestKey::Details);
        assert_eq!(*counter.lock().unwrap(), 1);
    }

    #[test]
    fn object_type_conflict_callback_appends_warning_string() {
        use smallvec::smallvec;
        use crate::value::Value;

        let mut e = WideEvent::new_with_warnings(|event, key: TestKey| {
            let warning = format!("{} type conflict", key.as_str());
            let entry = Box::new(Value::from(warning));
            for (k, v) in &mut event.entries {
                if *k == TestKey::Tag
                    && let Value::Array(arr) = v {
                    arr.push(entry);
                    return;
                }
            }
            event.entries.push((TestKey::Tag, Value::Array(smallvec![entry])));
        });
        e.add(TestKey::Details, true);
        e.object(TestKey::Details);
        let json = e.to_json().unwrap();
        assert!(json.contains("\"details type conflict\""), "warning string should appear in JSON");
    }

    #[test]
    fn object_type_conflict_callback_can_mutate_event() {
        let mut e = WideEvent::new_with_warnings(|event, _key| {
            event.add(TestKey::Flag, true);
        });
        e.add(TestKey::Details, 42u64);
        e.object(TestKey::Details).add(TestKey::Status, "ok");
        let json = e.to_json().unwrap();
        assert!(json.contains("\"flag\":true"), "callback mutation should appear in event");
        assert!(json.contains("\"details\""), "conflicted key should become an object");
    }

    #[test]
    fn object_type_conflict_replaces_with_object() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Details, true);
        e.object(TestKey::Details).add(TestKey::Status, "ok");
        assert!(matches!(e.entries[0].1, Value::Object(_)));
    }

    #[test]
    fn to_json_valid() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Requests, 7u64);
        e.add(TestKey::Status,   "ok");
        e.add(TestKey::Flag,     false);
        let json = e.to_json().unwrap();
        assert!(json.starts_with('{'));
        assert!(json.ends_with('}'));
    }

    #[test]
    fn to_json_roundtrip() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Requests, 42u64);
        e.add(TestKey::Status,   "active");
        let json = e.to_json().unwrap();
        let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        assert_eq!(parsed["requests"], 42u64);
        assert_eq!(parsed["status"], "active");
    }

    #[test]
    fn serialize_nested_object() {
        let mut e = WideEvent::<TestKey>::new();
        e.object(TestKey::Service).add(TestKey::Name, "svc");
        assert_eq!(e.to_json().unwrap(), r#"{"service":{"name":"svc"}}"#);
    }

    #[test]
    fn inline_capacity_not_spilled() {
        let keys = [
            BigKey::Duration, BigKey::TotalMs, BigKey::K2,  BigKey::K3,
            BigKey::K4,  BigKey::K5,  BigKey::K6,  BigKey::K7,
            BigKey::K8,  BigKey::K9,  BigKey::K10, BigKey::K11,
            BigKey::K12, BigKey::K13, BigKey::K14, BigKey::K15,
            BigKey::K16, BigKey::K17, BigKey::K18, BigKey::K19,
            BigKey::K20, BigKey::K21, BigKey::K22, BigKey::K23,
        ];
        let mut e = WideEvent::<BigKey>::new();
        for (i, &k) in keys.iter().enumerate() {
            e.add(k, i as u64);
        }
        assert_eq!(e.len(), 24);
        assert!(!e.entries.spilled(), "24 entries must fit in inline storage");
    }

    // ---- Path method tests ----

    #[test]
    fn add_path_single_segment() {
        let mut e = WideEvent::<TestKey>::new();
        e.add_path(&[TestKey::Status], "ok");
        assert_eq!(e.len(), 1);
        assert_eq!(e.to_json().unwrap(), r#"{"status":"ok"}"#);
    }

    #[test]
    fn add_path_two_segments() {
        let mut e = WideEvent::<TestKey>::new();
        e.add_path(&[TestKey::Service, TestKey::Name], "my-service");
        assert_eq!(e.to_json().unwrap(), r#"{"service":{"name":"my-service"}}"#);
    }

    #[test]
    fn add_path_updates_existing_nested() {
        let mut e = WideEvent::<TestKey>::new();
        e.add_path(&[TestKey::Service, TestKey::Name], "a");
        e.add_path(&[TestKey::Service, TestKey::Version], "1.0.0");
        e.add_path(&[TestKey::Service, TestKey::Name], "b");
        let json = e.to_json().unwrap();
        assert_eq!(json, r#"{"service":{"name":"b","version":"1.0.0"}}"#);
    }

    #[test]
    fn add_path_duration_path() {
        use crate::key::Key;
        let mut e = WideEvent::<TestKey>::new();
        e.add_path(<TestKey as Key>::DURATION_PATH, 42u64);
        let json = e.to_json().unwrap();
        assert_eq!(json, r#"{"duration":{"total_ms":42}}"#);
    }

    #[test]
    fn inc_path_single_segment() {
        let mut e = WideEvent::<TestKey>::new();
        e.inc_path(&[TestKey::Requests]);
        e.inc_path(&[TestKey::Requests]);
        e.inc_path(&[TestKey::Requests]);
        assert_eq!(e.to_json().unwrap(), r#"{"requests":3}"#);
    }

    #[test]
    fn dec_path_single_segment() {
        let mut e = WideEvent::<TestKey>::new();
        e.dec_path(&[TestKey::Requests]);
        assert_eq!(e.to_json().unwrap(), r#"{"requests":-1}"#);
    }

    #[test]
    fn add_n_path_single_segment() {
        let mut e = WideEvent::<TestKey>::new();
        e.add_n_path(&[TestKey::Requests], 5);
        e.add_n_path(&[TestKey::Requests], -2);
        assert_eq!(e.to_json().unwrap(), r#"{"requests":3}"#);
    }

    #[test]
    fn inc_path_two_segments() {
        let mut e = WideEvent::<TestKey>::new();
        e.add_path(&[TestKey::Service, TestKey::Requests], 0u64);
        e.inc_path(&[TestKey::Service, TestKey::Requests]);
        e.inc_path(&[TestKey::Service, TestKey::Requests]);
        let json = e.to_json().unwrap();
        assert_eq!(json, r#"{"service":{"requests":2}}"#);
    }

    #[test]
    fn add_n_path_two_segments() {
        let mut e = WideEvent::<TestKey>::new();
        e.add_n_path(&[TestKey::Service, TestKey::Requests], 10);
        e.add_n_path(&[TestKey::Service, TestKey::Requests], -3);
        let json = e.to_json().unwrap();
        assert_eq!(json, r#"{"service":{"requests":7}}"#);
    }

    // ---- inc / dec / add_n single-key tests ----

    #[test]
    fn inc_initializes_to_one() {
        let mut e = WideEvent::<TestKey>::new();
        e.inc(TestKey::Requests);
        assert_eq!(e.to_json().unwrap(), r#"{"requests":1}"#);
    }

    #[test]
    fn inc_increments_existing() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Requests, 41u64);
        e.inc(TestKey::Requests);
        assert_eq!(e.to_json().unwrap(), r#"{"requests":42}"#);
    }

    #[test]
    fn dec_initializes_to_minus_one() {
        let mut e = WideEvent::<TestKey>::new();
        e.dec(TestKey::Requests);
        assert_eq!(e.to_json().unwrap(), r#"{"requests":-1}"#);
    }

    #[test]
    fn dec_decrements_existing() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Requests, 5u64);
        e.dec(TestKey::Requests);
        assert_eq!(e.to_json().unwrap(), r#"{"requests":4}"#);
    }

    #[test]
    fn add_n_initializes() {
        let mut e = WideEvent::<TestKey>::new();
        e.add_n(TestKey::Requests, 10);
        assert_eq!(e.to_json().unwrap(), r#"{"requests":10}"#);
    }

    #[test]
    fn add_n_adds_to_existing() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Requests, 40u64);
        e.add_n(TestKey::Requests, 2);
        assert_eq!(e.to_json().unwrap(), r#"{"requests":42}"#);
    }

    #[test]
    fn add_n_negative_from_u64() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Requests, 10u64);
        e.add_n(TestKey::Requests, -3);
        assert_eq!(e.to_json().unwrap(), r#"{"requests":7}"#);
    }

    // ---- Log entry tests ----

    #[test]
    fn append_log_entry_single() {
        let mut e = WideEvent::<TestKey>::new();
        e.append_log_entry("info", "request received");
        assert!(e.has_logs());
        assert_eq!(e.log_len(), 1);
        let json = e.to_json().unwrap();
        assert_eq!(
            json,
            r#"{"log":[{"level":"info","message":"request received"}]}"#
        );
    }

    #[test]
    fn append_log_entry_multiple_preserves_order() {
        let mut e = WideEvent::<TestKey>::new();
        e.append_log_entry("info", "first");
        e.append_log_entry("warn", "second");
        e.append_log_entry("error", "third");
        assert_eq!(e.log_len(), 3);
        let json = e.to_json().unwrap();
        assert_eq!(
            json,
            concat!(
                r#"{"log":["#,
                r#"{"level":"info","message":"first"},"#,
                r#"{"level":"warn","message":"second"},"#,
                r#"{"level":"error","message":"third"}"#,
                r#"]}"#,
            )
        );
    }

    #[test]
    fn log_entries_after_user_entries() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Status, "ok");
        e.append_log_entry("info", "done");
        let json = e.to_json().unwrap();
        assert_eq!(
            json,
            r#"{"status":"ok","log":[{"level":"info","message":"done"}]}"#
        );
    }

    #[test]
    fn no_log_key_when_empty() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Status, "ok");
        let json = e.to_json().unwrap();
        assert_eq!(json, r#"{"status":"ok"}"#);
        assert!(!json.contains("log"));
    }
}