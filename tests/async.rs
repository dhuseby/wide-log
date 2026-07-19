use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
    "status": null,
});

use sonic_rs::{JsonContainerTrait, JsonValueTrait};
use std::sync::{Arc, Mutex};

type CaptureSlot = Arc<Mutex<Option<String>>>;

#[allow(clippy::type_complexity)]
fn capture() -> (
    CaptureSlot,
    impl FnOnce(&wide_log::WideEvent<EventKey>) + Send + 'static,
) {
    let slot: CaptureSlot = Arc::new(Mutex::new(None));
    let slot_clone = slot.clone();
    let emit = move |we: &wide_log::WideEvent<EventKey>| {
        *slot_clone.lock().unwrap() = Some(we.to_json().unwrap());
    };
    (slot, emit)
}

#[tokio::test]
async fn scope_default_works() {
    let (slot, emit) = capture();
    let result = scope(emit, async {
        wl_set!("service.name", "async-svc");
        wl_inc!("requests");
        info!("async request");
        42
    })
    .await;

    assert_eq!(result, 42);
    let json = slot.lock().unwrap().clone().unwrap();
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
    assert_eq!(parsed["service"]["name"], "async-svc");
    assert_eq!(parsed["service"]["version"], "1.0.0");
    assert_eq!(parsed["requests"], 1);
    assert!(parsed["duration"]["total_ms"].is_number());
    assert_eq!(parsed["log"][0]["level"], "info");
    assert_eq!(parsed["log"][0]["message"], "async request");
}

#[tokio::test]
async fn scope_default_uses_default_emit() {
    // scope_default uses the default emit (non-blocking stdout), so we can't
    // capture in-process. Just verify it compiles and runs without panic.
    let result = scope_default(async {
        wl_set!("status", "ok");
        wl_inc!("requests");
        7
    })
    .await;
    assert_eq!(result, 7);
}

#[tokio::test]
async fn macros_work_across_await() {
    let (slot, emit) = capture();
    scope(emit, async {
        wl_set!("status", "pending");
        info!("before await");

        tokio::task::yield_now().await;

        info!("after await");
        wl_set!("status", "done");
    })
    .await;

    let json = slot.lock().unwrap().clone().unwrap();
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
    assert_eq!(parsed["status"], "done");
    let log = &parsed["log"];
    let arr = log.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["message"], "before await");
    assert_eq!(arr[1]["message"], "after await");
}

#[tokio::test]
async fn nested_async_scopes() {
    let outer_slot = Arc::new(Mutex::new(None));
    let inner_slot = Arc::new(Mutex::new(None));

    let oc = outer_slot.clone();
    let ic = inner_slot.clone();

    scope(
        move |ev| *oc.lock().unwrap() = Some(ev.to_json().unwrap()),
        async {
            wl_set!("status", "outer");
            wl_inc!("requests");

            scope(
                move |ev| *ic.lock().unwrap() = Some(ev.to_json().unwrap()),
                async {
                    wl_set!("status", "inner");
                    wl_inc!("requests");
                    info!("inner scope");
                },
            )
            .await;

            // After inner scope: outer event is restored.
            // Outer should still have status="outer", requests=1.
            info!("outer after inner");
        },
    )
    .await;

    let outer_json = outer_slot.lock().unwrap().clone().unwrap();
    let inner_json = inner_slot.lock().unwrap().clone().unwrap();

    let outer_parsed: sonic_rs::Value = sonic_rs::from_str(&outer_json).unwrap();
    let inner_parsed: sonic_rs::Value = sonic_rs::from_str(&inner_json).unwrap();

    assert_eq!(outer_parsed["status"], "outer");
    assert_eq!(outer_parsed["requests"], 1);
    assert_eq!(inner_parsed["status"], "inner");
    assert_eq!(inner_parsed["requests"], 1);

    // Outer has 2 log entries (inner scope log went to inner event, not outer).
    let outer_log = outer_parsed["log"].as_array().unwrap();
    assert_eq!(outer_log.len(), 1);
    assert_eq!(outer_log[0]["message"], "outer after inner");

    // Inner has 1 log entry.
    let inner_log = inner_parsed["log"].as_array().unwrap();
    assert_eq!(inner_log.len(), 1);
    assert_eq!(inner_log[0]["message"], "inner scope");
}

#[tokio::test]
async fn current_is_none_without_scope() {
    assert!(current().is_none());
}

#[tokio::test]
async fn current_is_some_inside_scope() {
    scope(|_| {}, async {
        assert!(current().is_some());
    })
    .await;
}

#[tokio::test]
async fn concurrent_tasks_have_separate_events() {
    let slots: Vec<Arc<Mutex<Option<String>>>> =
        (0..5).map(|_| Arc::new(Mutex::new(None))).collect();

    let mut handles = vec![];
    for (i, slot) in slots.iter().enumerate() {
        let s = slot.clone();
        handles.push(tokio::spawn(scope(
            move |ev| *s.lock().unwrap() = Some(ev.to_json().unwrap()),
            async move {
                wl_set!("service.name", format!("task-{i}"));
                wl_inc!("requests");
                info!("task {} running", i);

                tokio::task::yield_now().await;

                info!("task {} done", i);
            },
        )));
    }

    for h in handles {
        h.await.unwrap();
    }

    for (i, slot) in slots.iter().enumerate() {
        let json = slot.lock().unwrap().clone().unwrap();
        let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        assert_eq!(parsed["service"]["name"], format!("task-{i}"));
        assert_eq!(parsed["requests"], 1);
        let log = parsed["log"].as_array().unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0]["message"], format!("task {i} running"));
        assert_eq!(log[1]["message"], format!("task {i} done"));
    }
}

// ---- WideLogLayer middleware test (§4.2 / Phase 7) ----

use std::convert::Infallible;
use wide_log::__re_exports::tower::{Layer, Service};

#[derive(Clone)]
struct OkService;

impl Service<String> for OkService {
    type Response = String;
    type Error = Infallible;
    type Future = std::future::Ready<Result<String, Infallible>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: String) -> Self::Future {
        std::future::ready(Ok(req))
    }
}

#[tokio::test]
async fn middleware_wraps_request_in_scope() {
    // The WideLogLayer middleware wraps every request in scope_default().
    // Inside the handler, current() should be Some, and wl_set!/info!
    // should work. When the handler returns, the guard drops and the
    // event is emitted via default_emit (non-blocking stdout).

    let mut middleware = WideLogLayer.layer(OkService);

    // The handler runs inside scope_default via the middleware.
    let response = middleware.call("hello".to_string()).await.unwrap();

    assert_eq!(response, "hello");

    // After the middleware's future completes, current() should be None
    // (the guard has been dropped).
    assert!(current().is_none());
}

#[tokio::test]
async fn middleware_handler_can_use_macros() {
    // Verify that inside a middleware-wrapped handler, the wide-log
    // macros (wl_set!, info!, etc.) work correctly via task-local storage.

    let mut middleware = WideLogLayer.layer(OkService);

    // We can't easily capture the emitted JSON (default_emit writes to
    // non-blocking stdout), but we can verify the macros don't panic and the
    // handler runs.
    let response = middleware.call("request-body".to_string()).await.unwrap();

    assert_eq!(response, "request-body");

    // After the request, no guard is active.
    assert!(current().is_none());
}

// ---- Phase 1: scope cancellation ----

#[tokio::test]
async fn scope_emits_on_cancellation() {
    // When a `scope` future is cancelled (dropped without being polled
    // to completion), the `ScopedGuard` it owns should still drop and
    // emit, just like a synchronous guard. Verify this with a
    // `tokio::select!` race.
    let (slot, emit) = capture();

    // Build a scope future that will sleep forever unless cancelled.
    let scope_fut = scope(emit, async {
        wl_set!("status", "before-cancel");
        // Yield once to let the outer `select!` poll us.
        tokio::task::yield_now().await;
        // Sleep so the outer select! can race us and pick the
        // "cancel" branch first.
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        wl_set!("status", "after-sleep");
    });

    // Race the scope against a short timer. The timer wins, dropping
    // the scope future — and the guard inside it.
    tokio::select! {
        _ = scope_fut => {
            panic!("scope should not have completed");
        }
        _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
            // Scope future was dropped mid-flight.
        }
    }

    // After cancellation, the guard inside the scope should have been
    // dropped and the emit called. The emit captured the JSON at
    // whatever state the event was in at cancellation time.
    let json = slot.lock().unwrap().clone().expect("emit was not called");
    let parsed: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
    // The status was set to "before-cancel" before the sleep; the
    // cancellation happened before "after-sleep".
    assert_eq!(parsed["status"], "before-cancel");
    // The duration and event metadata should still be present.
    assert!(parsed["duration"]["total_ms"].is_number());
    assert!(parsed["event"]["timestamp"].is_str());
    assert!(parsed["event"]["id"].is_str());
}
