//! The fleet an agent tick runs on, resolved the way a build run resolves it (the same probes,
//! the same servability rules, the same dispatcher door) minus the build-only levers (node caps,
//! planner-also-works, pre-warm). Lanes fan across the pool through a SLOT POOL: each device
//! serves `weight` lanes at once, a lane takes the freest device or waits for one — the queue
//! the operator sees in the desk.

use anyhow::{anyhow, Result};
use goose::agents::ExtensionConfig;
use goose_swarm::{DeviceCfg, EventSink};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

use super::super::fleet_order::aux_candidate_models;
use super::super::{
    build_worker_extension, load_config, repeat_break_enabled, stream_decode_retry_enabled,
    suppress_inherited_hints, swarm_min_p_resolved, swarm_repeat_penalty_resolved,
    swarm_temp_resolved, swarm_top_k_resolved, swarm_top_p_resolved, GooseAgentDispatcher,
    SamplingParams, SwarmDevice,
};
use super::store::DeviceSlot;
use crate::commands::swarm_engine::{
    all_resident_unservable_per_engine, device_engine_kind, drop_unservable_devices_per_engine,
    engines_for_run, exclude_unmountable_sidecar_devices, merge_sidecar_devices, planner_fallback,
    require_servable, served_by_engine, sidecar_exclusion_events, EngineKind,
};

pub struct Fleet {
    pub dispatcher: Arc<GooseAgentDispatcher>,
    pub planner_model: String,
    pub slots: Arc<SlotPool>,
    pub devices: Vec<DeviceSlot>,
    pub extensions: Vec<ExtensionConfig>,
}

/// Resolve the pool and build the dispatcher. Every named absence (an unservable device, a
/// planner fallback, an empty fleet falling to the configured devices) rides `sink`.
pub async fn resolve_fleet(
    agent_dir: &std::path::Path,
    sink: Arc<dyn EventSink>,
    extra_extensions: &[String],
) -> Result<Fleet> {
    let mut cfg = load_config();
    std::env::set_var("LMSTUDIO_HOST", &cfg.endpoint);
    suppress_inherited_hints();
    let engines = Arc::new(engines_for_run(&cfg.devices));
    let (mut fleet_pool, fleet_planner) =
        super::super::fleet_order::reconcile_pool_with_fleet(&cfg, &engines);
    for id in merge_sidecar_devices(&mut fleet_pool, &cfg.devices, &engines) {
        sink.write_value(serde_json::json!({
            "event": "sidecar-device-excluded", "id": id, "reason": "engine not registered",
        }));
    }
    let served = served_by_engine(&engines, &fleet_pool);
    for ev in engines.take_probe_absences() {
        sink.write_value(ev);
    }
    for ev in sidecar_exclusion_events(&exclude_unmountable_sidecar_devices(
        &mut fleet_pool,
        &served,
        cfg.allow_model_load,
    )) {
        sink.write_value(ev);
    }
    if require_servable() && all_resident_unservable_per_engine(&fleet_pool, &served) {
        let ids: Vec<&str> = fleet_pool.iter().map(|d| d.model_id.as_str()).collect();
        return Err(anyhow!(
            "the fleet at {} lists models but none of the loaded pool [{}] is servable",
            cfg.endpoint,
            ids.join(", ")
        ));
    }
    let (fleet_pool, unservable) = drop_unservable_devices_per_engine(fleet_pool, &served);
    for (id, why) in &unservable {
        sink.write_value(
            serde_json::json!({"event": "device_unservable", "id": id, "reason": why}),
        );
    }
    let enabled: Vec<SwarmDevice> = if !fleet_pool.is_empty() {
        if let Some(p) = fleet_planner {
            if !fleet_pool.iter().any(|d| d.model_id == cfg.planner_model) {
                cfg.planner_model = p;
            }
        }
        if let Some((host, alt)) =
            planner_fallback(&engines, &fleet_pool, &served, &cfg.planner_model)
        {
            sink.write_value(serde_json::json!({
                "event": "planner_fallback", "from": cfg.planner_model, "to": alt, "host": host,
            }));
            cfg.planner_model = alt;
        }
        fleet_pool
    } else {
        let configured: Vec<SwarmDevice> = cfg
            .devices
            .iter()
            .filter(|d| d.enabled)
            .cloned()
            .collect::<Vec<_>>();
        sink.write_value(serde_json::json!({
            "event": "fleet_empty_falls_to_config",
            "endpoint": cfg.endpoint,
            "configured_enabled": configured.len(),
        }));
        configured
    };
    if enabled.is_empty() {
        return Err(anyhow!(
            "no node can serve this desk: nothing is loaded at {} and no enabled device is configured (`goose swarm pool`)",
            cfg.endpoint
        ));
    }
    let devices: Vec<DeviceCfg> = enabled
        .iter()
        .map(|d| DeviceCfg {
            id: d.id.clone(),
            model_id: d.model_id.clone(),
            weight: d.weight.max(1),
            enabled: true,
            speed_weight: d.speed_weight.unwrap_or(1),
            supervision: d.supervision.unwrap_or(false),
            is_cloud: d.is_cloud(),
        })
        .collect();

    let mut ext_names = cfg.worker_extensions.clone();
    ext_names.extend(extra_extensions.iter().cloned());
    ext_names.sort();
    ext_names.dedup();
    let extensions: Vec<ExtensionConfig> = ext_names
        .iter()
        .filter_map(|n| build_worker_extension(n))
        .collect();

    let engine_models: std::collections::HashMap<String, EngineKind> = enabled
        .iter()
        .filter(|d| {
            let kind = device_engine_kind(d);
            kind != EngineKind::LmStudio && engines.for_kind(kind).is_some()
        })
        .map(|d| (d.model_id.clone(), device_engine_kind(d)))
        .collect();
    let dispatcher = Arc::new(
        GooseAgentDispatcher::new(
            agent_dir.to_path_buf(),
            sink.clone(),
            chrono::Utc::now().timestamp_millis(),
            cfg.worker_max_turns,
            extensions.clone(),
            enabled
                .iter()
                .filter(|d| d.is_cloud())
                .map(|d| (d.model_id.clone(), d.provider_name().to_string()))
                .collect(),
            cfg.planner_model.clone(),
            aux_candidate_models(&cfg.speed_weights, &enabled),
            cfg.allow_model_load,
            SamplingParams {
                temperature: swarm_temp_resolved(cfg.temperature),
                top_p: swarm_top_p_resolved(cfg.top_p),
                top_k: swarm_top_k_resolved(cfg.top_k),
                min_p: swarm_min_p_resolved(cfg.min_p),
                repeat_penalty: swarm_repeat_penalty_resolved(cfg.repeat_penalty),
            },
            stream_decode_retry_enabled(cfg.stream_decode_retry),
            repeat_break_enabled(cfg.repeat_break),
            engines.clone(),
            engine_models,
        )
        .await?,
    );
    dispatcher.set_fleet_nodes(&devices);

    let slots = Arc::new(SlotPool::new(&devices));
    let device_slots: Vec<DeviceSlot> = devices
        .iter()
        .map(|d| DeviceSlot {
            id: d.id.clone(),
            model_id: d.model_id.clone(),
            weight: d.weight,
            supervision: d.supervision,
        })
        .collect();
    sink.write_value(serde_json::json!({
        "event": "fleet_resolved",
        "planner_model": cfg.planner_model,
        "devices": device_slots,
        "lane_slots": slots.capacity(),
        "extensions": ext_names,
    }));
    Ok(Fleet {
        dispatcher,
        planner_model: cfg.planner_model,
        slots,
        devices: device_slots,
        extensions,
    })
}

struct Slot {
    model_id: String,
    weight: u32,
    inflight: AtomicU32,
}

/// Lane admission across the pool. Supervision devices are kept for the orchestrator unless
/// they are all the pool has.
pub struct SlotPool {
    slots: Vec<Slot>,
    freed: Notify,
}

pub struct SlotGuard {
    pool: Arc<SlotPool>,
    index: usize,
    pub model_id: String,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.pool.slots[self.index]
            .inflight
            .fetch_sub(1, Ordering::SeqCst);
        self.pool.freed.notify_waiters();
    }
}

impl SlotPool {
    pub fn new(devices: &[DeviceCfg]) -> Self {
        let lane_devices: Vec<&DeviceCfg> = {
            let workers: Vec<&DeviceCfg> = devices.iter().filter(|d| !d.supervision).collect();
            if workers.is_empty() {
                devices.iter().collect()
            } else {
                workers
            }
        };
        Self {
            slots: lane_devices
                .iter()
                .map(|d| Slot {
                    model_id: d.model_id.clone(),
                    weight: d.weight.max(1),
                    inflight: AtomicU32::new(0),
                })
                .collect(),
            freed: Notify::new(),
        }
    }

    pub fn capacity(&self) -> u32 {
        self.slots.iter().map(|s| s.weight).sum()
    }

    pub fn inflight(&self) -> u32 {
        self.slots
            .iter()
            .map(|s| s.inflight.load(Ordering::SeqCst))
            .sum()
    }

    fn try_take(self: &Arc<Self>) -> Option<SlotGuard> {
        let mut best: Option<(usize, u32)> = None;
        for (i, s) in self.slots.iter().enumerate() {
            let free = s.weight.saturating_sub(s.inflight.load(Ordering::SeqCst));
            if free > 0 && best.is_none_or(|(_, f)| free > f) {
                best = Some((i, free));
            }
        }
        let (i, _) = best?;
        self.slots[i].inflight.fetch_add(1, Ordering::SeqCst);
        Some(SlotGuard {
            pool: self.clone(),
            index: i,
            model_id: self.slots[i].model_id.clone(),
        })
    }

    /// Take the freest slot, or wait until one frees. Waiting is a QUEUE, never a bound.
    pub async fn acquire(self: &Arc<Self>) -> SlotGuard {
        loop {
            let notified = self.freed.notified();
            if let Some(g) = self.try_take() {
                return g;
            }
            notified.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, weight: u32, supervision: bool) -> DeviceCfg {
        DeviceCfg {
            id: id.into(),
            model_id: format!("{id}-model"),
            weight,
            enabled: true,
            speed_weight: 1,
            supervision,
            is_cloud: false,
        }
    }

    #[tokio::test]
    async fn slots_hand_out_the_freest_device_and_queue_when_full() {
        let pool = Arc::new(SlotPool::new(&[
            dev("a", 1, false),
            dev("b", 2, false),
            dev("s", 4, true),
        ]));
        assert_eq!(pool.capacity(), 3);
        let g1 = pool.acquire().await;
        assert_eq!(g1.model_id, "b-model");
        let g2 = pool.acquire().await;
        let g3 = pool.acquire().await;
        assert_eq!(pool.inflight(), 3);
        let waiting = {
            let p = pool.clone();
            tokio::spawn(async move { p.acquire().await.model_id.clone() })
        };
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        drop(g2);
        let got = waiting.await.unwrap();
        assert!(got == "a-model" || got == "b-model");
        drop(g1);
        drop(g3);
        // The spawned lane returned only the model id, so its guard dropped with the task.
        assert_eq!(pool.inflight(), 0);
    }

    #[test]
    fn an_all_supervision_pool_still_serves_lanes() {
        let pool = SlotPool::new(&[dev("s", 2, true)]);
        assert_eq!(pool.capacity(), 2);
    }
}
