use wide_log::wide_log;

wide_log!({
    "service": {
        "name": null,
        "version": "1.0.0",
    },
    "requests": counter!,
});

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt().init();

    handle_request().await;
}

async fn handle_request() {
    scope_default(async {
        wl_set!("service.name", "example-service");
        wl_inc!("requests");
        info!("request received");

        fetch_upstream().await;

        info!("request completed");
    })
    .await;
    // guard drops here → duration.total_ms set, event emitted
}

async fn fetch_upstream() {
    warn!("upstream slow");
}
