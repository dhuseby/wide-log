pub trait Key: Copy + Eq + 'static {
    fn as_str(self) -> &'static str;
    const MAX_KEYS: usize;
    fn as_index(self) -> usize;
    const SUBSYSTEM_KEY: Self;
    const DURATION_NS_KEY: Self;
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::Key;

    #[derive(Copy, Clone, PartialEq, Eq, Debug)]
    #[repr(u8)]
    pub enum TestKey {
        Subsystem,
        DurationNs,
        UserId,
        Status,
        Details,
        Tag,
        Count,
        Flag,
    }

    impl Key for TestKey {
        fn as_str(self) -> &'static str {
            match self {
                TestKey::Subsystem  => "subsystem",
                TestKey::DurationNs => "duration_ns",
                TestKey::UserId     => "user_id",
                TestKey::Status     => "status",
                TestKey::Details    => "details",
                TestKey::Tag        => "tag",
                TestKey::Count      => "count",
                TestKey::Flag       => "flag",
            }
        }

        const MAX_KEYS: usize = 8;

        fn as_index(self) -> usize {
            self as usize
        }

        const SUBSYSTEM_KEY: Self = TestKey::Subsystem;
        const DURATION_NS_KEY: Self = TestKey::DurationNs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::TestKey;

    #[test]
    fn key_as_str() {
        assert_eq!(TestKey::Subsystem.as_str(),  "subsystem");
        assert_eq!(TestKey::DurationNs.as_str(), "duration_ns");
        assert_eq!(TestKey::UserId.as_str(),     "user_id");
        assert_eq!(TestKey::Status.as_str(),     "status");
        assert_eq!(TestKey::Details.as_str(),    "details");
        assert_eq!(TestKey::Tag.as_str(),        "tag");
        assert_eq!(TestKey::Count.as_str(),      "count");
        assert_eq!(TestKey::Flag.as_str(),       "flag");
    }

    #[test]
    fn key_as_index() {
        assert_eq!(TestKey::Subsystem.as_index(),  0);
        assert_eq!(TestKey::DurationNs.as_index(), 1);
        assert_eq!(TestKey::UserId.as_index(),     2);
        assert_eq!(TestKey::Status.as_index(),     3);
        assert_eq!(TestKey::Details.as_index(),    4);
        assert_eq!(TestKey::Tag.as_index(),        5);
        assert_eq!(TestKey::Count.as_index(),      6);
        assert_eq!(TestKey::Flag.as_index(),       7);
    }

    #[test]
    fn max_keys_correct() {
        assert_eq!(TestKey::MAX_KEYS, 8);
    }

    #[test]
    fn subsystem_and_duration_constants() {
        assert_eq!(TestKey::SUBSYSTEM_KEY,  TestKey::Subsystem);
        assert_eq!(TestKey::DURATION_NS_KEY, TestKey::DurationNs);
    }
}