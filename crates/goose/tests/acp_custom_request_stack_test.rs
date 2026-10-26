//! A custom ACP request answers on a stock 2 MiB tokio worker stack, in a debug build too.
//!
//! On 2026-10-26 the desktop's debug `goose serve` aborted with "tokio-rt-worker has overflowed its
//! stack" on its first `_goose/unstable/providers/config/status` request. The generated
//! `handle_custom_request` carried every handler's future in one poll function: its stack frame
//! measured 1.8 MB, and the provider registry's first regex compile on top went past 2 MiB. Each
//! arm now awaits a boxed future. This test drives that request the way the desktop does (a
//! window on a real websocket to the real ACP router) on workers with the default stack size, so a
//! frame that grows back past the stack aborts the test process.

#[path = "acp_ws/mod.rs"]
mod acp_ws;

use acp_ws::{configure, serve, Model, Window};
use serde_json::json;

#[test]
fn provider_config_status_answers_on_a_default_worker_stack() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(2 * 1024 * 1024)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let model = Model::start(Vec::new()).await;
        configure(&model, false);
        let addr = serve().await;
        let mut window = Window::open(addr, true).await;
        let status = window
            .request(
                "_goose/unstable/providers/config/status",
                json!({"providerIds": [], "checkConnections": false}),
            )
            .await;
        assert!(status.is_object(), "a status object comes back: {status}");
        window.close().await;
    });
}
