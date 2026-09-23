//! Measure a real model copy between two machines with the shipped replica code, no goosed
//! and no mesh: the sender offers a model dir on ONE address, the receiver pulls it.
//!
//! ```text
//! sender$   replica_probe serve <bind-ip> <model-root> <publisher/name>
//!           → prints `source=<url>` and `token=<hex>`, serves until the receiver releases
//! receiver$ replica_probe pull <source-url> <token> <publisher/name> <models-dir>
//!           → prints the final progress as JSON and the model's `list_local_models` row
//! ```
//!
//! The token travels between the two by hand here; goose sends it inside the
//! node-authenticated `/v1/swarm/mlx/replicaPull` call.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use leanzero_link::netpath::LinkKind;
use leanzero_link::replica::{
    PullSpec, ReplicaListener, ReplicaOffers, ReplicaState, ReplicaTracker,
};

#[tokio::main]
async fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["serve", ip, root, model] => serve(ip, root, model).await,
        ["pull", source, token, model, models_dir] => pull(source, token, model, models_dir).await,
        _ => Err(
            "usage: replica_probe serve <bind-ip> <model-root> <publisher/name> | \
                  pull <source-url> <token> <publisher/name> <models-dir>"
                .to_string(),
        ),
    }
}

async fn serve(ip: &str, root: &str, model: &str) -> Result<(), String> {
    let ip: IpAddr = ip.parse().map_err(|e| format!("bind ip: {e}"))?;
    let offers = ReplicaOffers::new();
    let ticket = offers.offer(model, &PathBuf::from(root))?;
    let listener = ReplicaListener::bind(SocketAddr::new(ip, 0), offers.clone())
        .await
        .map_err(|e| format!("bind {ip}: {e}"))?;
    println!("source={}", listener.base_url());
    println!("token={}", ticket.token);
    println!("bytes={}", ticket.manifest.total_bytes);
    while offers.active() > 0 {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    println!("released");
    Ok(())
}

async fn pull(source: &str, token: &str, model: &str, models_dir: &str) -> Result<(), String> {
    let tracker = ReplicaTracker::new();
    let models_dir = PathBuf::from(models_dir);
    tracker.start(
        PullSpec {
            model_id: model.to_string(),
            source_url: source.to_string(),
            offer_token: token.to_string(),
            link: LinkKind::Thunderbolt,
            link_detail: format!("probe from {source}"),
        },
        &models_dir,
        Arc::new(|_| Ok(())),
    )?;
    let progress = loop {
        let progress = tracker.progress(model).expect("tracked");
        if !matches!(progress.state, ReplicaState::Queued | ReplicaState::Copying) {
            break progress;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&progress).map_err(|e| e.to_string())?
    );
    if progress.wire_millis > 0 {
        println!(
            "wire_rate_gb_s={:.3}",
            progress.wire_bytes as f64 / progress.wire_millis as f64 / 1e6
        );
    }
    if progress.elapsed_millis > 0 {
        println!(
            "end_to_end_gb_s={:.3}",
            progress.total_bytes as f64 / progress.elapsed_millis as f64 / 1e6
        );
    }
    let listed = goose_sidecar::hf::list_local_models(&models_dir).map_err(|e| e.to_string())?;
    match listed.iter().find(|m| m.id == model) {
        Some(row) => println!("listed: {row:?}"),
        None => println!("listed: <absent>"),
    }
    Ok(())
}
