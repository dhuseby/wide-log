use smallvec::SmallVec;

use crate::error::Error;
use crate::key::Key;
use crate::log::LogEntry;
use crate::value::Value;

pub(crate) type ConflictFn<K> = fn(&mut WideEvent<K>, K);

const INLINE_CAP: usize = 32;
const LOG_INLINE_CAP: usize = 16;

/// A wide event — a structured log record that accumulates fields throughout
/// a request/task lifecycle and is emitted as a single JSON line on completion.
///
/// Fields are stored in an array indexed by `K::as_index()` — O(1) lookup
/// with no linear scan. Up to `INLINE_CAP` (24) `Option<Value>` slots are
/// inline on the stack; zero heap allocation in the common case. Log entries
/// are stored in a separate `SmallVec` with inline capacity of 16.
///
/// The `Serialize` impl emits all user entries in enum-variant order, then
/// the `"log"` key (if any log entries have been accumulated). The `"log"`
/// key is never declared by the user — it appears automatically.
///
/// This type is not constructed directly by users. The `wide_log!` macro
/// generates a `WideLogGuard` that owns a `WideEvent` and manages the
/// thread-local/task-local pointer via `current()`.
#[derive(Clone)]
pub struct WideEvent<K: Key> {
    /// Indexed value slots. `values[key.as_index()]` holds the value for that
    /// key, or `None` if not yet set.
    pub(crate) values: SmallVec<[Option<Value<K>>; INLINE_CAP]>,
    /// Log entries accumulated by `info!`, `warn!`, etc. (up to 16 inline).
    pub(crate) log_entries: SmallVec<[LogEntry<K>; LOG_INLINE_CAP]>,
    /// Optional callback fired when a key's value type conflicts.
    pub(crate) on_type_conflict: Option<ConflictFn<K>>,
}

impl<K: Key> WideEvent<K> {
    /// Creates a new empty wide event with no conflict callback.
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            values: SmallVec::new(),
            log_entries: SmallVec::new(),
            on_type_conflict: None,
        }
    }

    /// Ensures the values SmallVec is large enough to index by `key.as_index()`.
    #[inline]
    pub(crate) fn ensure_capacity(&mut self, idx: usize) {
        if idx >= self.values.len() {
            self.values.resize(idx + 1, None);
        }
    }

    /// Sets or replaces a field value at the given key. O(1) indexed access.
    #[inline]
    pub(crate) fn add<V: Into<Value<K>>>(&mut self, key: K, value: V) {
        let idx = key.as_index();
        self.ensure_capacity(idx);
        self.values[idx] = Some(value.into());
    }

    /// Gets or creates a nested object at the given key, returning `&mut` to it.
    #[inline]
    pub(crate) fn object(&mut self, key: K) -> &mut WideEvent<K> {
        let idx = key.as_index();
        self.ensure_capacity(idx);
        let needs_replace = match &self.values[idx] {
            None => true,
            Some(v) => !v.is_object(),
        };
        if needs_replace {
            if let Some(cb) = self.on_type_conflict
                && self.values[idx].is_some()
            {
                cb(self, key);
            }
            self.values[idx] = Some(Value::from_object(self.new_child()));
        }
        match &mut self.values[idx] {
            Some(Value::Object(boxed)) => boxed,
            _ => unreachable!(),
        }
    }

    #[inline]
    pub(crate) fn new_child(&self) -> WideEvent<K> {
        WideEvent {
            values: SmallVec::new(),
            log_entries: SmallVec::new(),
            on_type_conflict: self.on_type_conflict,
        }
    }

    /// Set a value at a nested path. Traverses/creates intermediate objects.
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
    pub(crate) fn inc(&mut self, key: K) {
        let idx = key.as_index();
        self.ensure_capacity(idx);
        let new_val = match &self.values[idx] {
            Some(Value::U64(n)) => Value::from(*n + 1),
            Some(Value::I64(n)) => Value::from(*n + 1),
            Some(_) => Value::from(1u64),
            None => Value::from(1u64),
        };
        self.values[idx] = Some(new_val);
    }

    /// Decrement a numeric field by 1. Initializes to -1 if absent.
    #[inline]
    pub(crate) fn dec(&mut self, key: K) {
        let idx = key.as_index();
        self.ensure_capacity(idx);
        let new_val = match &self.values[idx] {
            Some(Value::U64(n)) => {
                if *n > 0 {
                    Value::from(*n - 1)
                } else {
                    Value::from(0u64)
                }
            }
            Some(Value::I64(n)) => Value::from(*n - 1),
            Some(_) => Value::from(-1i64),
            None => Value::from(-1i64),
        };
        self.values[idx] = Some(new_val);
    }

    /// Add a number to a numeric field. Initializes to `n` if absent.
    #[inline]
    pub(crate) fn add_n(&mut self, key: K, n: i64) {
        let idx = key.as_index();
        self.ensure_capacity(idx);
        let new_val = match &self.values[idx] {
            Some(Value::U64(u)) => Value::from(u.saturating_add_signed(n)),
            Some(Value::I64(i)) => Value::from(*i + n),
            Some(_) => {
                if n >= 0 {
                    Value::from(n as u64)
                } else {
                    Value::from(n)
                }
            }
            None => {
                if n >= 0 {
                    Value::from(n as u64)
                } else {
                    Value::from(n)
                }
            }
        };
        self.values[idx] = Some(new_val);
    }

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

    #[inline]
    pub fn append_log_entry(&mut self, level: &'static str, message: &str) {
        self.log_entries.push(LogEntry::new(
            level,
            crate::log::LogMsg::Owned(faststr::FastStr::new(message)),
        ));
    }

    /// Append a log entry with a `&'static str` message — zero-copy.
    #[inline]
    pub fn append_log_entry_static(&mut self, level: &'static str, message: &'static str) {
        self.log_entries
            .push(LogEntry::new(level, crate::log::LogMsg::Static(message)));
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.values.iter().filter(|v| v.is_some()).count()
    }

    pub fn to_json(&self) -> Result<String, Error> {
        sonic_rs::to_string(self).map_err(|e| Error::Serialize(e.to_string()))
    }

    /// Serialize directly to a writer, bypassing serde entirely.
    /// Uses itoa/ryu for zero-allocation number formatting.
    pub fn serialize_to<W: std::io::Write>(&self, w: &mut W) -> Result<(), Error> {
        write_event(self, w).map_err(|e| Error::Serialize(e.to_string()))
    }

    /// Count of present entries (alias for `len()`).
    #[inline]
    pub(crate) fn count_present(&self) -> usize {
        self.values.iter().filter(|v| v.is_some()).count()
    }
}

#[cfg(test)]
impl<K: Key> WideEvent<K> {
    /// Creates a new empty wide event with a type-conflict callback.
    #[inline]
    pub(crate) fn new_with_warnings(f: ConflictFn<K>) -> Self {
        Self {
            values: SmallVec::new(),
            log_entries: SmallVec::new(),
            on_type_conflict: Some(f),
        }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.values.iter().all(|v| v.is_none())
    }

    #[inline]
    pub(crate) fn log_len(&self) -> usize {
        self.log_entries.len()
    }

    #[inline]
    pub(crate) fn has_logs(&self) -> bool {
        !self.log_entries.is_empty()
    }
}

impl<K: Key> Default for WideEvent<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Key> serde::Serialize for WideEvent<K> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let present = self.count_present();
        let total = present + if self.log_entries.is_empty() { 0 } else { 1 };
        let mut map = serializer.serialize_map(Some(total))?;
        for (i, key) in K::KEYS.iter().enumerate() {
            if i < self.values.len()
                && let Some(value) = &self.values[i]
            {
                map.serialize_entry(key.as_str(), value)?;
            }
        }
        if !self.log_entries.is_empty() {
            map.serialize_entry(K::LOG_KEY, &self.log_entries)?;
        }
        map.end()
    }
}

impl<K: Key> std::fmt::Debug for WideEvent<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WideEvent")
            .field("present", &self.len())
            .field("log_entries_len", &self.log_entries.len())
            .field("has_conflict_callback", &self.on_type_conflict.is_some())
            .finish()
    }
}

// ── Direct serializer (bypasses serde) ──

#[inline]
fn write_event<K: Key, W: std::io::Write>(ev: &WideEvent<K>, w: &mut W) -> std::io::Result<()> {
    w.write_all(b"{")?;
    let mut first = true;
    for (i, key) in K::KEYS.iter().enumerate() {
        if i < ev.values.len()
            && let Some(val) = &ev.values[i]
        {
            if !first {
                w.write_all(b",")?;
            }
            first = false;
            write_json_str(w, key.as_str())?;
            w.write_all(b":")?;
            write_value(val, w)?;
        }
    }
    if !ev.log_entries.is_empty() {
        if !first {
            w.write_all(b",")?;
        }
        write_json_str(w, K::LOG_KEY)?;
        w.write_all(b":")?;
        w.write_all(b"[")?;
        for (j, entry) in ev.log_entries.iter().enumerate() {
            if j > 0 {
                w.write_all(b",")?;
            }
            w.write_all(b"{")?;
            write_json_str(w, K::LEVEL_KEY)?;
            w.write_all(b":")?;
            write_json_str(w, entry.level)?;
            w.write_all(b",")?;
            write_json_str(w, K::MESSAGE_KEY)?;
            w.write_all(b":")?;
            write_json_str(w, entry.message.as_str())?;
            w.write_all(b"}")?;
        }
        w.write_all(b"]")?;
    }
    w.write_all(b"}")?;
    Ok(())
}

#[inline]
fn write_value<K: Key, W: std::io::Write>(
    val: &crate::value::Value<K>,
    w: &mut W,
) -> std::io::Result<()> {
    match val {
        Value::Null => w.write_all(b"null"),
        Value::Bool(b) => {
            if *b {
                w.write_all(b"true")
            } else {
                w.write_all(b"false")
            }
        }
        Value::I64(i) => {
            let mut buf = itoa::Buffer::new();
            w.write_all(buf.format(*i).as_bytes())
        }
        Value::U64(u) => {
            let mut buf = itoa::Buffer::new();
            w.write_all(buf.format(*u).as_bytes())
        }
        Value::F64(f) => {
            let mut buf = ryu::Buffer::new();
            w.write_all(buf.format(*f).as_bytes())
        }
        Value::Str(s) => write_json_str(w, s.as_str()),
        Value::StaticStr(s) => write_json_str(w, s),
        Value::Array(arr) => {
            w.write_all(b"[")?;
            for (i, v) in arr.iter().enumerate() {
                if i > 0 {
                    w.write_all(b",")?;
                }
                write_value(v, w)?;
            }
            w.write_all(b"]")?;
            Ok(())
        }
        Value::Object(obj) => write_event(obj, w),
    }
}

#[inline]
fn write_json_str<W: std::io::Write>(w: &mut W, s: &str) -> std::io::Result<()> {
    w.write_all(b"\"")?;
    for c in s.bytes() {
        match c {
            b'"' => w.write_all(b"\\\"")?,
            b'\\' => w.write_all(b"\\\\")?,
            b'\n' => w.write_all(b"\\n")?,
            b'\t' => w.write_all(b"\\t")?,
            b'\r' => w.write_all(b"\\r")?,
            0x08 => w.write_all(b"\\b")?,
            0x0c => w.write_all(b"\\f")?,
            c if c < 0x20 => {
                w.write_all(format!("\\u{:04x}", c).as_bytes())?;
            }
            c => w.write_all(&[c])?,
        }
    }
    w.write_all(b"\"")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::test_support::TestKey;

    // 25-variant key used to verify the inline SmallVec capacity.
    #[derive(Copy, Clone, PartialEq, Eq, Debug)]
    #[repr(u8)]
    #[allow(dead_code)]
    enum BigKey {
        Duration,
        TotalMs,
        Event,
        Timestamp,
        Id,
        K2,
        K3,
        K4,
        K5,
        K6,
        K7,
        K8,
        K9,
        K10,
        K11,
        K12,
        K13,
        K14,
        K15,
        K16,
        K17,
        K18,
        K19,
        K20,
        K21,
        K22,
    }

    impl crate::key::Key for BigKey {
        fn as_str(self) -> &'static str {
            match self {
                BigKey::Duration => "duration",
                BigKey::TotalMs => "total_ms",
                BigKey::Event => "event",
                BigKey::Timestamp => "timestamp",
                BigKey::Id => "id",
                BigKey::K2 => "k2",
                BigKey::K3 => "k3",
                BigKey::K4 => "k4",
                BigKey::K5 => "k5",
                BigKey::K6 => "k6",
                BigKey::K7 => "k7",
                BigKey::K8 => "k8",
                BigKey::K9 => "k9",
                BigKey::K10 => "k10",
                BigKey::K11 => "k11",
                BigKey::K12 => "k12",
                BigKey::K13 => "k13",
                BigKey::K14 => "k14",
                BigKey::K15 => "k15",
                BigKey::K16 => "k16",
                BigKey::K17 => "k17",
                BigKey::K18 => "k18",
                BigKey::K19 => "k19",
                BigKey::K20 => "k20",
                BigKey::K21 => "k21",
                BigKey::K22 => "k22",
            }
        }
        const MAX_KEYS: usize = 25;
        const KEYS: &'static [Self] = &[
            BigKey::Duration,
            BigKey::TotalMs,
            BigKey::Event,
            BigKey::Timestamp,
            BigKey::Id,
            BigKey::K2,
            BigKey::K3,
            BigKey::K4,
            BigKey::K5,
            BigKey::K6,
            BigKey::K7,
            BigKey::K8,
            BigKey::K9,
            BigKey::K10,
            BigKey::K11,
            BigKey::K12,
            BigKey::K13,
            BigKey::K14,
            BigKey::K15,
            BigKey::K16,
            BigKey::K17,
            BigKey::K18,
            BigKey::K19,
            BigKey::K20,
            BigKey::K21,
            BigKey::K22,
        ];
        const KEY_STRS: &'static [&'static str] = &[
            "duration",
            "total_ms",
            "event",
            "timestamp",
            "id",
            "k2",
            "k3",
            "k4",
            "k5",
            "k6",
            "k7",
            "k8",
            "k9",
            "k10",
            "k11",
            "k12",
            "k13",
            "k14",
            "k15",
            "k16",
            "k17",
            "k18",
            "k19",
            "k20",
            "k21",
            "k22",
        ];
        fn as_index(self) -> usize {
            self as usize
        }
        const DURATION_PATH: &'static [Self] = &[BigKey::Duration, BigKey::TotalMs];
        const TIMESTAMP_PATH: &'static [Self] = &[BigKey::Event, BigKey::Timestamp];
        const ID_PATH: &'static [Self] = &[BigKey::Event, BigKey::Id];
        const LOG_KEY: &'static str = "log";
        const LEVEL_KEY: &'static str = "level";
        const MESSAGE_KEY: &'static str = "message";
    }

    // ── Basic tests ──

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
        e.add(TestKey::Status, "ok");
        e.add(TestKey::Tag, "web");
        assert_eq!(e.len(), 3);
    }

    // ── Object tests ──

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

    // ── Type conflict tests (fn pointer version) ──

    #[test]
    fn object_type_conflict_replaces_with_object() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Details, true);
        e.object(TestKey::Details).add(TestKey::Status, "ok");
        assert!(matches!(&e.values[TestKey::Details.as_index()], Some(v) if v.is_object()));
    }

    #[test]
    fn object_type_conflict_fires_callback() {
        use std::sync::atomic::{AtomicU32, Ordering};

        static COUNTER: AtomicU32 = AtomicU32::new(0);
        COUNTER.store(0, Ordering::SeqCst);

        fn cb(_event: &mut WideEvent<TestKey>, _key: TestKey) {
            COUNTER.fetch_add(1, Ordering::SeqCst);
        }

        let mut e = WideEvent::new_with_warnings(cb);
        e.add(TestKey::Details, true);
        e.object(TestKey::Details);
        assert_eq!(COUNTER.load(Ordering::SeqCst), 1);
    }

    // ── Serialization tests ──

    #[test]
    fn to_json_valid() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Requests, 7u64);
        e.add(TestKey::Status, "ok");
        e.add(TestKey::Flag, false);
        let json = e.to_json().unwrap();
        assert!(json.starts_with('{'));
        assert!(json.ends_with('}'));
    }

    #[test]
    fn to_json_roundtrip() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::Requests, 42u64);
        e.add(TestKey::Status, "active");
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
        let mut e = WideEvent::<BigKey>::new();
        for i in 0..BigKey::MAX_KEYS {
            e.add(BigKey::KEYS[i], i as u64);
        }
        assert_eq!(e.len(), BigKey::MAX_KEYS);
        assert!(
            !e.values.spilled(),
            "{} entries must fit in inline storage",
            BigKey::MAX_KEYS
        );
    }

    // ── Path method tests ──

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
        // Index-ordered: service object has name and version
        assert!(json.contains(r#""name":"b""#));
        assert!(json.contains(r#""version":"1.0.0""#));
    }

    #[test]
    fn add_path_duration_path() {
        use crate::key::Key;
        let mut e = WideEvent::<TestKey>::new();
        e.add_path(<TestKey as Key>::DURATION_PATH, 42u64);
        let json = e.to_json().unwrap();
        assert_eq!(json, r#"{"duration":{"total_ms":42}}"#);
    }

    // ── inc / dec / add_n tests ──

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

    // ── Path inc/dec/add_n tests ──

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
        assert!(json.contains(r#""requests":2"#));
    }

    #[test]
    fn add_n_path_two_segments() {
        let mut e = WideEvent::<TestKey>::new();
        e.add_n_path(&[TestKey::Service, TestKey::Requests], 10);
        e.add_n_path(&[TestKey::Service, TestKey::Requests], -3);
        let json = e.to_json().unwrap();
        assert!(json.contains(r#""requests":7"#));
    }

    // ── Log entry tests ──

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
