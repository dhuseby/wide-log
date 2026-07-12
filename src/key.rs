/// Trait for wide-event keys.
///
/// Each variant of a key enum represents a JSON key in the wide event.
/// The `wide_log!` macro generates the enum and this trait impl
/// automatically from the JSON structure — users do not implement this
/// trait manually.
///
/// # Path-Based Duration
///
/// [`DURATION_PATH`](Key::DURATION_PATH) specifies the nested path to the
/// duration field (e.g. `[Duration, TotalMs]`). The guard auto-sets this
/// field to the elapsed time in milliseconds on drop.
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

    /// Returns the discriminant index of this key.
    fn as_index(self) -> usize;

    /// Path to the duration field (e.g. `[Duration, TotalMs]`).
    /// Auto-set by the guard on drop. The value is in milliseconds (u64).
    const DURATION_PATH: &'static [Self];
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::Key;

    #[derive(Copy, Clone, PartialEq, Eq, Debug)]
    #[repr(u8)]
    pub enum TestKey {
        Duration,
        TotalMs,
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

        const MAX_KEYS: usize = 11;

        fn as_index(self) -> usize {
            self as usize
        }

        const DURATION_PATH: &'static [Self] = &[TestKey::Duration, TestKey::TotalMs];
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
        assert_eq!(TestKey::Service.as_index(), 2);
        assert_eq!(TestKey::Name.as_index(), 3);
        assert_eq!(TestKey::Version.as_index(), 4);
        assert_eq!(TestKey::Requests.as_index(), 5);
        assert_eq!(TestKey::Status.as_index(), 6);
        assert_eq!(TestKey::Details.as_index(), 7);
        assert_eq!(TestKey::Tag.as_index(), 8);
        assert_eq!(TestKey::Count.as_index(), 9);
        assert_eq!(TestKey::Flag.as_index(), 10);
    }

    #[test]
    fn max_keys_correct() {
        assert_eq!(TestKey::MAX_KEYS, 11);
    }

    #[test]
    fn duration_path_correct() {
        assert_eq!(
            TestKey::DURATION_PATH,
            &[TestKey::Duration, TestKey::TotalMs]
        );
    }
}
