use std::ops::{Deref, DerefMut};
use std::time::Instant;

use crate::key::Key;
use crate::wide_event::WideEvent;

pub struct WideEventGuard<K: Key, F>
where
    F: FnOnce(&WideEvent<K>) + Send + 'static,
{
    event: WideEvent<K>,
    start: Instant,
    emit_fn: Option<F>,
}

impl<K: Key, F> WideEventGuard<K, F>
where
    F: FnOnce(&WideEvent<K>) + Send + 'static,
{
    pub fn new(emit_fn: F) -> Self {
        Self {
            event: WideEvent::new(),
            start: Instant::now(),
            emit_fn: Some(emit_fn),
        }
    }

    pub fn new_with_warnings<G>(emit_fn: F, on_type_conflict: G) -> Self
    where
        G: Fn(&mut WideEvent<K>, K) + Send + Sync + 'static,
    {
        Self {
            event: WideEvent::new_with_warnings(on_type_conflict),
            start: Instant::now(),
            emit_fn: Some(emit_fn),
        }
    }
}

impl<K: Key, F> Deref for WideEventGuard<K, F>
where
    F: FnOnce(&WideEvent<K>) + Send + 'static,
{
    type Target = WideEvent<K>;

    #[inline]
    fn deref(&self) -> &WideEvent<K> {
        &self.event
    }
}

impl<K: Key, F> DerefMut for WideEventGuard<K, F>
where
    F: FnOnce(&WideEvent<K>) + Send + 'static,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut WideEvent<K> {
        &mut self.event
    }
}

impl<K: Key, F> Drop for WideEventGuard<K, F>
where
    F: FnOnce(&WideEvent<K>) + Send + 'static,
{
    fn drop(&mut self) {
        let duration_ms = self.start.elapsed().as_millis() as u64;
        self.event.add_path(K::DURATION_PATH, duration_ms);
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

    fn capture_json() -> (CaptureSlot, impl FnOnce(&WideEvent<TestKey>) + Send + 'static) {
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
        drop(WideEventGuard::<TestKey, _>::new(emit));
        let json = slot.lock().unwrap().clone().unwrap();
        assert!(json.contains("\"duration\""));
        assert!(json.contains("\"total_ms\""));
    }

    #[test]
    fn guard_deref_add() {
        let (slot, emit) = capture_json();
        let mut g = WideEventGuard::<TestKey, _>::new(emit);
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
        drop(WideEventGuard::<TestKey, _>::new(emit));
        assert_eq!(*counter.lock().unwrap(), 1);
    }

    #[test]
    fn guard_new_with_warnings_fires_callback() {
        let counter = Arc::new(Mutex::new(0u32));
        let c = counter.clone();
        let (slot, emit) = capture_json();
        let mut g = WideEventGuard::<TestKey, _>::new_with_warnings(
            emit,
            move |_event, _key| { *c.lock().unwrap() += 1; },
        );
        g.add(TestKey::Details, true);
        g.object(TestKey::Details);
        drop(g);
        assert_eq!(*counter.lock().unwrap(), 1);
        assert!(slot.lock().unwrap().is_some());
    }

    #[test]
    fn guard_new_with_warnings_callback_can_mutate_event() {
        let (slot, emit) = capture_json();
        let mut g = WideEventGuard::<TestKey, _>::new_with_warnings(
            emit,
            |event, _key| { event.add(TestKey::Flag, true); },
        );
        g.add(TestKey::Details, 42u64);
        g.object(TestKey::Details);
        drop(g);
        let json = slot.lock().unwrap().clone().unwrap();
        assert!(json.contains("\"flag\":true"));
    }

    #[test]
    fn guard_new_with_warnings_callback_appends_warning_string() {
        use smallvec::smallvec;
        use crate::value::Value;

        let (slot, emit) = capture_json();
        let mut g = WideEventGuard::<TestKey, _>::new_with_warnings(
            emit,
            |event, key| {
                let warning = format!("{} type conflict", key.as_str());
                let entry = Box::new(Value::from(warning));
                for (k, v) in &mut event.entries {
                    if *k == TestKey::Tag
                        && let Value::Array(arr) = v {
                        arr.push(entry);
                        return;
                    }
                }
                event.entries.push((TestKey::Tag, Value::Array(smallvec![entry])));
            },
        );
        g.add(TestKey::Details, true);
        g.object(TestKey::Details);
        drop(g);
        let json = slot.lock().unwrap().clone().unwrap();
        assert!(json.contains("\"details type conflict\""), "warning string should appear in JSON");
    }

    #[test]
    fn guard_emit_not_called_on_forget() {
        let counter = Arc::new(Mutex::new(0u32));
        let c = counter.clone();
        let emit = move |_: &WideEvent<TestKey>| {
            *c.lock().unwrap() += 1;
        };
        let g = WideEventGuard::<TestKey, _>::new(emit);
        std::mem::forget(g);
        assert_eq!(*counter.lock().unwrap(), 0);
    }

    #[test]
    fn guard_duration_is_milliseconds() {
        let (slot, emit) = capture_json();
        let g = WideEventGuard::<TestKey, _>::new(emit);
        std::thread::sleep(std::time::Duration::from_millis(2));
        drop(g);
        let json = slot.lock().unwrap().clone().unwrap();
        let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        use sonic_rs::JsonValueTrait;
        let total_ms = parsed["duration"]["total_ms"].as_u64().unwrap();
        assert!(total_ms >= 1, "duration.total_ms should be >= 1, got {total_ms}");
    }
}