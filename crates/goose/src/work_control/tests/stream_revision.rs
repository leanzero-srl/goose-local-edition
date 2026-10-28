use super::*;
use crate::conversation::message::Message;

fn steer() -> ActionPayload {
    ActionPayload::Steer {
        content: vec![ControllerContent::Text {
            text: "Use the revised instruction".into(),
        }],
    }
}

async fn chunk(fixture: &Fixture, ticket: &RunTicket) {
    fixture
        .runs
        .record_stream_message(
            &fixture.session,
            &ticket.run_id.to_string(),
            &Message::assistant()
                .with_id("streaming-answer")
                .with_text("next chunk "),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn active_controls_deliver_across_real_chunks_before_admission_and_dispatch() {
    for action in [steer(), ActionPayload::Stop {}] {
        let fixture = Fixture::new().await;
        let ticket = fixture.reserve_run().await;
        chunk(&fixture, &ticket).await;
        let command = fixture.command(action).await;
        fixture
            .manager
            .create_session(
                fixture._dir.path().to_path_buf(),
                "Unrelated work".into(),
                crate::session::SessionType::User,
                crate::config::GooseMode::Auto,
            )
            .await
            .unwrap();
        chunk(&fixture, &ticket).await;
        let accepted = fixture.submit(command.clone()).await.unwrap();
        // This current-thread test takes the run lock before the newly spawned dispatcher
        // can run. The production chunk writer therefore commits between the two checks.
        chunk(&fixture, &ticket).await;
        let receipt = settled_receipt(&fixture, accepted.command_id).await;
        assert!(
            matches!(receipt.state, CommandState::Delivered),
            "{receipt:?}"
        );
        assert_eq!(receipt.run_id, Some(ticket.run_id));
        assert_eq!(receipt.revision, command.expected_revision);
        assert_eq!(
            ticket.cancel_token.is_cancelled(),
            matches!(command.action, ActionPayload::Stop {})
        );
        let duplicate = fixture.submit(command).await.unwrap();
        assert_eq!(duplicate.command_id, receipt.command_id);
        assert!(matches!(duplicate.state, CommandState::Delivered));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM owner_commands")
            .fetch_one(fixture.runs.pool().await.unwrap())
            .await
            .unwrap();
        assert_eq!(count, 1);
        fixture
            .runs
            .finish(&fixture.session, &ticket.run_id.to_string())
            .await;
    }
}

#[tokio::test]
async fn original_pre_message_snapshot_stays_stale_but_fresh_partial_snapshot_can_control() {
    let fixture = Fixture::new().await;
    let ticket = fixture.reserve_run().await;
    let original = fixture.command(steer()).await;
    fixture
        .manager
        .add_message(
            &fixture.session,
            &Message::user()
                .with_id("new-prompt")
                .with_text("Start listing"),
        )
        .await
        .unwrap();
    fixture
        .manager
        .update(&fixture.session)
        .user_provided_name("Active cloud test")
        .apply()
        .await
        .unwrap();
    chunk(&fixture, &ticket).await;
    assert_eq!(
        fixture.submit(original).await.unwrap_err().body.code,
        "stale_revision"
    );
    let refreshed = fixture.command(steer()).await;
    chunk(&fixture, &ticket).await;
    let accepted = fixture.submit(refreshed).await.unwrap();
    assert!(matches!(
        settled_receipt(&fixture, accepted.command_id).await.state,
        CommandState::Delivered
    ));
    fixture
        .runs
        .finish(&fixture.session, &ticket.run_id.to_string())
        .await;
}

#[tokio::test]
async fn semantic_events_are_not_hidden_by_later_partials() {
    for event in [
        "message_upsert",
        "message_deleted",
        "session_changed",
        "pending_input",
        "partial_discarded",
    ] {
        let fixture = Fixture::new().await;
        let ticket = fixture.reserve_run().await;
        chunk(&fixture, &ticket).await;
        let command = fixture.command(steer()).await;
        sqlx::query("UPDATE work_identity SET event_kind=? WHERE work_id=?")
            .bind(event)
            .bind(command.work.work_id.to_string())
            .execute(fixture.runs.pool().await.unwrap())
            .await
            .unwrap();
        chunk(&fixture, &ticket).await;
        assert_eq!(
            fixture.submit(command).await.unwrap_err().body.code,
            "stale_revision",
            "{event}"
        );
        assert!(!ticket.cancel_token.is_cancelled());
        fixture
            .runs
            .finish(&fixture.session, &ticket.run_id.to_string())
            .await;
    }
}

#[tokio::test]
async fn partial_proof_rejects_every_catalog_metadata_change_and_missing_join() {
    for mutation in [
        "UPDATE catalog_versions SET kind='agent_work' WHERE revision=?",
        "UPDATE catalog_versions SET local_locator='another-session' WHERE revision=?",
        "UPDATE catalog_versions SET title='changed' WHERE revision=?",
        "UPDATE catalog_versions SET working_dir='/different' WHERE revision=?",
        "UPDATE catalog_versions SET session_type='hidden' WHERE revision=?",
        "UPDATE catalog_versions SET state='waiting_input' WHERE revision=?",
        "UPDATE catalog_versions SET current_run_id=NULL WHERE revision=?",
        "UPDATE catalog_versions SET archived=1 WHERE revision=?",
        "UPDATE catalog_versions SET updated_at='changed' WHERE revision=?",
        "UPDATE catalog_versions SET deleted=1 WHERE revision=?",
        "UPDATE catalog_versions SET event_kind='message_upsert' WHERE revision=?",
        "UPDATE work_events SET event_kind='message_upsert' WHERE revision=?",
        "UPDATE work_events SET work_id='missing' WHERE revision=?",
        "DELETE FROM work_events WHERE revision=?",
        "UPDATE owner_partial_events SET run_id='different-run' WHERE revision=?",
        "UPDATE owner_partial_events SET work_id='different-work' WHERE revision=?",
        "DELETE FROM owner_partial_events WHERE revision=?",
    ] {
        let fixture = Fixture::new().await;
        let ticket = fixture.reserve_run().await;
        chunk(&fixture, &ticket).await;
        let command = fixture.command(steer()).await;
        chunk(&fixture, &ticket).await;
        let middle = fixture.command(steer()).await.expected_revision;
        chunk(&fixture, &ticket).await;
        let pool = fixture.runs.pool().await.unwrap();
        // A real different work keeps the foreign-key counterexample structurally valid.
        let mutation = if mutation.contains("work_events SET work_id") {
            let other = fixture
                .manager
                .create_session(
                    fixture._dir.path().to_path_buf(),
                    "Other".into(),
                    crate::session::SessionType::User,
                    crate::config::GooseMode::Auto,
                )
                .await
                .unwrap();
            let id = fixture
                .manager
                .work_catalog()
                .await
                .unwrap()
                .lookup_local_session(&other.id)
                .await
                .unwrap()
                .work_id;
            mutation.replace("'missing'", &format!("'{id}'"))
        } else {
            mutation.to_owned()
        };
        sqlx::query(&mutation)
            .bind(i64::try_from(middle).unwrap())
            .execute(pool)
            .await
            .unwrap();
        assert_eq!(
            fixture.submit(command).await.unwrap_err().body.code,
            "stale_revision",
            "{mutation}"
        );
        fixture
            .runs
            .finish(&fixture.session, &ticket.run_id.to_string())
            .await;
    }
}

#[tokio::test]
async fn unknown_future_and_pruned_anchors_do_not_authorize_controls() {
    for variant in [
        "unknown",
        "future",
        "observed_missing",
        "current_missing",
        "wrong_run",
        "no_run",
    ] {
        let fixture = Fixture::new().await;
        let ticket = fixture.reserve_run().await;
        chunk(&fixture, &ticket).await;
        let mut command = fixture.command(steer()).await;
        chunk(&fixture, &ticket).await;
        let current = fixture.command(steer()).await.expected_revision;
        match variant {
            "unknown" => command.expected_revision = 0,
            "future" => command.expected_revision = current + 1,
            "wrong_run" => command.expected_run_id = Some(Uuid::new_v4()),
            "no_run" => command.expected_run_id = None,
            _ => {
                let revision = if variant == "observed_missing" {
                    command.expected_revision
                } else {
                    current
                };
                let mut tx = fixture.runs.pool().await.unwrap().begin().await.unwrap();
                sqlx::query("DELETE FROM work_events WHERE revision=?")
                    .bind(revision as i64)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                sqlx::query("DELETE FROM catalog_versions WHERE revision=?")
                    .bind(revision as i64)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                tx.commit().await.unwrap();
            }
        }
        assert_eq!(
            fixture.submit(command).await.unwrap_err().body.code,
            if variant == "no_run" {
                "stale_run"
            } else {
                "stale_revision"
            },
            "{variant}"
        );
        fixture
            .runs
            .finish(&fixture.session, &ticket.run_id.to_string())
            .await;
    }
}

#[tokio::test]
async fn send_answers_and_generation_changes_keep_strict_revision_guards() {
    for action in [
        send("Do not queue from stale state"),
        ActionPayload::AnswerInput {
            input_id: Uuid::new_v4(),
            option_id: "yes".into(),
            value: None,
        },
    ] {
        let fixture = Fixture::new().await;
        let ticket = fixture.reserve_run().await;
        let command = fixture.command(action).await;
        chunk(&fixture, &ticket).await;
        assert_eq!(
            fixture.submit(command).await.unwrap_err().body.code,
            "stale_revision"
        );
        fixture
            .runs
            .finish(&fixture.session, &ticket.run_id.to_string())
            .await;
    }
    let fixture = Fixture::new().await;
    let ticket = fixture.reserve_run().await;
    let command = fixture.command(steer()).await;
    chunk(&fixture, &ticket).await;
    fixture
        .manager
        .work_catalog()
        .await
        .unwrap()
        .reset_generation(
            &command.expected_store_generation.to_string(),
            "Explicit regression-test owner recovery",
        )
        .await
        .unwrap();
    assert_eq!(
        fixture.submit(command).await.unwrap_err().body.code,
        "store_generation_mismatch"
    );
}

#[tokio::test]
async fn semantic_change_between_acceptance_and_dispatch_still_rejects() {
    let fixture = Fixture::new().await;
    let ticket = fixture.reserve_run().await;
    chunk(&fixture, &ticket).await;
    let command = fixture.command(steer()).await;
    let device = authorize(
        fixture.authority.as_ref(),
        "fixture-device",
        &fixture.receiver,
        ControllerScope::WorkSteer,
    )
    .await
    .unwrap();
    let guard = DispatchAuthorization::new(
        fixture.authority.clone(),
        "fixture-device".into(),
        fixture.receiver.clone(),
        ControllerScope::WorkSteer,
        device.clone(),
    );
    let entered = Arc::new(tokio::sync::Notify::new());
    let resume = Arc::new(tokio::sync::Notify::new());
    *fixture.authority.barrier.lock().await = Some((entered.clone(), resume.clone()));
    let accepted = fixture
        .runs
        .accept_command(&device, command, guard)
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    fixture
        .manager
        .update(&fixture.session)
        .user_provided_name("Changed while dispatch reauthorizes")
        .apply()
        .await
        .unwrap();
    resume.notify_one();
    let receipt = settled_receipt(&fixture, accepted.command_id).await;
    assert!(matches!(receipt.state, CommandState::Rejected));
    assert!(receipt.reason.unwrap().contains("stale_revision"));
    fixture
        .runs
        .finish(&fixture.session, &ticket.run_id.to_string())
        .await;
}
