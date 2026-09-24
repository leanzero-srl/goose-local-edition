//! The placement planner: given a model and the Macs goose can reach, which way of running it is
//! best for what the owner wants (fastest chat, long documents, many requests) — deterministic,
//! from each Mac's measured facts, the model's own files and the speeds goose has measured.
//! Design: local-edition/mlx/DESIGN-PLACEMENT.md (approved 2026-09-24).

pub mod bench;
pub mod chip;
pub mod model;
pub mod planner;
pub mod predict;
pub mod store;

/// Resident bytes of whatever LISTENS on `port` (the single engine's python, found by its socket,
/// not by the uv wrapper's pid). The planner counts them as memory this Mac gets back when the
/// running model makes way for another placement.
pub async fn engine_resident_bytes(port: u16) -> anyhow::Result<u64> {
    let pids = crate::listening_pids(port).await?;
    anyhow::ensure!(!pids.is_empty(), "nothing listens on port {port}");
    let mut sys = sysinfo::System::new();
    let wanted: Vec<sysinfo::Pid> = pids.iter().map(|p| sysinfo::Pid::from_u32(*p)).collect();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&wanted),
        true,
        sysinfo::ProcessRefreshKind::nothing().with_memory(),
    );
    let mut total = 0;
    for pid in &wanted {
        let process = sys
            .process(*pid)
            .ok_or_else(|| anyhow::anyhow!("pid {pid} listening on {port} vanished"))?;
        total += process.memory();
    }
    Ok(total)
}
