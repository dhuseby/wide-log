use wide_log::wide_log;

wide_log!({
    "service": { "name": null, "version": "1.0.0" },
    "requests": counter!,
});

fn main() {
    let _guard = WideLogGuard::builder()
        .with_emit(|ev| {
            if let Ok(json) = ev.to_json() {
                println!("{json}");
            }
        })
        .build();

    wl_set!("service.name", "example-service");
    wl_inc!("requests");
    info!("request received");
}
