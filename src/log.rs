use faststr::FastStr;
use serde::ser::{Serialize, SerializeMap, Serializer};

use crate::key::Key;

/// A log message — either a `&'static str` (zero-copy for literal messages)
/// or an owned `FastStr` (for formatted messages).
#[derive(Clone)]
pub(crate) enum LogMsg {
    Static(&'static str),
    Owned(FastStr),
}

impl LogMsg {
    #[inline]
    pub fn as_str(&self) -> &str {
        match self {
            LogMsg::Static(s) => s,
            LogMsg::Owned(s) => s.as_str(),
        }
    }
}

/// A single log entry accumulated in a wide event.
/// Serialized as: `{"level": "info", "message": "request received"}`
/// (the "level" and "message" keys are determined by `K::LEVEL_KEY` and
/// `K::MESSAGE_KEY`).
#[derive(Clone)]
pub(crate) struct LogEntry<K: Key> {
    pub level: &'static str,
    pub message: LogMsg,
    _marker: std::marker::PhantomData<K>,
}

impl<K: Key> LogEntry<K> {
    pub fn new(level: &'static str, message: LogMsg) -> Self {
        Self {
            level,
            message,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<K: Key> Serialize for LogEntry<K> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry(K::LEVEL_KEY, self.level)?;
        map.serialize_entry(K::MESSAGE_KEY, self.message.as_str())?;
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::test_support::TestKey;

    #[test]
    fn serialize_log_entry_owned() {
        let entry = LogEntry::<TestKey>::new(
            "info",
            LogMsg::Owned(FastStr::new("request received")),
        );
        let s = sonic_rs::to_string(&entry).unwrap();
        assert_eq!(s, r#"{"level":"info","message":"request received"}"#);
    }

    #[test]
    fn serialize_log_entry_static() {
        let entry = LogEntry::<TestKey>::new(
            "warn",
            LogMsg::Static("upstream slow"),
        );
        let s = sonic_rs::to_string(&entry).unwrap();
        assert_eq!(s, r#"{"level":"warn","message":"upstream slow"}"#);
    }

    #[test]
    fn log_msg_static_zero_copy() {
        let msg = LogMsg::Static("hello");
        assert_eq!(msg.as_str(), "hello");
        let ptr = match &msg {
            LogMsg::Static(s) => *s as *const str as *const () as usize,
            _ => 0,
        };
        assert!(ptr != 0, "static str should have a non-null pointer");
    }
}