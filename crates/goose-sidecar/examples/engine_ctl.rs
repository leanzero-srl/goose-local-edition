//! Operator tool for `evals/mlx-engine-bench`: mounts the engine through the SAME path goosed's
//! `_goose/unstable/mlxEngine/mount` takes (`MlxEngineManager::set_settings` → `mount` → the
//! supervised `Sidecar`), holds it until SIGINT/SIGTERM, then `unmount`s it through the
//! sidecar's own shutdown (SIGTERM, grace window, proven group kill, port released).
//!
//! Usage: cargo run -p goose-sidecar --example engine_ctl -- <settings.json>
//! `settings.json` is an `EngineSettings` (the `mlx_engine` config block as JSON); its
//! `model_id` is what gets mounted.

use std::time::Duration;

use anyhow::{Context, Result};
use goose_sidecar::engine::{build_serve_command, EngineSettings, MlxEngineManager};

#[tokio::main]
async fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .context("usage: engine_ctl <settings.json>")?;
    let raw = std::fs::read_to_string(&path).with_context(|| format!("reading {path}"))?;
    let settings: EngineSettings = serde_json::from_str(&raw).context("parsing EngineSettings")?;
    let model_id = settings
        .model_id
        .clone()
        .context("settings.model_id is required")?;

    let manager = MlxEngineManager::new();
    manager.set_settings(settings.clone());
    println!(
        "argv: {}",
        serde_json::to_string(&build_serve_command(&manager.settings(), &model_id)?)?
    );
    manager.mount(&model_id).await?;
    loop {
        let status = manager.status().await;
        match status.state.as_str() {
            "mounting" => tokio::time::sleep(Duration::from_secs(2)).await,
            "running" => {
                println!("running: {}", serde_json::to_string(&status)?);
                break;
            }
            other => anyhow::bail!("mount ended in state '{other}': {:?}", status.last_error),
        }
    }

    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate())?;
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    let started = std::time::Instant::now();
    manager.unmount().await;
    println!(
        "unmounted in {:.2}s: {}",
        started.elapsed().as_secs_f64(),
        serde_json::to_string(&manager.status().await)?
    );
    Ok(())
}
