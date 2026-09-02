//! Teardown of everything goosed supervises, run by `goose serve` when SIGTERM / SIGINT /
//! SIGHUP arrives and before the process exits.
//!
//! Why this exists (measured 2026-09-02 on the packaged app): the desktop stops goosed by
//! signalling its process GROUP and SIGKILLs after a grace; goosed had no signal handler,
//! so it died without stopping its children — and those are spawned with `process_group(0)`
//! into groups of their own, so the desktop's group signal never reaches them. Every
//! relaunch then found an orphaned bundled `tailscaled` on the mesh socket and an orphaned
//! `rapid-mlx serve` tree on the engine port, and the new goosed refused both by design
//! (it never adopts a daemon it did not spawn). The fix is for goosed to stop what it
//! supervises, per-pid, on its way out — this module is the sequence.
//!
//! The order is fixed: the mesh first, so peers see the node leave before its engine
//! disappears; the engine second. Every step reports what it did in one line, and a step
//! that has nothing to do says so — a silent step would be indistinguishable from a step
//! that never ran.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::info;

/// One thing goosed supervises that must not outlive it.
#[async_trait]
pub trait SupervisedResource: Send + Sync {
    fn name(&self) -> &'static str;
    /// Tear the resource down and describe the outcome in one line.
    async fn teardown(&self) -> String;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeardownReport {
    pub resource: &'static str,
    pub outcome: String,
}

/// Run every teardown, sequentially, in the order given.
pub async fn teardown_in_order(resources: &[Arc<dyn SupervisedResource>]) -> Vec<TeardownReport> {
    let mut reports = Vec::with_capacity(resources.len());
    for resource in resources {
        let outcome = resource.teardown().await;
        info!(resource = resource.name(), %outcome, "goose serve: teardown");
        reports.push(TeardownReport {
            resource: resource.name(),
            outcome,
        });
    }
    reports
}

struct LinkMeshes;

#[async_trait]
impl SupervisedResource for LinkMeshes {
    fn name(&self) -> &'static str {
        "leanzero-link mesh"
    }
    async fn teardown(&self) -> String {
        super::link::shutdown_started_meshes().await
    }
}

struct MlxEngine;

#[async_trait]
impl SupervisedResource for MlxEngine {
    fn name(&self) -> &'static str {
        "mlx engine"
    }
    async fn teardown(&self) -> String {
        super::mlx_engine::shutdown_supervised_engine().await
    }
}

/// The production sequence: the mesh daemon, then the engine sidecar.
pub async fn teardown_supervised() -> Vec<TeardownReport> {
    let resources: Vec<Arc<dyn SupervisedResource>> =
        vec![Arc::new(LinkMeshes), Arc::new(MlxEngine)];
    teardown_in_order(&resources).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Recording {
        name: &'static str,
        log: Arc<Mutex<Vec<&'static str>>>,
    }

    #[async_trait]
    impl SupervisedResource for Recording {
        fn name(&self) -> &'static str {
            self.name
        }
        async fn teardown(&self) -> String {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            self.log.lock().unwrap().push(self.name);
            format!("{} torn down", self.name)
        }
    }

    #[tokio::test]
    async fn teardown_runs_every_resource_in_the_order_given_and_reports_each() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let resources: Vec<Arc<dyn SupervisedResource>> = vec![
            Arc::new(Recording {
                name: "mesh",
                log: log.clone(),
            }),
            Arc::new(Recording {
                name: "engine",
                log: log.clone(),
            }),
        ];

        let reports = teardown_in_order(&resources).await;

        assert_eq!(*log.lock().unwrap(), vec!["mesh", "engine"]);
        assert_eq!(
            reports,
            vec![
                TeardownReport {
                    resource: "mesh",
                    outcome: "mesh torn down".into()
                },
                TeardownReport {
                    resource: "engine",
                    outcome: "engine torn down".into()
                },
            ]
        );
    }

    /// With nothing supervised the production sequence still runs both steps and each
    /// says so — the report never has a hole where a step was skipped.
    #[tokio::test]
    async fn production_sequence_reports_both_steps_when_nothing_is_supervised() {
        let reports = teardown_supervised().await;
        let names: Vec<_> = reports.iter().map(|r| r.resource).collect();
        assert_eq!(names, vec!["leanzero-link mesh", "mlx engine"]);
        assert!(
            reports[0].outcome.contains("no mesh daemon"),
            "{}",
            reports[0].outcome
        );
        assert!(
            reports[1].outcome.contains("nothing supervised"),
            "{}",
            reports[1].outcome
        );
    }
}
