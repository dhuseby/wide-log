use faststr::FastStr;
use serde::ser::{Serialize, SerializeMap, Serializer};

/// A single log entry accumulated in a wide event.
/// Serialized as: `{"level": "info", "message": "request received"}`
#[derive(Clone)]
pub(crate) struct LogEntry {
    pub level: &'static str,
    pub message: FastStr,
}

impl Serialize for LogEntry {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("level", self.level)?;
        map.serialize_entry("message", self.message.as_str())?;
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_log_entry() {
        let entry = LogEntry {
            level: "info",
            message: FastStr::new("request received"),
        };
        let s = sonic_rs::to_string(&entry).unwrap();
        assert_eq!(s, r#"{"level":"info","message":"request received"}"#);
    }

    #[test]
    fn serialize_log_entry_warn() {
        let entry = LogEntry {
            level: "warn",
            message: FastStr::new("upstream slow"),
        };
        let s = sonic_rs::to_string(&entry).unwrap();
        assert_eq!(s, r#"{"level":"warn","message":"upstream slow"}"#);
    }
}