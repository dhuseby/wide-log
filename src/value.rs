use faststr::FastStr;
use serde::ser::{Serialize, Serializer};
use smallvec::SmallVec;

use crate::key::Key;
use crate::wide_event::WideEvent;

/// Tag byte identifying the active variant in [`Value`].
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueTag {
    Null = 0,
    Bool = 1,
    I64 = 2,
    U64 = 3,
    F64 = 4,
    Str = 5,    // owned FastStr
    StaticStr = 6, // &'static str (zero-copy)
    Array = 7,  // Box<SmallVec<[Value; 8]>>
    Object = 8, // Box<WideEvent<K>>
}

/// Union holding the data for the active variant.
/// The largest member is `FastStr` at 32 bytes.
/// Drop-able types are wrapped in `ManuallyDrop` — the `Value`'s `Drop` impl
/// handles their cleanup based on the tag.
#[repr(C)]
pub(crate) union ValueData<K: Key> {
    pub(crate) b: bool,
    pub(crate) i: i64,
    pub(crate) u: u64,
    pub(crate) f: f64,
    pub(crate) s: std::mem::ManuallyDrop<FastStr>,
    pub(crate) static_str: &'static str,
    pub(crate) array: std::mem::ManuallyDrop<Box<SmallVec<[Value<K>; 8]>>>,
    pub(crate) object: std::mem::ManuallyDrop<Box<WideEvent<K>>>,
}

/// A JSON value stored in a wide event.
///
/// Uses a tag + union layout (`#[repr(C)]`) to minimize size. The struct is
/// 40 bytes — a 2x reduction from the previous 80-byte enum. The tag is a
/// single byte; the remaining 7 bytes are padding for the 8-byte-aligned
/// `FastStr` (32 bytes) in the union.
///
/// # Variants
///
/// - [`ValueTag::Null`] — JSON `null`
/// - [`ValueTag::Bool`] — `bool`
/// - [`ValueTag::I64`] — `i64`
/// - [`ValueTag::U64`] — `u64`
/// - [`ValueTag::F64`] — `f64`
/// - [`ValueTag::Str`] — owned string via `FastStr` (SSO for short strings)
/// - [`ValueTag::StaticStr`] — `&'static str` (zero-copy, zero-allocation)
/// - [`ValueTag::Array`] — `Box<SmallVec<[Value; 8]>>`
/// - [`ValueTag::Object`] — `Box<WideEvent<K>>`
///
/// # Conversions
///
/// All primitive types implement `Into<Value<K>>`:
///
/// - `bool` → `Bool`
/// - `i64` → `I64`
/// - `u64` → `U64`
/// - `f64` → `F64`
/// - `&'static str` → `StaticStr` (zero-copy)
/// - `&str` → `Str` (via `FastStr::new`, SSO for short strings)
/// - `String` → `Str` (via `FastStr::from_string`, takes ownership)
/// - `FastStr` → `Str`
/// - `()` → `Null`
#[repr(C)]
pub struct Value<K: Key> {
    tag: ValueTag,
    _pad: [u8; 7],
    pub(crate) data: ValueData<K>,
}

impl<K: Key> Value<K> {
    /// Creates an Object value from a WideEvent.
    #[inline]
    pub(crate) fn from_object(ev: WideEvent<K>) -> Self {
        Value { tag: ValueTag::Object, _pad: [0; 7], data: ValueData { object: std::mem::ManuallyDrop::new(Box::new(ev)) } }
    }

    /// Creates an Array value from a SmallVec of Values.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn from_array(arr: SmallVec<[Value<K>; 8]>) -> Self {
        Value { tag: ValueTag::Array, _pad: [0; 7], data: ValueData { array: std::mem::ManuallyDrop::new(Box::new(arr)) } }
    }

    /// Returns the tag identifying the active variant.
    #[inline]
    pub fn tag(&self) -> ValueTag {
        self.tag
    }

    /// Returns `true` if this value is an `Object`.
    #[inline]
    pub fn is_object(&self) -> bool {
        self.tag == ValueTag::Object
    }

    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        match self.tag {
            ValueTag::Str => Some(unsafe { &self.data.s }.as_str()),
            ValueTag::StaticStr => Some(unsafe { self.data.static_str }),
            _ => None,
        }
    }

    #[inline]
    pub fn as_u64(&self) -> Option<u64> {
        match self.tag {
            ValueTag::U64 => Some(unsafe { self.data.u }),
            _ => None,
        }
    }

    #[inline]
    pub fn as_i64(&self) -> Option<i64> {
        match self.tag {
            ValueTag::I64 => Some(unsafe { self.data.i }),
            _ => None,
        }
    }

    /// Returns a mutable reference to the Array if this value is an Array.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn as_array_mut(&mut self) -> Option<&mut SmallVec<[Value<K>; 8]>> {
        if self.tag == ValueTag::Array {
            Some(unsafe { &mut **self.data.array })
        } else {
            None
        }
    }

    /// Returns a reference to the Array if this value is an Array.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn as_array_ref(&self) -> Option<&SmallVec<[Value<K>; 8]>> {
        if self.tag == ValueTag::Array {
            Some(unsafe { &**self.data.array })
        } else {
            None
        }
    }
}

impl<K: Key> Clone for Value<K> {
    fn clone(&self) -> Self {
        let tag = self.tag;
        let data = match tag {
            ValueTag::Null => ValueData { b: false },
            ValueTag::Bool => ValueData { b: unsafe { self.data.b } },
            ValueTag::I64 => ValueData { i: unsafe { self.data.i } },
            ValueTag::U64 => ValueData { u: unsafe { self.data.u } },
            ValueTag::F64 => ValueData { f: unsafe { self.data.f } },
            ValueTag::Str => ValueData { s: std::mem::ManuallyDrop::new(unsafe { (*self.data.s).clone() }) },
            ValueTag::StaticStr => ValueData { static_str: unsafe { self.data.static_str } },
            ValueTag::Array => ValueData { array: std::mem::ManuallyDrop::new(unsafe { (*self.data.array).clone() }) },
            ValueTag::Object => ValueData { object: std::mem::ManuallyDrop::new(unsafe { (*self.data.object).clone() }) },
        };
        Value { tag, _pad: [0; 7], data }
    }
}

impl<K: Key> std::fmt::Debug for Value<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.tag {
            ValueTag::Null => f.write_str("Value::Null"),
            ValueTag::Bool => f.debug_tuple("Value::Bool").field(&unsafe { self.data.b }).finish(),
            ValueTag::I64 => f.debug_tuple("Value::I64").field(&unsafe { self.data.i }).finish(),
            ValueTag::U64 => f.debug_tuple("Value::U64").field(&unsafe { self.data.u }).finish(),
            ValueTag::F64 => f.debug_tuple("Value::F64").field(&unsafe { self.data.f }).finish(),
            ValueTag::Str => f.debug_tuple("Value::Str").field(&unsafe { &self.data.s }).finish(),
            ValueTag::StaticStr => f.debug_tuple("Value::StaticStr").field(&unsafe { self.data.static_str }).finish(),
            ValueTag::Array => f.debug_tuple("Value::Array").field(&unsafe { &self.data.array }).finish(),
            ValueTag::Object => f.debug_tuple("Value::Object").field(&unsafe { &self.data.object }).finish(),
        }
    }
}

impl<K: Key> Drop for Value<K> {
    fn drop(&mut self) {
        match self.tag {
            ValueTag::Str => unsafe { std::ptr::drop_in_place(&mut self.data.s) },
            ValueTag::Array => unsafe { std::ptr::drop_in_place(&mut self.data.array) },
            ValueTag::Object => unsafe { std::ptr::drop_in_place(&mut self.data.object) },
            _ => {}
        }
    }
}

impl<K: Key> Serialize for Value<K> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.tag {
            ValueTag::Null => serializer.serialize_unit(),
            ValueTag::Bool => serializer.serialize_bool(unsafe { self.data.b }),
            ValueTag::I64 => serializer.serialize_i64(unsafe { self.data.i }),
            ValueTag::U64 => serializer.serialize_u64(unsafe { self.data.u }),
            ValueTag::F64 => serializer.serialize_f64(unsafe { self.data.f }),
            ValueTag::Str => serializer.serialize_str(unsafe { &self.data.s }.as_str()),
            ValueTag::StaticStr => serializer.serialize_str(unsafe { self.data.static_str }),
            ValueTag::Array => unsafe { &*self.data.array }.serialize(serializer),
            ValueTag::Object => unsafe { &*self.data.object }.serialize(serializer),
        }
    }
}

// ── From impls ──

impl<K: Key> From<bool> for Value<K> {
    #[inline]
    fn from(b: bool) -> Self {
        Value { tag: ValueTag::Bool, _pad: [0; 7], data: ValueData { b } }
    }
}

impl<K: Key> From<i64> for Value<K> {
    #[inline]
    fn from(n: i64) -> Self {
        Value { tag: ValueTag::I64, _pad: [0; 7], data: ValueData { i: n } }
    }
}

impl<K: Key> From<u64> for Value<K> {
    #[inline]
    fn from(n: u64) -> Self {
        Value { tag: ValueTag::U64, _pad: [0; 7], data: ValueData { u: n } }
    }
}

impl<K: Key> From<f64> for Value<K> {
    #[inline]
    fn from(n: f64) -> Self {
        Value { tag: ValueTag::F64, _pad: [0; 7], data: ValueData { f: n } }
    }
}

impl<K: Key> From<&str> for Value<K> {
    #[inline]
    fn from(s: &str) -> Self {
        Value { tag: ValueTag::Str, _pad: [0; 7], data: ValueData { s: std::mem::ManuallyDrop::new(FastStr::new(s)) } }
    }
}

// Note: there is no `From<&'static str>` impl because it conflicts with `From<&str>`.
// Instead, `&'static str` literals go through `From<&str>` which uses `FastStr::new`
// (SSO). For true zero-copy `&'static str` storage, use `Value::from_static_str`.
impl<K: Key> Value<K> {
    /// Creates a `StaticStr` value from a `&'static str` — zero-copy, zero-allocation.
    #[inline]
    pub fn from_static_str(s: &'static str) -> Self {
        Value { tag: ValueTag::StaticStr, _pad: [0; 7], data: ValueData { static_str: s } }
    }
}

impl<K: Key> From<String> for Value<K> {
    #[inline]
    fn from(s: String) -> Self {
        Value { tag: ValueTag::Str, _pad: [0; 7], data: ValueData { s: std::mem::ManuallyDrop::new(FastStr::from_string(s)) } }
    }
}

impl<K: Key> From<FastStr> for Value<K> {
    #[inline]
    fn from(s: FastStr) -> Self {
        Value { tag: ValueTag::Str, _pad: [0; 7], data: ValueData { s: std::mem::ManuallyDrop::new(s) } }
    }
}

impl<K: Key> From<()> for Value<K> {
    #[inline]
    fn from(_: ()) -> Self {
        Value { tag: ValueTag::Null, _pad: [0; 7], data: ValueData { b: false } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::test_support::TestKey;

    #[test]
    fn from_bool() {
        assert!(matches!(Value::<TestKey>::from(true).tag(), ValueTag::Bool));
        assert!(unsafe { Value::<TestKey>::from(true).data.b });
        assert!(!unsafe { Value::<TestKey>::from(false).data.b });
    }

    #[test]
    fn from_i64() {
        let v = Value::<TestKey>::from(-42i64);
        assert_eq!(v.tag(), ValueTag::I64);
        assert_eq!(unsafe { v.data.i }, -42);
    }

    #[test]
    fn from_u64() {
        let v = Value::<TestKey>::from(99u64);
        assert_eq!(v.tag(), ValueTag::U64);
        assert_eq!(unsafe { v.data.u }, 99);
    }

    #[test]
    fn from_f64() {
        let v = Value::<TestKey>::from(3.15f64);
        assert_eq!(v.tag(), ValueTag::F64);
        assert_eq!(unsafe { v.data.f }, 3.15);
    }

    #[test]
    fn from_str_is_string() {
        let v = Value::<TestKey>::from("hello");
        assert_eq!(v.tag(), ValueTag::Str);
        assert_eq!(v.as_str(), Some("hello"));
    }

    #[test]
    fn from_static_str() {
        let v = Value::<TestKey>::from_static_str("world");
        assert_eq!(v.tag(), ValueTag::StaticStr);
        assert_eq!(v.as_str(), Some("world"));
    }

    #[test]
    fn from_owned_string_is_string() {
        let v = Value::<TestKey>::from("world".to_string());
        assert_eq!(v.tag(), ValueTag::Str);
        assert_eq!(v.as_str(), Some("world"));
    }

    #[test]
    fn from_unit_is_null() {
        let v = Value::<TestKey>::from(());
        assert_eq!(v.tag(), ValueTag::Null);
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
        assert_eq!(v2.tag(), ValueTag::Object);
        let json = sonic_rs::to_string(&v2).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
    }
}