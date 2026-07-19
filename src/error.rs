/// Errors that can occur during wide-event operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Serialization of a wide event to JSON failed.
    #[error("serialization failed: {0}")]
    Serialize(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let e = Error::Serialize("bad input".into());
        assert_eq!(e.to_string(), "serialization failed: bad input");
    }

    #[test]
    fn error_is_debug() {
        let e = Error::Serialize("y".into());
        let s = format!("{e:?}");
        assert!(s.contains("Serialize"));
    }
}
