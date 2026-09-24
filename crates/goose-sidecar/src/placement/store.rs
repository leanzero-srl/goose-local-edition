//! The measurement store: every real speed goose measured — a "Measure speed" benchmark or a
//! finished chat turn — as one JSON line in a goose-owned file. The planner prefers these over any
//! formula, and recalibrates its per-chip factors from them. A line that no longer parses is
//! reported by line number, never skipped in silence.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The file under goose's data dir.
pub const STORE_FILE: &str = "mlx-speed-measurements.jsonl";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlacementKind {
    /// One Mac runs the whole model (the Rapid-MLX single engine).
    Single,
    /// Every Mac holds a shard of every layer (mlx_lm tensor parallel).
    Tensor,
    /// Each Mac holds a contiguous range of layers (the fork's pipeline).
    Pipeline,
}

impl PlacementKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PlacementKind::Single => "single",
            PlacementKind::Tensor => "tensor",
            PlacementKind::Pipeline => "pipeline",
        }
    }
}

/// Which placement ran: its kind, its nodes in rank order (`local` = the Mac that recorded it, a
/// peer by its configured host — an ssh alias or `link:<id>`), and the link backend of a split.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementKey {
    pub kind: PlacementKind,
    pub nodes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

impl PlacementKey {
    pub fn single(node: &str) -> Self {
        Self {
            kind: PlacementKind::Single,
            nodes: vec![node.to_string()],
            link: None,
        }
    }

    /// `single:local`, `tensor:jaccl:local+link:abc`.
    pub fn id(&self) -> String {
        let mut id = self.kind.as_str().to_string();
        if let Some(link) = &self.link {
            id.push(':');
            id.push_str(link);
        }
        id.push(':');
        id.push_str(&self.nodes.join("+"));
        id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordSource {
    Benchmark,
    Chat,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedRecord {
    pub model_id: String,
    pub placement: PlacementKey,
    /// Display names of the nodes, in the placement's order.
    pub node_names: Vec<String>,
    /// Each node's chip label ("Apple M3 Ultra · 60-core GPU"), in the placement's order.
    pub chips: Vec<String>,
    /// The serving stack ("rapid-mlx", "mlx_lm", "pipeline_qwen4") — a measurement answers only
    /// for the stack that produced it.
    pub backend: String,
    /// The power of two at or above the prompt's token count.
    pub context_bucket: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    /// Prompt tokens read per second (uncached tokens ÷ time to first token). `None` for a chat
    /// turn: its stream does not say how much of the prompt the prefix cache served.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill_tps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_tps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<f64>,
    pub recorded_at_ms: u64,
    pub source: RecordSource,
    /// The benchmark workload's name; `None` for a chat turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<String>,
    /// The single engine's compressed KV cache when one was on ("int8" / "int4").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kv_cache: Option<String>,
}

/// The bucket a prompt of `tokens` falls in: the power of two at or above it.
pub fn context_bucket(tokens: u64) -> u64 {
    tokens.max(1).next_power_of_two()
}

#[derive(Debug, Default)]
pub struct StoreRead {
    pub records: Vec<SpeedRecord>,
    /// `line N: <why>` for every line that did not parse.
    pub unreadable: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SpeedStore {
    path: PathBuf,
}

impl SpeedStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one record as one line (one `write` of the whole line, so concurrent writers never
    /// interleave inside a record).
    pub fn append(&self, record: &SpeedRecord) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating {}", dir.display()))?;
        }
        let mut line = serde_json::to_string(record)?;
        line.push('\n');
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| f.write_all(line.as_bytes()))
            .with_context(|| format!("appending to {}", self.path.display()))
    }

    /// Every record. A missing file is an empty store (nothing was ever measured); an unreadable
    /// file is an error; an unparseable line is listed in `unreadable`.
    pub fn read(&self) -> Result<StoreRead> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(StoreRead::default()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", self.path.display())),
        };
        let mut read = StoreRead::default();
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<SpeedRecord>(line) {
                Ok(record) => read.records.push(record),
                Err(e) => read.unreadable.push(format!("line {}: {e}", index + 1)),
            }
        }
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn record(model: &str, key: PlacementKey, decode: f64) -> SpeedRecord {
        SpeedRecord {
            model_id: model.to_string(),
            placement: key,
            node_names: vec!["Mihai Macbook".to_string()],
            chips: vec!["Apple M4 Max · 40-core GPU".to_string()],
            backend: "rapid-mlx".to_string(),
            context_bucket: 2048,
            prompt_tokens: 1900,
            completion_tokens: 256,
            prefill_tps: Some(230.0),
            decode_tps: Some(decode),
            ttft_ms: Some(8260.0),
            recorded_at_ms: 1,
            source: RecordSource::Benchmark,
            workload: Some("chat".to_string()),
            kv_cache: None,
        }
    }

    #[test]
    fn records_round_trip_and_a_broken_line_is_named_not_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let store = SpeedStore::new(dir.path().join("nested").join(STORE_FILE));
        assert!(store.read().unwrap().records.is_empty(), "absent = empty");
        let a = record("m", PlacementKey::single("local"), 21.5);
        store.append(&a).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(store.path())
            .unwrap()
            .write_all(b"{not json\n")
            .unwrap();
        store.append(&a).unwrap();
        let read = store.read().unwrap();
        assert_eq!(read.records, vec![a.clone(), a]);
        assert_eq!(read.unreadable.len(), 1);
        assert!(read.unreadable[0].starts_with("line 2:"), "{:?}", read.unreadable);
    }

    #[test]
    fn keys_and_buckets_are_stable() {
        let key = PlacementKey {
            kind: PlacementKind::Tensor,
            nodes: vec!["local".into(), "link:abc".into()],
            link: Some("jaccl".into()),
        };
        assert_eq!(key.id(), "tensor:jaccl:local+link:abc");
        assert_eq!(PlacementKey::single("local").id(), "single:local");
        assert_eq!(context_bucket(1900), 2048);
        assert_eq!(context_bucket(2048), 2048);
        assert_eq!(context_bucket(2049), 4096);
        let json = serde_json::to_value(record("m", key, 1.0)).unwrap();
        assert_eq!(json["placement"]["kind"], "tensor");
        assert_eq!(json["contextBucket"], 2048);
    }
}
