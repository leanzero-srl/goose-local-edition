//! Every real chat turn an MLX engine on THIS Mac answers feeds the placement planner's
//! measurement store (`goose_sidecar::placement::store`): the turn's time to first token, its
//! decode rate between the first and the last streamed message, and its prompt size. Prefill is
//! recorded only when the provider reported how much of the prompt the cache served — a turn's
//! prompt is mostly cache hits, so a rate over the whole prompt would be a fabricated reading speed.
//!
//! Which engine served the turn is decided when it ends, from the same facts the router probed:
//! the peer's single engine while a remote-single route is live (its chip unrecorded: the chat
//! stream does not carry it), else this goosed's distributed engine when it owns the Mac, else the
//! single engine. Another window's distributed engine is not recorded here: this process cannot
//! name its placement's nodes.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;

use super::base::MessageStream;
use futures::Stream;
use goose_providers::conversation::token_usage::ProviderUsage;

use crate::config::paths::Paths;

#[cfg(unix)]
pub(crate) fn speed_store() -> goose_sidecar::placement::store::SpeedStore {
    goose_sidecar::placement::store::SpeedStore::new(Paths::in_data_dir(
        goose_sidecar::placement::store::STORE_FILE,
    ))
}

struct Turn {
    started: Instant,
    first: Option<Instant>,
    last: Option<Instant>,
    usage: Option<ProviderUsage>,
    failed: bool,
}

struct Observed {
    inner: MessageStream,
    turn: Option<Turn>,
}

impl Stream for Observed {
    type Item = <MessageStream as Stream>::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let next = this.inner.as_mut().poll_next(cx);
        if let Poll::Ready(item) = &next {
            match item {
                Some(Ok((message, usage))) => {
                    if let Some(turn) = this.turn.as_mut() {
                        if message.is_some() {
                            let now = Instant::now();
                            turn.first.get_or_insert(now);
                            turn.last = Some(now);
                        }
                        if usage.is_some() {
                            turn.usage = usage.clone();
                        }
                    }
                }
                Some(Err(_)) => {
                    if let Some(turn) = this.turn.as_mut() {
                        turn.failed = true;
                    }
                }
                None => {
                    if let Some(turn) = this.turn.take() {
                        if !turn.failed {
                            tokio::spawn(record(turn));
                        }
                    }
                }
            }
        }
        next
    }
}

/// Wrap a stream an MLX engine on this Mac is answering.
pub(crate) fn observe(inner: MessageStream) -> MessageStream {
    Box::pin(Observed {
        inner,
        turn: Some(Turn {
            started: Instant::now(),
            first: None,
            last: None,
            usage: None,
            failed: false,
        }),
    })
}

#[cfg(unix)]
async fn record(turn: Turn) {
    use goose_sidecar::placement::store::{
        context_bucket, PlacementKey, PlacementKind, RecordSource, SpeedRecord,
    };
    use goose_sidecar::placement::{chip, planner};

    let (Some(usage), Some(first), Some(last)) = (turn.usage, turn.first, turn.last) else {
        return;
    };
    let (Some(input), Some(output)) = (usage.usage.input_tokens, usage.usage.output_tokens) else {
        return;
    };
    let (input, output) = (input.max(0) as u64, output.max(0) as u64);
    let dist = goose_sidecar::distributed::global_manager().status();
    let (key, model_id, node_names, peers) = if let Some(route) = super::mlx_remote::read().live() {
        let key = PlacementKey::single(&format!("link:{}", route.peer));
        (key, route.model_id, vec![route.peer_hostname], 0)
    } else if dist.state.owns_the_mac() {
        let (Some(config), Some(model), Some(runner)) =
            (dist.config.clone(), dist.model_id.clone(), dist.runner)
        else {
            return;
        };
        let kind = match runner {
            goose_sidecar::distributed::Runner::MlxLmTensor => PlacementKind::Tensor,
            goose_sidecar::distributed::Runner::PipelineQwen4 => PlacementKind::Pipeline,
        };
        let key = PlacementKey {
            kind,
            nodes: config
                .nodes
                .iter()
                .map(|n| n.ssh.clone().unwrap_or_else(|| "local".to_string()))
                .collect(),
            link: Some(
                match config.backend {
                    goose_sidecar::distributed::Backend::Jaccl => "jaccl",
                    goose_sidecar::distributed::Backend::Ring => "ring",
                }
                .to_string(),
            ),
        };
        let names = config.nodes.iter().map(|n| n.name.clone()).collect();
        (key, model, names, config.nodes.len() - 1)
    } else {
        let single = goose_sidecar::engine::global_manager().status().await;
        let (true, Some(model)) = (single.state == "running", single.model_id) else {
            return;
        };
        (
            PlacementKey::single("local"),
            model,
            vec![local_name().await],
            0,
        )
    };
    let remote =
        key.nodes.first().is_some_and(|n| n != "local") && key.kind == PlacementKind::Single;
    let kv_cache = if key.kind == PlacementKind::Single && !remote {
        match crate::config::Config::global()
            .get_param::<goose_sidecar::engine::EngineSettings>("mlx_engine")
        {
            Ok(settings) => settings
                .model_profiles
                .get(&model_id)
                .and_then(|p| p.kv_cache)
                .map(|m| m.engine_dtype().to_string()),
            Err(crate::config::ConfigError::NotFound(_)) => None,
            Err(error) => {
                tracing::warn!(%error, "mlx speed record: mlx_engine settings unreadable; the turn is not recorded (its KV cache setting is unknown)");
                return;
            }
        }
    } else {
        None
    };
    let local_chip = match chip::local_chip().await {
        Ok(c) => Some(c),
        Err(error) => {
            tracing::warn!(%error, "mlx speed record: this Mac's chip is unreadable; recorded without it");
            None
        }
    };
    let decode_s = last.duration_since(first).as_secs_f64();
    let ttft_s = first.duration_since(turn.started).as_secs_f64();
    let uncached = usage
        .usage
        .cache_read_input_tokens
        .map(|cached| input.saturating_sub(cached.max(0) as u64));
    let record = SpeedRecord {
        model_id,
        backend: planner::backend_of(key.kind).to_string(),
        placement: key,
        node_names,
        chips: if remote {
            vec![None]
        } else {
            std::iter::once(local_chip)
                .chain(std::iter::repeat_n(None, peers))
                .collect()
        },
        context_bucket: context_bucket(input),
        prompt_tokens: input,
        completion_tokens: output,
        prefill_tps: uncached.filter(|_| ttft_s > 0.0).map(|u| u as f64 / ttft_s),
        decode_tps: (decode_s > 0.0 && output > 1).then(|| (output - 1) as f64 / decode_s),
        ttft_ms: Some(ttft_s * 1000.0),
        recorded_at_ms: goose_sidecar::distributed::preflight::now_ms(),
        source: RecordSource::Chat,
        workload: None,
        kv_cache,
    };
    if let Err(error) = speed_store().append(&record) {
        tracing::warn!(error = %format!("{error:#}"), "mlx speed record: the measurement store refused the turn");
    }
}

/// This Mac's display name for the record (`scutil`); the placement id stays `local` either way.
#[cfg(unix)]
async fn local_name() -> String {
    match tokio::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "ComputerName"])
        .output()
        .await
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => "local".to_string(),
    }
}

#[cfg(not(unix))]
async fn record(_turn: Turn) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::message::Message;
    use futures::StreamExt;
    use goose_providers::base::stream_from_single_message;
    use goose_providers::conversation::token_usage::Usage;

    #[tokio::test]
    async fn an_observed_stream_passes_every_item_through_unchanged() {
        let usage = ProviderUsage::new("m".to_string(), Usage::default());
        let inner = stream_from_single_message(Message::assistant().with_text("hi"), usage.clone());
        let items: Vec<_> = observe(inner).collect().await;
        assert_eq!(items.len(), 1);
        let (message, seen) = items[0].as_ref().unwrap();
        assert_eq!(message.as_ref().unwrap().as_concat_text(), "hi");
        assert_eq!(seen.as_ref().unwrap().model, "m");
    }
}
