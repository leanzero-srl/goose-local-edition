//! Q-381: "Answer on {next} for now" rides the prompt as `_meta.goose.answerOn = {node}`. goosed
//! holds the ask for exactly that prompt's reply — the router reads it for the chat's route while
//! the reply runs — and drops it when the reply ends, however it ends, so the next prompt goes back
//! to the chat's lead. A mark that names no node is refused by name, never read as no mark.
//!
//! Driven the way the desktop drives it: a window on a real websocket to the real ACP router and a
//! scripted model. One goose and one model per process, one test at a time.

#[path = "acp_ws/mod.rs"]
mod acp_ws;

use std::future::Future;
use std::sync::LazyLock;

use acp_ws::{configure, eventually, serve, Answer, Model, Window};
use goose::nodes::answer_on::asked;
use serde_json::json;
use serial_test::serial;
use tokio::sync::OnceCell;

static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_stack_size(8 * 1024 * 1024)
        .enable_all()
        .build()
        .unwrap()
});

static GOOSE: OnceCell<(Model, std::net::SocketAddr)> = OnceCell::const_new();

fn run(test: impl Future<Output = ()>) {
    RUNTIME.block_on(test);
}

async fn goose() -> (Model, std::net::SocketAddr) {
    GOOSE
        .get_or_init(|| async {
            let model = Model::start(Vec::new()).await;
            configure(&model, false);
            (model, serve().await)
        })
        .await
        .clone()
}

#[test]
#[serial]
fn the_ask_lives_for_its_reply_and_goes_with_it() {
    run(async {
        let (model, addr) = goose().await;
        model.script(Vec::new());
        let mut window = Window::open(addr, true).await;
        let work = tempfile::tempdir().unwrap();
        let chat = window.new_chat(work.path()).await;
        assert_eq!(asked(&chat), None);

        let held_before = model.held();
        let prompt = window
            .prompt(
                &chat,
                "Summarise the release notes",
                Some(json!({"goose": {"answerOn": {"node": "flash-here"}}})),
            )
            .await;
        eventually("the reply's model call is held", || async {
            model.held() == held_before + 1
        })
        .await;
        assert_eq!(
            asked(&chat).as_deref(),
            Some("flash-here"),
            "the router reads the ask while the reply runs"
        );

        // The reply ends (its model call fails): the ask goes with it.
        model.script(vec![Answer::Finish("Done."); 3]);
        model.release_held();
        let _ = window.response(prompt).await;
        eventually("the ask is dropped with its reply", || async {
            asked(&chat).is_none()
        })
        .await;

        // The next prompt carries no mark: nothing is asked.
        let next = window.prompt(&chat, "And the changelog?", None).await;
        window
            .response(next)
            .await
            .expect("the next prompt answered");
        assert_eq!(asked(&chat), None);
        window.close().await;
    });
}

#[test]
#[serial]
fn a_mark_that_names_no_node_is_refused_by_name() {
    run(async {
        let (model, addr) = goose().await;
        model.script(vec![Answer::Finish("Done.")]);
        let mut window = Window::open(addr, true).await;
        let work = tempfile::tempdir().unwrap();
        let chat = window.new_chat(work.path()).await;
        let prompt = window
            .prompt(
                &chat,
                "Summarise the release notes",
                Some(json!({"goose": {"answerOn": {"node": ""}}})),
            )
            .await;
        let refused = window
            .response(prompt)
            .await
            .expect_err("a mark with no node is refused");
        assert!(
            refused
                .to_string()
                .contains("the answer-on mark names no node"),
            "{refused}"
        );
        assert_eq!(asked(&chat), None);
        window.close().await;
    });
}
