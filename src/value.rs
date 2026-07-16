use faststr::FastStr;
use serde::ser::{Serialize, Serializer};
use smallvec::SmallVec;

use crate::key::Key;
use crate::wide_event::WideEvent;

/// A JSON value stored in a wide event.
///
/// All variants are safe to construct, clone, and drop — the compiler
/// handles destructors automatically. No `unsafe` is required anywhere
/// in this type.
pub enum Value<K: Key> {
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    Str(FastStr),
    StaticStr(&'static str),
    Array(Box<SmallVec<[Value<K>; 8]>>),
    Object(Box<WideEvent<K>>),
}

impl<K: Key> Value<K> {
    /// Creates an Object value from a WideEvent.
    #[inline]
    pub(crate) fn from_object(ev: WideEvent<K>) -> Self {
        Value::Object(Box::new(ev))
    }

    /// Returns `true` if this value is an `Object`.
    #[inline]
    pub(crate) fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }
}

#[cfg(test)]
impl<K: Key> Value<K> {
    /// Creates an Array value from a SmallVec of Values.
    #[inline]
    pub(crate) fn from_array(arr: SmallVec<[Value<K>; 8]>) -> Self {
        Value::Array(Box::new(arr))
    }

    #[inline]
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s.as_str()),
            Value::StaticStr(s) => Some(s),
            _ => None,
        }
    }

    /// Returns a reference to the Array if this value is an Array.
    #[inline]
    pub(crate) fn as_array_ref(&self) -> Option<&SmallVec<[Value<K>; 8]>> {
        match self {
            Value::Array(arr) => Some(arr),
            _ => None,
        }
    }

    /// Creates a `StaticStr` value from a `&'static str` — zero-copy, zero-allocation.
    #[inline]
    pub(crate) fn from_static_str(s: &'static str) -> Self {
        Value::StaticStr(s)
    }
}

impl<K: Key> Clone for Value<K> {
    #[inline]
    fn clone(&self) -> Self {
        match self {
            Value::Null => Value::Null,
            Value::Bool(b) => Value::Bool(*b),
            Value::I64(i) => Value::I64(*i),
            Value::U64(u) => Value::U64(*u),
            Value::F64(f) => Value::F64(*f),
            Value::Str(s) => Value::Str(s.clone()),
            Value::StaticStr(s) => Value::StaticStr(s),
            Value::Array(arr) => Value::Array(arr.clone()),
            Value::Object(obj) => Value::Object(obj.clone()),
        }
    }
}

impl<K: Key> std::fmt::Debug for Value<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Null => f.write_str("Value::Null"),
            Value::Bool(b) => f.debug_tuple("Value::Bool").field(b).finish(),
            Value::I64(i) => f.debug_tuple("Value::I64").field(i).finish(),
            Value::U64(u) => f.debug_tuple("Value::U64").field(u).finish(),
            Value::F64(fl) => f.debug_tuple("Value::F64").field(fl).finish(),
            Value::Str(s) => f.debug_tuple("Value::Str").field(s).finish(),
            Value::StaticStr(s) => f.debug_tuple("Value::StaticStr").field(s).finish(),
            Value::Array(arr) => f.debug_tuple("Value::Array").field(arr).finish(),
            Value::Object(obj) => f.debug_tuple("Value::Object").field(obj).finish(),
        }
    }
}

impl<K: Key> Serialize for Value<K> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::I64(i) => serializer.serialize_i64(*i),
            Value::U64(u) => serializer.serialize_u64(*u),
            Value::F64(f) => serializer.serialize_f64(*f),
            Value::Str(s) => serializer.serialize_str(s.as_str()),
            Value::StaticStr(s) => serializer.serialize_str(s),
            Value::Array(arr) => arr.serialize(serializer),
            Value::Object(obj) => obj.serialize(serializer),
        }
    }
}

// ── From impls ──

impl<K: Key> From<bool> for Value<K> {
    #[inline]
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl<K: Key> From<i64> for Value<K> {
    #[inline]
    fn from(n: i64) -> Self {
        Value::I64(n)
    }
}

impl<K: Key> From<u64> for Value<K> {
    #[inline]
    fn from(n: u64) -> Self {
        Value::U64(n)
    }
}

impl<K: Key> From<f64> for Value<K> {
    #[inline]
    fn from(n: f64) -> Self {
        Value::F64(n)
    }
}

impl<K: Key> From<&str> for Value<K> {
    #[inline]
    fn from(s: &str) -> Self {
        Value::Str(FastStr::new(s))
    }
}

impl<K: Key> From<String> for Value<K> {
    #[inline]
    fn from(s: String) -> Self {
        Value::Str(FastStr::from_string(s))
    }
}

impl<K: Key> From<FastStr> for Value<K> {
    #[inline]
    fn from(s: FastStr) -> Self {
        Value::Str(s)
    }
}

impl<K: Key> From<()> for Value<K> {
    #[inline]
    fn from(_: ()) -> Self {
        Value::Null
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::test_support::TestKey;

    #[test]
    fn from_bool() {
        assert!(matches!(Value::<TestKey>::from(true), Value::Bool(true)));
        assert!(matches!(Value::<TestKey>::from(false), Value::Bool(false)));
    }

    #[test]
    fn from_i64() {
        let v = Value::<TestKey>::from(-42i64);
        assert!(matches!(v, Value::I64(-42)));
    }

    #[test]
    fn from_u64() {
        let v = Value::<TestKey>::from(99u64);
        assert!(matches!(v, Value::U64(99)));
    }

    #[test]
    fn from_f64() {
        let v = Value::<TestKey>::from(3.15f64);
        assert!(matches!(v, Value::F64(_)));
    }

    #[test]
    fn from_str_is_string() {
        let v = Value::<TestKey>::from("hello");
        assert!(matches!(v, Value::Str(_)));
        assert_eq!(v.as_str(), Some("hello"));
    }

    #[test]
    fn from_static_str() {
        let v = Value::<TestKey>::from_static_str("world");
        assert!(matches!(v, Value::StaticStr(_)));
        assert_eq!(v.as_str(), Some("world"));
    }

    #[test]
    fn from_owned_string_is_string() {
        let v = Value::<TestKey>::from("world".to_string());
        assert!(matches!(v, Value::Str(_)));
        assert_eq!(v.as_str(), Some("world"));
    }

    #[test]
    fn from_unit_is_null() {
        assert!(matches!(Value::<TestKey>::from(()), Value::Null));
    }

    #[test]
    fn serialize_null() {
        let v = Value::<TestKey>::from(());
        let s = sonic_rs::to_string(&v).unwrap();
        assert_eq!(s, "null");
    }

    #[test]
    fn serialize_scalars() {
        assert_eq!(
            sonic_rs::to_string(&Value::<TestKey>::from(true)).unwrap(),
            "true"
        );
        assert_eq!(
            sonic_rs::to_string(&Value::<TestKey>::from(false)).unwrap(),
            "false"
        );
        assert_eq!(
            sonic_rs::to_string(&Value::<TestKey>::from(-7i64)).unwrap(),
            "-7"
        );
        assert_eq!(
            sonic_rs::to_string(&Value::<TestKey>::from(42u64)).unwrap(),
            "42"
        );
        assert_eq!(
            sonic_rs::to_string(&Value::<TestKey>::from("hi")).unwrap(),
            "\"hi\""
        );
    }

    #[test]
    fn serialize_static_str() {
        let v = Value::<TestKey>::from_static_str("hello");
        assert_eq!(sonic_rs::to_string(&v).unwrap(), "\"hello\"");
    }

    #[test]
    fn serialize_array() {
        let arr: SmallVec<[Value<TestKey>; 8]> =
            smallvec::smallvec![Value::from(1i64), Value::from(2i64)];
        let v = Value::<TestKey>::from_array(arr);
        assert_eq!(sonic_rs::to_string(&v).unwrap(), "[1,2]");
    }

    #[test]
    fn clone_works() {
        let v = Value::<TestKey>::from("hello");
        let v2 = v.clone();
        assert_eq!(v2.as_str(), Some("hello"));
    }

    #[test]
    fn clone_static_str() {
        let v = Value::<TestKey>::from_static_str("world");
        let v2 = v.clone();
        assert_eq!(v2.as_str(), Some("world"));
    }

    #[test]
    fn clone_object() {
        let mut ev = WideEvent::<TestKey>::new();
        ev.add(TestKey::Status, "ok");
        let v = Value::<TestKey>::from_object(ev);
        let v2 = v.clone();
        assert!(matches!(v2, Value::Object(_)));
        let json = sonic_rs::to_string(&v2).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
    }
}
