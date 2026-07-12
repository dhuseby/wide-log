use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing_subscriber::fmt().init();

    let mut handles = vec![];
    for i in 0..10 {
        handles.push(tokio::spawn(handle_request(i)));
    }
    for h in handles {
        h.await.unwrap();
    }
}

async fn handle_request(id: u64) {
    scope_default(async {
        wl_set!("service.name", format!("worker-{id}"));
        wl_inc!("requests");
        info!("request {} started", id);

        // The task may be moved to another thread here.
        // task_local! ensures the event pointer moves with it.
        tokio::task::yield_now().await;

        info!("request {} completed", id);
    })
    .await;
    // guard drops → event emitted with duration.total_ms
}