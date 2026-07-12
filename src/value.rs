use faststr::FastStr;
use serde::ser::{Serialize, Serializer};
use smallvec::SmallVec;

use crate::key::Key;
use crate::wide_event::WideEvent;

/// A JSON value stored in a wide event.
///
/// This enum is generic over the key type `K` because `Object` and `Array`
/// variants can contain nested [`WideEvent`]s that are parameterized by `K`.
///
/// # Conversions
///
/// All primitive types implement `Into<Value<K>>`:
///
/// - `bool` → [`Value::Bool`]
/// - `i64` → [`Value::I64`]
/// - `u64` → [`Value::U64`]
/// - `f64` → [`Value::F64`]
/// - `&str`, `String`, `FastStr` → [`Value::String`]
/// - `()` → [`Value::Null`]
///
/// The `wl_set!` and `wl_null!` macros use these conversions transparently.
///
/// [`WideEvent`]: crate::WideEvent
#[derive(Debug, Clone)]
pub enum Value<K: Key> {
    /// JSON `null`.
    Null,
    /// A boolean value.
    Bool(bool),
    /// A signed 64-bit integer.
    I64(i64),
    /// An unsigned 64-bit integer.
    U64(u64),
    /// A 64-bit floating-point value.
    F64(f64),
    /// A string value, stored with small-string optimization via `FastStr`.
    String(FastStr),
    /// A JSON array of values.
    Array(SmallVec<[Box<Value<K>>; 8]>),
    /// A nested JSON object (a boxed [`WideEvent`]).
    ///
    /// [`WideEvent`]: crate::WideEvent
    Object(Box<WideEvent<K>>),
}

impl<K: Key> Serialize for Value<K> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null       => serializer.serialize_unit(),
            Value::Bool(b)    => serializer.serialize_bool(*b),
            Value::I64(n)     => serializer.serialize_i64(*n),
            Value::U64(n)     => serializer.serialize_u64(*n),
            Value::F64(n)     => serializer.serialize_f64(*n),
            Value::String(s)  => serializer.serialize_str(s.as_str()),
            Value::Array(arr) => arr.serialize(serializer),
            Value::Object(obj) => obj.serialize(serializer),
        }
    }
}

impl<K: Key> From<bool> for Value<K> {
    #[inline]
    fn from(b: bool) -> Self { Value::Bool(b) }
}

impl<K: Key> From<i64> for Value<K> {
    #[inline]
    fn from(n: i64) -> Self { Value::I64(n) }
}

impl<K: Key> From<u64> for Value<K> {
    #[inline]
    fn from(n: u64) -> Self { Value::U64(n) }
}

impl<K: Key> From<f64> for Value<K> {
    #[inline]
    fn from(n: f64) -> Self { Value::F64(n) }
}

impl<K: Key> From<&str> for Value<K> {
    #[inline]
    fn from(s: &str) -> Self { Value::String(FastStr::new(s)) }
}

impl<K: Key> From<String> for Value<K> {
    #[inline]
    fn from(s: String) -> Self { Value::String(FastStr::new(&s)) }
}

impl<K: Key> From<FastStr> for Value<K> {
    #[inline]
    fn from(s: FastStr) -> Self { Value::String(s) }
}

impl<K: Key> From<()> for Value<K> {
    #[inline]
    fn from(_: ()) -> Self { Value::Null }
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
        assert!(matches!(Value::<TestKey>::from(-42i64), Value::I64(-42)));
    }

    #[test]
    fn from_u64() {
        assert!(matches!(Value::<TestKey>::from(99u64), Value::U64(99)));
    }

    #[test]
    fn from_f64() {
        assert!(matches!(Value::<TestKey>::from(3.15f64), Value::F64(_)));
    }

    #[test]
    fn from_str_is_string() {
        assert!(matches!(Value::<TestKey>::from("hello"), Value::String(_)));
    }

    #[test]
    fn from_owned_string_is_string() {
        assert!(matches!(
            Value::<TestKey>::from("world".to_string()),
            Value::String(_)
        ));
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
        assert_eq!(sonic_rs::to_string(&Value::<TestKey>::from(true)).unwrap(),  "true");
        assert_eq!(sonic_rs::to_string(&Value::<TestKey>::from(false)).unwrap(), "false");
        assert_eq!(sonic_rs::to_string(&Value::<TestKey>::from(-7i64)).unwrap(), "-7");
        assert_eq!(sonic_rs::to_string(&Value::<TestKey>::from(42u64)).unwrap(), "42");
        assert_eq!(sonic_rs::to_string(&Value::<TestKey>::from("hi")).unwrap(),  "\"hi\"");
    }

    #[test]
    fn serialize_array() {
        let arr: SmallVec<[Box<Value<TestKey>>; 8]> = smallvec::smallvec![
            Box::new(Value::from(1i64)),
            Box::new(Value::from(2i64)),
        ];
        let v = Value::<TestKey>::Array(arr);
        assert_eq!(sonic_rs::to_string(&v).unwrap(), "[1,2]");
    }
}