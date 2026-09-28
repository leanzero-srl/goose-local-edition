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
//! The order is fixed: the developer shell tool's in-flight commands first (Q-406 — each leads a
//! process group of its own now, so the desktop's signal to goosed's group no longer reaches
//! them; a goosed that dies without this sequence leaves them to its watchdog, Q-407
//! `developer::shell_watchdog`); then the stdio extension children (Q-138 — `std::process::exit` after
//! this sequence runs no destructor, so rmcp's own child cleanup never fired and every bundled
//! MCP outlived goosed; they are leaves nothing else depends on); then the peers are told this
//! goose is leaving, so they see the node go before its engine disappears; then the engines; the
//! mesh daemon LAST, because the distributed engine's stop reaches its peer rank over the mesh
//! (Q-242: with the daemon stopped second, every split's teardown said "rank 1 … cannot reach …
//! over LeanZero Link" — six of six goosed logs with a split up, 2026-09-27..28, SIGTERM and
//! stdin-EOF alike — and the peer's rank was left to notice on its own). Every step reports what
//! it did in one line, and a
//! step that has nothing to do says so — a silent step would be indistinguishable from a step
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

struct ShellCommands;

#[async_trait]
impl SupervisedResource for ShellCommands {
    fn name(&self) -> &'static str {
        "shell commands"
    }
    async fn teardown(&self) -> String {
        crate::agents::platform_extensions::developer::process_groups::terminate_live_commands()
            .await
    }
}

struct StdioExtensions;

#[async_trait]
impl SupervisedResource for StdioExtensions {
    fn name(&self) -> &'static str {
        "stdio extensions"
    }
    async fn teardown(&self) -> String {
        crate::agents::stdio_children::teardown_all().await
    }
}

struct LinkPeers;

#[async_trait]
impl SupervisedResource for LinkPeers {
    fn name(&self) -> &'static str {
        "leanzero-link peers"
    }
    async fn teardown(&self) -> String {
        super::link::announce_leaving_to_peers().await
    }
}

struct LinkMeshes;

#[async_trait]
impl SupervisedResource for LinkMeshes {
    fn name(&self) -> &'static str {
        "leanzero-link mesh"
    }
    async fn teardown(&self) -> String {
        super::link::stop_started_mesh_daemons().await
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

struct MlxDistributedEngine;

#[async_trait]
impl SupervisedResource for MlxDistributedEngine {
    fn name(&self) -> &'static str {
        "mlx distributed engine"
    }
    async fn teardown(&self) -> String {
        super::mlx_distributed::shutdown_distributed_engine().await
    }
}

/// The production sequence: every stdio extension child, the going-away notice to the peers,
/// the engine sidecar, the distributed engine's ranks (at most one of the two engines runs; the
/// other reports it has nothing), and the mesh daemon last — it carries the rank-1 stop.
pub async fn teardown_supervised() -> Vec<TeardownReport> {
    teardown_in_order(&production_sequence()).await
}

fn production_sequence() -> Vec<Arc<dyn SupervisedResource>> {
    vec![
        Arc::new(ShellCommands),
        Arc::new(StdioExtensions),
        Arc::new(LinkPeers),
        Arc::new(MlxEngine),
        Arc::new(MlxDistributedEngine),
        Arc::new(LinkMeshes),
    ]
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
        // The shell-commands step drains a process-wide registry: run here it would terminate
        // whatever shell test is mid-command in this same test binary. It is pinned first by
        // `in_flight_shell_commands_stop_first` and exercised on its own groups in
        // process_groups' tests.
        let sequence: Vec<_> = production_sequence()
            .into_iter()
            .filter(|r| r.name() != "shell commands")
            .collect();
        let reports = teardown_in_order(&sequence).await;
        let names: Vec<_> = reports.iter().map(|r| r.resource).collect();
        assert_eq!(
            names,
            vec![
                "stdio extensions",
                "leanzero-link peers",
                "mlx engine",
                "mlx distributed engine",
                "leanzero-link mesh",
            ]
        );
        assert!(
            reports[0].outcome.contains("no stdio extension children"),
            "{}",
            reports[0].outcome
        );
        assert!(
            reports[1].outcome.contains("peer"),
            "{}",
            reports[1].outcome
        );
        assert!(
            reports[2].outcome.contains("nothing supervised"),
            "{}",
            reports[2].outcome
        );
        assert!(
            reports[3].outcome.contains("nothing supervised"),
            "{}",
            reports[3].outcome
        );
        assert!(
            reports[4].outcome.contains("no mesh daemon"),
            "{}",
            reports[4].outcome
        );
    }

    /// Q-406: shell commands lead process groups of their own, so the desktop's signal to
    /// goosed's group no longer reaches them — goosed stops them itself, before anything else.
    #[test]
    fn in_flight_shell_commands_stop_first() {
        let names: Vec<_> = production_sequence().iter().map(|r| r.name()).collect();
        assert_eq!(names[0], "shell commands", "{names:?}");
    }

    /// Q-242: the mesh daemon outlives every step that talks to a peer over it — the going-away
    /// notice and the distributed engine's rank-1 stop. Stopping it second (the old order) made
    /// every split's rank-1 stop "cannot reach … over LeanZero Link".
    #[test]
    fn the_mesh_daemon_stops_after_every_step_that_reaches_a_peer() {
        let names: Vec<_> = production_sequence().iter().map(|r| r.name()).collect();
        let at = |name: &str| names.iter().position(|n| *n == name).unwrap();
        let mesh = at("leanzero-link mesh");
        assert_eq!(mesh, names.len() - 1, "{names:?}");
        assert!(at("leanzero-link peers") < mesh, "{names:?}");
        assert!(at("mlx distributed engine") < mesh, "{names:?}");
        // Peers hear the node is leaving before its engines go (a peer served by this Mac's
        // engine gets the notice, not a dead socket).
        assert!(at("leanzero-link peers") < at("mlx engine"), "{names:?}");
        assert!(
            at("leanzero-link peers") < at("mlx distributed engine"),
            "{names:?}"
        );
    }
}
