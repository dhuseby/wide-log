use std::ops::{Deref, DerefMut};
use std::time::Instant;

use chrono_tz::Tz;

use crate::key::Key;
use crate::wide_event::WideEvent;

/// RAII guard that owns a [`WideEvent`] and emits it on drop.
///
/// The guard starts a timer on creation. On drop, it sets the duration field
/// (via [`Key::DURATION_PATH`]) to the elapsed milliseconds, sets the
/// timestamp field (via [`Key::TIMESTAMP_PATH`]) to the current time as an
/// RFC 3339 string in the guard's timezone, and calls the emit function
/// with a reference to the event.
///
/// Implements [`Deref`] / [`DerefMut`] to [`WideEvent`] so you can call
/// `add`, `add_path`, `inc`, etc. directly on the guard.
///
/// The `wide_log!` macro generates a `WideLogGuard` that wraps this guard
/// and manages the thread-local / task-local pointer. Users typically
/// interact with `WideLogGuard`, not `ScopedGuard` directly.
///
/// [`WideEvent`]: crate::WideEvent
pub struct ScopedGuard<K: Key, F>
where
    F: FnOnce(&WideEvent<K>) + Send + 'static,
{
    event: WideEvent<K>,
    start: Instant,
    tz: Tz,
    emit_fn: Option<F>,
}

impl<K: Key, F> ScopedGuard<K, F>
where
    F: FnOnce(&WideEvent<K>) + Send + 'static,
{
    /// Creates a new guard with the given emit function and UTC timezone.
    ///
    /// The timer starts immediately. On drop, the guard sets
    /// `K::DURATION_PATH` to the elapsed milliseconds, sets
    /// `K::TIMESTAMP_PATH` to the current UTC time as RFC 3339, and
    /// calls `emit_fn`.
    pub fn new(emit_fn: F) -> Self {
        Self::new_with_tz(emit_fn, Tz::UTC)
    }

    /// Creates a new guard with the given emit function and timezone.
    ///
    /// The timezone is used to format the timestamp set on drop.
    pub fn new_with_tz(emit_fn: F, tz: Tz) -> Self {
        Self {
            event: WideEvent::new(),
            start: Instant::now(),
            tz,
            emit_fn: Some(emit_fn),
        }
    }

    /// Creates a new guard with the given emit function, timezone, and a
    /// type-conflict callback.
    ///
    /// The callback is fired when `object()` is called on a key that already
    /// has a non-object value. See [`WideEvent::new_with_warnings`].
    ///
    /// [`WideEvent::new_with_warnings`]: crate::WideEvent::new_with_warnings
    pub fn new_with_warnings(emit_fn: F, tz: Tz, on_type_conflict: crate::wide_event::ConflictFn<K>) -> Self {
        Self {
            event: WideEvent::new_with_warnings(on_type_conflict),
            start: Instant::now(),
            tz,
            emit_fn: Some(emit_fn),
        }
    }
}

impl<K: Key, F> Deref for ScopedGuard<K, F>
where
    F: FnOnce(&WideEvent<K>) + Send + 'static,
{
    type Target = WideEvent<K>;

    #[inline]
    fn deref(&self) -> &WideEvent<K> {
        &self.event
    }
}

impl<K: Key, F> DerefMut for ScopedGuard<K, F>
where
    F: FnOnce(&WideEvent<K>) + Send + 'static,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut WideEvent<K> {
        &mut self.event
    }
}

impl<K: Key, F> Drop for ScopedGuard<K, F>
where
    F: FnOnce(&WideEvent<K>) + Send + 'static,
{
    fn drop(&mut self) {
        let duration_ms = self.start.elapsed().as_millis() as u64;
        // Fast path for common 2-segment DURATION_PATH and TIMESTAMP_PATH.
        // Uses direct indexed writes instead of object() to avoid the overhead
        // of creating and returning &mut references through the ManuallyDrop chain.
        if K::DURATION_PATH.len() == 2 && K::TIMESTAMP_PATH.len() == 2 {
            let dur_idx = K::DURATION_PATH[0].as_index();
            let ts_idx = K::TIMESTAMP_PATH[0].as_index();
            self.event.ensure_capacity(dur_idx.max(ts_idx));

            // Create child objects if needed (before mutable borrow)
            let need_dur_child = match &self.event.values.get(dur_idx) {
                Some(Some(v)) => !v.is_object(),
                _ => true,
            };
            let need_ts_child = match &self.event.values.get(ts_idx) {
                Some(Some(v)) => !v.is_object(),
                _ => true,
            };
            if need_dur_child {
                let child = self.event.new_child();
                self.event.values[dur_idx] = Some(crate::value::Value::from_object(child));
            }
            if need_ts_child {
                let child = self.event.new_child();
                self.event.values[ts_idx] = Some(crate::value::Value::from_object(child));
            }

            // Now write the leaf values
            if let Some(Some(v)) = self.event.values.get_mut(dur_idx) {
                unsafe { &mut **v.data.object }.add(K::DURATION_PATH[1], duration_ms);
            }
            let now = chrono::Utc::now().with_timezone(&self.tz);
            if let Some(Some(v)) = self.event.values.get_mut(ts_idx) {
                unsafe { &mut **v.data.object }.add(K::TIMESTAMP_PATH[1], now.to_rfc3339());
            }
        } else {
            self.event.add_path(K::DURATION_PATH, duration_ms);
            let now = chrono::Utc::now().with_timezone(&self.tz);
            self.event.add_path(K::TIMESTAMP_PATH, now.to_rfc3339());
        }
        if let Some(emit) = self.emit_fn.take() {
            emit(&self.event);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::key::test_support::TestKey;

    type CaptureSlot = Arc<Mutex<Option<String>>>;

    fn capture_json() -> (
        CaptureSlot,
        impl FnOnce(&WideEvent<TestKey>) + Send + 'static,
    ) {
        let slot: CaptureSlot = Arc::new(Mutex::new(None));
        let slot_clone = slot.clone();
        let emit = move |we: &WideEvent<TestKey>| {
            *slot_clone.lock().unwrap() = Some(we.to_json().unwrap());
        };
        (slot, emit)
    }

    #[test]
    fn guard_sets_duration_path() {
        let (slot, emit) = capture_json();
        drop(ScopedGuard::<TestKey, _>::new(emit));
        let json = slot.lock().unwrap().clone().unwrap();
        assert!(json.contains("\"duration\""));
        assert!(json.contains("\"total_ms\""));
    }

    #[test]
    fn guard_sets_timestamp() {
        let (slot, emit) = capture_json();
        drop(ScopedGuard::<TestKey, _>::new(emit));
        let json = slot.lock().unwrap().clone().unwrap();
        let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        use sonic_rs::JsonValueTrait;
        let ts = parsed["event"]["timestamp"].as_str().unwrap();
        assert!(!ts.is_empty(), "timestamp should not be empty");
        assert!(
            ts.contains('T'),
            "timestamp should be RFC 3339 (contain 'T'): {ts}"
        );
    }

    #[test]
    fn guard_deref_add() {
        let (slot, emit) = capture_json();
        let mut g = ScopedGuard::<TestKey, _>::new(emit);
        g.add(TestKey::Status, "ok");
        drop(g);
        let json = slot.lock().unwrap().clone().unwrap();
        assert!(json.contains(r#""status":"ok""#));
    }

    #[test]
    fn guard_emit_called_exactly_once() {
        let counter = Arc::new(Mutex::new(0u32));
        let c = counter.clone();
        let emit = move |_: &WideEvent<TestKey>| {
            *c.lock().unwrap() += 1;
        };
        drop(ScopedGuard::<TestKey, _>::new(emit));
        assert_eq!(*counter.lock().unwrap(), 1);
    }

    #[test]
    fn guard_new_with_warnings_fires_callback() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        COUNTER.store(0, Ordering::SeqCst);

        fn cb(_event: &mut WideEvent<TestKey>, _key: TestKey) {
            COUNTER.fetch_add(1, Ordering::SeqCst);
        }

        let (slot, emit) = capture_json();
        let mut g = ScopedGuard::<TestKey, _>::new_with_warnings(emit, Tz::UTC, cb);
        g.add(TestKey::Details, true);
        g.object(TestKey::Details);
        drop(g);
        assert_eq!(COUNTER.load(Ordering::SeqCst), 1);
        assert!(slot.lock().unwrap().is_some());
    }

    #[test]
    fn guard_new_with_warnings_callback_can_mutate_event() {
        let (slot, emit) = capture_json();
        let mut g = ScopedGuard::<TestKey, _>::new_with_warnings(emit, Tz::UTC, |event, _key| {
            event.add(TestKey::Flag, true);
        });
        g.add(TestKey::Details, 42u64);
        g.object(TestKey::Details);
        drop(g);
        let json = slot.lock().unwrap().clone().unwrap();
        assert!(json.contains("\"flag\":true"));
    }

    #[test]
    fn guard_new_with_warnings_callback_appends_warning_string() {
        use crate::value::Value;
        use smallvec::smallvec;

        let (slot, emit) = capture_json();
        let mut g = ScopedGuard::<TestKey, _>::new_with_warnings(emit, Tz::UTC, |event, key| {
            let warning = format!("{} type conflict", key.as_str());
            let idx = TestKey::Tag.as_index();
            // Check if there's already an array at Tag
            let existing = event.values.get(idx).and_then(|v| v.as_ref());
            if let Some(v) = existing {
                if let Some(arr) = v.as_array_ref() {
                    let mut new_arr = arr.clone();
                    new_arr.push(Value::from(warning));
                    event.add(TestKey::Tag, Value::from_array(new_arr));
                    return;
                }
            }
            event.add(TestKey::Tag, Value::from_array(smallvec![Value::from(warning)]));
        });
        g.add(TestKey::Details, true);
        g.object(TestKey::Details);
        drop(g);
        let json = slot.lock().unwrap().clone().unwrap();
        assert!(
            json.contains("\"details type conflict\""),
            "warning string should appear in JSON"
        );
    }

    #[test]
    fn guard_emit_not_called_on_forget() {
        let counter = Arc::new(Mutex::new(0u32));
        let c = counter.clone();
        let emit = move |_: &WideEvent<TestKey>| {
            *c.lock().unwrap() += 1;
        };
        let g = ScopedGuard::<TestKey, _>::new(emit);
        std::mem::forget(g);
        assert_eq!(*counter.lock().unwrap(), 0);
    }

    #[test]
    fn guard_duration_is_milliseconds() {
        let (slot, emit) = capture_json();
        let g = ScopedGuard::<TestKey, _>::new(emit);
        std::thread::sleep(std::time::Duration::from_millis(2));
        drop(g);
        let json = slot.lock().unwrap().clone().unwrap();
        let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        use sonic_rs::JsonValueTrait;
        let total_ms = parsed["duration"]["total_ms"].as_u64().unwrap();
        assert!(
            total_ms >= 1,
            "duration.total_ms should be >= 1, got {total_ms}"
        );
    }

    #[test]
    fn guard_timestamp_reflects_timezone() {
        let (slot, emit) = capture_json();
        let g = ScopedGuard::<TestKey, _>::new_with_tz(emit, Tz::America__New_York);
        drop(g);
        let json = slot.lock().unwrap().clone().unwrap();
        let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        use sonic_rs::JsonValueTrait;
        let ts = parsed["event"]["timestamp"].as_str().unwrap();
        assert!(
            ts.contains("-04:00") || ts.contains("-05:00"),
            "timestamp should reflect America/New_York offset: {ts}"
        );
    }
}