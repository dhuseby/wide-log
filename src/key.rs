/// Trait for wide-event keys.
///
/// Each variant of a key enum represents a JSON key in the wide event.
/// The `wide_log!` macro generates the enum and this trait impl
/// automatically from the JSON structure — users do not implement this
/// trait manually.
///
/// # Path-Based Duration / Timestamp / ID
///
/// [`DURATION_PATH`](Key::DURATION_PATH) specifies the nested path to the
/// duration field (e.g. `[Duration, TotalMs]`). The guard auto-sets this
/// field to the elapsed time in milliseconds on drop.
///
/// [`TIMESTAMP_PATH`](Key::TIMESTAMP_PATH) specifies the nested path to the
/// timestamp field (e.g. `[Event, Timestamp]`). The guard auto-sets this
/// field to the current time as an RFC 3339 string on drop.
///
/// [`ID_PATH`](Key::ID_PATH) specifies the nested path to the id field
/// (e.g. `[Event, Id]`). The builder auto-sets this field to the generated
/// ID string on `build()`.
///
/// The user does **not** define `Log`, `Level`, or `Message` variants. Log
/// entries are handled entirely internally by a `LogEntry` type and the
/// `log_entries` field on [`WideEvent`].
///
/// [`WideEvent`]: crate::WideEvent
pub trait Key: Copy + Eq + 'static {
    /// Returns the JSON string representation of this key.
    fn as_str(self) -> &'static str;

    /// The total number of keys in the enum.
    const MAX_KEYS: usize;

    /// All keys in enum order, indexed by `as_index`.
    const KEYS: &'static [Self];

    /// All key strings in enum order, indexed by `as_index`.
    /// Used for O(1) `as_str()` via array index instead of a match.
    const KEY_STRS: &'static [&'static str];

    /// Returns the discriminant index of this key.
    fn as_index(self) -> usize;

    /// Path to the duration field (e.g. `[Duration, TotalMs]`).
    /// Auto-set by the guard on drop. The value is an `f64` in the unit
    /// indicated by the suffix of the last path segment's key name
    /// (e.g. `_s`, `_ms`, `_ns`); an unrecognized suffix defaults to
    /// milliseconds.
    const DURATION_PATH: &'static [Self];

    /// Path to the timestamp field (e.g. `[Event, Timestamp]`).
    /// Auto-set by the guard on drop. The value is an RFC 3339 string.
    const TIMESTAMP_PATH: &'static [Self];

    /// Path to the id field (e.g. `[Event, Id]`).
    /// Auto-set by the builder on `build()`. The value is a string.
    const ID_PATH: &'static [Self];

    /// JSON key for the log entries array.
    /// Override path: `Log`. Default: "log".
    const LOG_KEY: &'static str;

    /// JSON key for the level field within each log entry.
    /// Override path: `Log.Level`. Default: "level".
    const LEVEL_KEY: &'static str;

    /// JSON key for the message field within each log entry.
    /// Override path: `Log.Message`. Default: "message".
    const MESSAGE_KEY: &'static str;
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::Key;

    #[derive(Copy, Clone, PartialEq, Eq, Debug)]
    #[repr(u8)]
    pub enum TestKey {
        Duration,
        TotalMs,
        Event,
        Timestamp,
        Id,
        Service,
        Name,
        Version,
        Requests,
        Status,
        Details,
        Tag,
        Count,
        Flag,
    }

    impl Key for TestKey {
        fn as_str(self) -> &'static str {
            match self {
                TestKey::Duration => "duration",
                TestKey::TotalMs => "total_ms",
                TestKey::Event => "event",
                TestKey::Timestamp => "timestamp",
                TestKey::Id => "id",
                TestKey::Service => "service",
                TestKey::Name => "name",
                TestKey::Version => "version",
                TestKey::Requests => "requests",
                TestKey::Status => "status",
                TestKey::Details => "details",
                TestKey::Tag => "tag",
                TestKey::Count => "count",
                TestKey::Flag => "flag",
            }
        }

        const MAX_KEYS: usize = 14;

        const KEYS: &'static [Self] = &[
            TestKey::Duration,
            TestKey::TotalMs,
            TestKey::Event,
            TestKey::Timestamp,
            TestKey::Id,
            TestKey::Service,
            TestKey::Name,
            TestKey::Version,
            TestKey::Requests,
            TestKey::Status,
            TestKey::Details,
            TestKey::Tag,
            TestKey::Count,
            TestKey::Flag,
        ];

        const KEY_STRS: &'static [&'static str] = &[
            "duration",
            "total_ms",
            "event",
            "timestamp",
            "id",
            "service",
            "name",
            "version",
            "requests",
            "status",
            "details",
            "tag",
            "count",
            "flag",
        ];

        fn as_index(self) -> usize {
            self as usize
        }

        const DURATION_PATH: &'static [Self] = &[TestKey::Duration, TestKey::TotalMs];
        const TIMESTAMP_PATH: &'static [Self] = &[TestKey::Event, TestKey::Timestamp];
        const ID_PATH: &'static [Self] = &[TestKey::Event, TestKey::Id];
        const LOG_KEY: &'static str = "log";
        const LEVEL_KEY: &'static str = "level";
        const MESSAGE_KEY: &'static str = "message";
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::TestKey;

    #[test]
    fn key_as_str() {
        assert_eq!(TestKey::Duration.as_str(), "duration");
        assert_eq!(TestKey::TotalMs.as_str(), "total_ms");
        assert_eq!(TestKey::Event.as_str(), "event");
        assert_eq!(TestKey::Timestamp.as_str(), "timestamp");
        assert_eq!(TestKey::Id.as_str(), "id");
        assert_eq!(TestKey::Service.as_str(), "service");
        assert_eq!(TestKey::Name.as_str(), "name");
        assert_eq!(TestKey::Version.as_str(), "version");
        assert_eq!(TestKey::Requests.as_str(), "requests");
        assert_eq!(TestKey::Status.as_str(), "status");
        assert_eq!(TestKey::Details.as_str(), "details");
        assert_eq!(TestKey::Tag.as_str(), "tag");
        assert_eq!(TestKey::Count.as_str(), "count");
        assert_eq!(TestKey::Flag.as_str(), "flag");
    }

    #[test]
    fn key_as_index() {
        assert_eq!(TestKey::Duration.as_index(), 0);
        assert_eq!(TestKey::TotalMs.as_index(), 1);
        assert_eq!(TestKey::Event.as_index(), 2);
        assert_eq!(TestKey::Timestamp.as_index(), 3);
        assert_eq!(TestKey::Id.as_index(), 4);
        assert_eq!(TestKey::Service.as_index(), 5);
        assert_eq!(TestKey::Name.as_index(), 6);
        assert_eq!(TestKey::Version.as_index(), 7);
        assert_eq!(TestKey::Requests.as_index(), 8);
        assert_eq!(TestKey::Status.as_index(), 9);
        assert_eq!(TestKey::Details.as_index(), 10);
        assert_eq!(TestKey::Tag.as_index(), 11);
        assert_eq!(TestKey::Count.as_index(), 12);
        assert_eq!(TestKey::Flag.as_index(), 13);
    }

    #[test]
    fn max_keys_correct() {
        assert_eq!(TestKey::MAX_KEYS, 14);
        assert_eq!(TestKey::KEYS.len(), 14);
    }

    #[test]
    fn duration_path_correct() {
        assert_eq!(
            TestKey::DURATION_PATH,
            &[TestKey::Duration, TestKey::TotalMs]
        );
    }

    #[test]
    fn timestamp_path_correct() {
        assert_eq!(
            TestKey::TIMESTAMP_PATH,
            &[TestKey::Event, TestKey::Timestamp]
        );
    }

    #[test]
    fn id_path_correct() {
        assert_eq!(TestKey::ID_PATH, &[TestKey::Event, TestKey::Id]);
    }
}
