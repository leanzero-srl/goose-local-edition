use super::*;
use crate::session::work_catalog::{ExternalWork, VERIFIED_LOG_APPEND};
use crate::work_control::command_revision::check_revision;

fn source(kind: &str) -> ExternalWork {
    ExternalWork {
        kind: kind.into(),
        locator: format!("/fixture/{kind}"),
        title: "External work".into(),
        working_dir: "/fixture".into(),
        state: "running".into(),
        run_key: Some("run-one".into()),
        updated_at: "2026-10-28T21:00:00Z".into(),
        event_kind: "running".into(),
        log_bytes: Some(1),
    }
}

async fn check(f: &Fixture, command: &WorkCommand, revision: i64) -> Result<(), ControllerError> {
    check_revision(
        &mut f.runs.pool().await.unwrap().acquire().await.unwrap(),
        command,
        revision,
    )
    .await
}

fn command(
    f: &Fixture,
    row: &crate::session::work_catalog::CatalogRecord,
    kind: WorkKind,
    action: ActionPayload,
) -> WorkCommand {
    WorkCommand {
        command_id: Uuid::new_v4(),
        work: WorkRef {
            owner_node_id: f.receiver.owner_node_id,
            kind,
            work_id: row.work_id.parse().unwrap(),
        },
        expected_store_generation: f.receiver.store_generation,
        expected_revision: row.revision as u64,
        expected_run_id: row.current_run_id.as_ref().map(|s| s.parse().unwrap()),
        action,
    }
}

#[tokio::test]
async fn external_known_growth_preserves_controls_and_other_work_does_not_interfere() {
    for (kind, wire, actions) in [
        (
            "agent_work",
            WorkKind::AgentWork,
            vec![
                ActionPayload::Note {
                    text: "note".into(),
                },
                ActionPayload::Stop {},
                ActionPayload::Pause {},
                ActionPayload::Resume {},
                ActionPayload::Tick {},
            ],
        ),
        (
            "benchmark",
            WorkKind::Benchmark,
            vec![
                ActionPayload::Note {
                    text: "note".into(),
                },
                ActionPayload::Stop {},
            ],
        ),
        (
            "shell_job",
            WorkKind::ShellJob,
            vec![ActionPayload::Stop {}],
        ),
    ] {
        let f = Fixture::new().await;
        let catalog = f.manager.work_catalog().await.unwrap();
        let mut source = source(kind);
        let anchor = catalog.upsert_external(&source).await.unwrap();
        source.log_bytes = Some(2);
        let first = catalog.upsert_external(&source).await.unwrap();
        assert_eq!(first.event_kind, VERIFIED_LOG_APPEND);
        let mut other = source.clone();
        other.locator.push_str("-other");
        other.title = "A semantic change elsewhere".into();
        catalog.upsert_external(&other).await.unwrap();
        source.log_bytes = Some(3);
        let current = catalog.upsert_external(&source).await.unwrap();
        assert_eq!(current.event_kind, VERIFIED_LOG_APPEND);
        assert_eq!(catalog.upsert_external(&source).await.unwrap(), current);
        for action in actions {
            check(&f, &command(&f, &anchor, wire, action), current.revision)
                .await
                .unwrap();
        }
        for action in [
            send("strict"),
            ActionPayload::AnswerInput {
                input_id: Uuid::new_v4(),
                option_id: "yes".into(),
                value: None,
            },
        ] {
            assert_eq!(
                check(&f, &command(&f, &anchor, wire, action), current.revision)
                    .await
                    .unwrap_err()
                    .body
                    .code,
                "stale_revision"
            );
        }
    }
}

#[tokio::test]
async fn simultaneous_semantic_changes_unknown_lengths_and_truncation_never_mint_proof() {
    for kind in ["agent_work", "benchmark", "shell_job"] {
        for variant in [
            "title",
            "working_dir",
            "state",
            "run",
            "source_event",
            "source_timestamp",
            "shorter",
            "unknown_before",
            "unknown_after",
            "reserved",
        ] {
            let f = Fixture::new().await;
            let catalog = f.manager.work_catalog().await.unwrap();
            let mut source = source(kind);
            if variant == "unknown_before" {
                source.log_bytes = None;
            }
            let anchor = catalog.upsert_external(&source).await.unwrap();
            source.log_bytes = Some(2);
            match variant {
                "title" => source.title.push_str(" changed"),
                "working_dir" => source.working_dir.push_str("/changed"),
                "state" => source.state = "paused".into(),
                "run" => source.run_key = Some("run-two".into()),
                "source_event" => source.event_kind = "new_phase".into(),
                "source_timestamp" => source.updated_at = "2026-10-28T22:00:00Z".into(),
                "shorter" => source.log_bytes = Some(0),
                "unknown_after" => source.log_bytes = None,
                "reserved" => source.event_kind = VERIFIED_LOG_APPEND.into(),
                _ => {}
            }
            if variant == "reserved" {
                assert!(catalog.upsert_external(&source).await.is_err());
                continue;
            }
            let changed = catalog.upsert_external(&source).await.unwrap();
            assert_ne!(changed.event_kind, VERIFIED_LOG_APPEND, "{kind} {variant}");
            // Later legitimate growth cannot hide an earlier semantic/unknown-byte event.
            source.log_bytes = Some(3);
            let current = catalog.upsert_external(&source).await.unwrap();
            let wire = match kind {
                "agent_work" => WorkKind::AgentWork,
                "benchmark" => WorkKind::Benchmark,
                _ => WorkKind::ShellJob,
            };
            assert_eq!(
                check(
                    &f,
                    &command(&f, &anchor, wire, ActionPayload::Stop {}),
                    current.revision
                )
                .await
                .unwrap_err()
                .body
                .code,
                "stale_revision",
                "{kind} {variant}"
            );
        }
    }
}

#[tokio::test]
async fn external_missing_anchors_old_ambiguous_events_and_metadata_corruption_stay_strict() {
    for variant in [
        "observed_missing",
        "current_missing",
        "future",
        "unknown",
        "wrong_run",
        "no_run",
        "old_marker",
        "metadata",
    ] {
        let f = Fixture::new().await;
        let catalog = f.manager.work_catalog().await.unwrap();
        let mut source = source("agent_work");
        let anchor = catalog.upsert_external(&source).await.unwrap();
        let mut cmd = command(&f, &anchor, WorkKind::AgentWork, ActionPayload::Stop {});
        source.log_bytes = Some(2);
        let middle = catalog.upsert_external(&source).await.unwrap();
        source.log_bytes = Some(3);
        let current = catalog.upsert_external(&source).await.unwrap();
        let pool = f.runs.pool().await.unwrap();
        match variant {
            "observed_missing" | "current_missing" => {
                sqlx::query("DELETE FROM external_catalog_versions WHERE revision=?")
                    .bind(if variant == "observed_missing" {
                        anchor.revision
                    } else {
                        current.revision
                    })
                    .execute(pool)
                    .await
                    .unwrap();
            }
            "future" => cmd.expected_revision = current.revision as u64 + 1,
            "unknown" => cmd.expected_revision = 0,
            "wrong_run" => cmd.expected_run_id = Some(Uuid::new_v4()),
            "no_run" => cmd.expected_run_id = None,
            "old_marker" => {
                sqlx::query("UPDATE external_catalog_versions SET event_kind='log_updated' WHERE revision=?").bind(middle.revision).execute(pool).await.unwrap();
            }
            "metadata" => {
                sqlx::query("UPDATE external_catalog_versions SET archived=1 WHERE revision=?")
                    .bind(middle.revision)
                    .execute(pool)
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            check(&f, &cmd, current.revision)
                .await
                .unwrap_err()
                .body
                .code,
            "stale_revision",
            "{variant}"
        );
    }
}
