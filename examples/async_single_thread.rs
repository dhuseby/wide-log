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
    handle_request().await;
}

async fn handle_request() {
    scope_default(async {
        // with_uuid() generates a UUIDv4 event id instead of the default
        // ULID (requires the `uuid` feature on wide-log).
        let _guard = WideLogGuard::builder().with_uuid().build();

        wl_set!("service.name", "example-service");
        wl_inc!("requests");
        info!("request received");

        fetch_upstream().await;

        info!("request completed");
        // guard drops → event emitted via default_emit (bare JSON to stdout).
    })
    .await;
    // Flush the non-blocking stdout writer so the line lands before exit.
    wide_log::stdout_emit::flush();
}

async fn fetch_upstream() {
    warn!("upstream slow");
}
