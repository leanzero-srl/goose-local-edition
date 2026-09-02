//! Median decode rate per node from the run's OWN telemetry file (`GOOSE_SWARM_TELEMETRY_FILE`,
//! one JSON line per completion call). Consumed by the REPAIR fix-target ranking
//! (`fleet_order::rank_fix_target`) through swarm.rs. Moved out of swarm.rs under the
//! incremental-split law (VA-107) — mechanical move, tests with it.

use std::collections::HashMap;
use std::path::Path;

/// Median decode rate (tokens/sec) per node, from the run's OWN telemetry file. A record
/// contributes only when it carries real backend usage and a positive decode window —
/// approximations and failed calls never rank a node. Pure/testable.
pub(super) fn telemetry_node_rates(path: &Path) -> HashMap<String, f64> {
    let mut samples: HashMap<String, Vec<f64>> = Default::default();
    let Ok(text) = std::fs::read_to_string(path) else {
        return Default::default();
    };
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("usage").and_then(|x| x.as_bool()) != Some(true) {
            continue;
        }
        let (Some(node), Some(ct), Some(ttft), Some(total)) = (
            v.get("node").and_then(|x| x.as_str()),
            v.get("completion_tokens").and_then(|x| x.as_f64()),
            v.get("ttft_ms").and_then(|x| x.as_f64()),
            v.get("total_ms").and_then(|x| x.as_f64()),
        ) else {
            continue;
        };
        let decode_s = (total - ttft) / 1000.0;
        if ct > 0.0 && decode_s > 0.0 {
            samples
                .entry(node.to_string())
                .or_default()
                .push(ct / decode_s);
        }
    }
    samples
        .into_iter()
        .map(|(k, mut v)| {
            v.sort_by(|a, b| a.total_cmp(b));
            let m = v[v.len() / 2];
            (k, m)
        })
        .collect()
}

#[cfg(test)]
mod telemetry_rank_tests {
    use super::*;

    #[test]
    fn rates_use_median_and_skip_useless_records() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("t.jsonl");
        std::fs::write(
            &p,
            [
                // three good gabee samples: 10, 20, 30 tok/s -> median 20
                r#"{"node":"gabee","usage":true,"completion_tokens":100,"ttft_ms":0,"total_ms":10000}"#,
                r#"{"node":"gabee","usage":true,"completion_tokens":200,"ttft_ms":0,"total_ms":10000}"#,
                r#"{"node":"gabee","usage":true,"completion_tokens":300,"ttft_ms":0,"total_ms":10000}"#,
                // no real usage -> never ranks a node
                r#"{"node":"mihai","usage":false,"completion_tokens":900,"ttft_ms":0,"total_ms":1000}"#,
                // zero decode window -> skipped
                r#"{"node":"mihai","usage":true,"completion_tokens":50,"ttft_ms":1000,"total_ms":1000}"#,
                "not json at all",
            ]
            .join("\n"),
        )
        .unwrap();
        let rates = telemetry_node_rates(&p);
        assert_eq!(
            rates.len(),
            1,
            "only nodes with usable samples rank: {rates:?}"
        );
        assert!(
            (rates["gabee"] - 20.0).abs() < 1e-9,
            "median, not mean: {rates:?}"
        );
        assert!(telemetry_node_rates(Path::new("/definitely/missing")).is_empty());
    }
}
