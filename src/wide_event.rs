use std::sync::Arc;

use smallvec::SmallVec;
use serde::ser::{SerializeMap, Serialize, Serializer};

use crate::error::Error;
use crate::key::Key;
use crate::value::Value;

type ConflictCb<K> = Arc<dyn Fn(&mut WideEvent<K>, K) + Send + Sync>;

#[derive(Clone)]
pub struct WideEvent<K: Key> {
    pub(crate) entries: SmallVec<[(K, Value<K>); 24]>,
    pub(crate) on_type_conflict: Option<ConflictCb<K>>,
}

impl<K: Key + std::fmt::Debug> std::fmt::Debug for WideEvent<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WideEvent")
            .field("entries", &self.entries)
            .field("has_conflict_callback", &self.on_type_conflict.is_some())
            .finish()
    }
}

impl<K: Key> Serialize for WideEvent<K> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (key, value) in &self.entries {
            map.serialize_entry(key.as_str(), value)?;
        }
        map.end()
    }
}

impl<K: Key> WideEvent<K> {
    #[inline]
    pub fn new() -> Self {
        Self {
            entries: SmallVec::new(),
            on_type_conflict: None,
        }
    }

    pub fn new_with_warnings<F: Fn(&mut WideEvent<K>, K) + Send + Sync + 'static>(f: F) -> Self {
        Self {
            entries: SmallVec::new(),
            on_type_conflict: Some(Arc::new(f)),
        }
    }

    pub(crate) fn with_callback(cb: Option<ConflictCb<K>>) -> Self {
        Self {
            entries: SmallVec::new(),
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

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
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
        K0,  K1,  K2,  K3,  K4,  K5,  K6,  K7,
        K8,  K9,  K10, K11, K12, K13, K14, K15,
        K16, K17, K18, K19, K20, K21, K22, K23,
    }

    impl crate::key::Key for BigKey {
        fn as_str(self) -> &'static str {
            match self {
                BigKey::K0  => "k0",  BigKey::K1  => "k1",
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
        const SUBSYSTEM_KEY: Self = BigKey::K0;
        const DURATION_NS_KEY: Self = BigKey::K1;
    }

    #[test]
    fn new_is_empty() {
        let e = WideEvent::<TestKey>::new();
        assert!(e.is_empty());
        assert_eq!(e.len(), 0);
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
        e.add(TestKey::UserId,  42u64);
        e.add(TestKey::Status,  "ok");
        e.add(TestKey::Tag,     "web");
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
        e.object(TestKey::Details).add(TestKey::UserId, 1u64);
        e.object(TestKey::Details).add(TestKey::Status, "ok");
        let json = e.to_json().unwrap();
        assert!(json.contains("\"user_id\""));
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
        e.add(TestKey::UserId, 7u64);
        e.add(TestKey::Status, "ok");
        e.add(TestKey::Flag,   false);
        let json = e.to_json().unwrap();
        assert!(json.starts_with('{'));
        assert!(json.ends_with('}'));
    }

    #[test]
    fn to_json_roundtrip() {
        let mut e = WideEvent::<TestKey>::new();
        e.add(TestKey::UserId, 42u64);
        e.add(TestKey::Status, "active");
        let json = e.to_json().unwrap();
        let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        assert_eq!(parsed["user_id"], 42u64);
        assert_eq!(parsed["status"], "active");
    }

    #[test]
    fn serialize_nested_object() {
        let mut e = WideEvent::<TestKey>::new();
        e.object(TestKey::Details).add(TestKey::UserId, 42u64);
        assert_eq!(e.to_json().unwrap(), r#"{"details":{"user_id":42}}"#);
    }

    #[test]
    fn inline_capacity_not_spilled() {
        let keys = [
            BigKey::K0,  BigKey::K1,  BigKey::K2,  BigKey::K3,
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
}