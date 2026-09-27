//! The load store: how long each model took to load, per way, as one JSON line per load in a
//! goose-owned file beside the speed store (design: DESIGN-NODES-AND-STRATEGIES.md §6.4 step 10).
//! A load time is DISPLAYED ("about 48 s · median of 3 loads"), never used to decide which node
//! loads (gate 5). A missing file means no load was ever measured, which is the honest answer
//! until the ready paths write rows; it is never replaced by an estimate (gate 1).

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::store::PlacementKey;

/// The file under goose's data dir.
pub const LOADS_FILE: &str = "mlx-load-measurements.jsonl";

/// Milliseconds spent in each load phase the engine reports. A phase the load never showed is
/// absent, never 0.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadPhasesMs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starting: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loading: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warming: Option<u64>,
}

/// How the load ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LoadOutcome {
    /// The way answered `/v1/models`.
    Ready,
    /// The load failed; `words` are the engine's own.
    Failed { words: String },
    /// The demanding turn was cancelled after the stops began; the swap ran to its end.
    CancelledAfterStop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadRecord {
    /// The model as the models folder names it (the HF directory id).
    pub model: String,
    pub placement: PlacementKey,
    /// Display names of the Macs, in the placement's order.
    pub macs: Vec<String>,
    pub weights_bytes: u64,
    pub phases_ms: LoadPhasesMs,
    pub total_ms: u64,
    /// Whether the weights were already in the file cache when the load began.
    pub file_cache_warm: bool,
    pub outcome: LoadOutcome,
    pub recorded_at_ms: u64,
}

#[derive(Debug, Default)]
pub struct LoadsRead {
    pub records: Vec<LoadRecord>,
    /// `line N: <why>` for every line that did not parse.
    pub unreadable: Vec<String>,
}

/// The median of the measured loads of one model on one way, with how many it was taken over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadMedian {
    pub total_ms: u64,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct LoadStore {
    path: PathBuf,
}

impl LoadStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one record as one line (one `write` of the whole line, so concurrent writers never
    /// interleave inside a record).
    pub fn append(&self, record: &LoadRecord) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
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
    /// file is an error; an unparseable line is listed in `unreadable`, never skipped in silence.
    pub fn read(&self) -> Result<LoadsRead> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(LoadsRead::default()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", self.path.display())),
        };
        let mut read = LoadsRead::default();
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<LoadRecord>(line) {
                Ok(record) => read.records.push(record),
                Err(e) => read.unreadable.push(format!("line {}: {e}", index + 1)),
            }
        }
        Ok(read)
    }
}

/// The median total of the loads of `model` on `placement` that reached Ready. A failed or
/// cancelled load says nothing about how long a load takes, so it is not counted. `None` = no
/// such load was measured.
pub fn median_for(
    records: &[LoadRecord],
    model: &str,
    placement: &PlacementKey,
) -> Option<LoadMedian> {
    let mut totals: Vec<u64> = records
        .iter()
        .filter(|r| {
            r.model == model && &r.placement == placement && r.outcome == LoadOutcome::Ready
        })
        .map(|r| r.total_ms)
        .collect();
    if totals.is_empty() {
        return None;
    }
    totals.sort_unstable();
    let mid = totals.len() / 2;
    let total_ms = if totals.len() % 2 == 1 {
        totals[mid]
    } else {
        (totals[mid - 1] + totals[mid]) / 2
    };
    Some(LoadMedian {
        total_ms,
        count: totals.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::store::PlacementKind;

    fn split() -> PlacementKey {
        PlacementKey {
            kind: PlacementKind::Pipeline,
            nodes: vec!["local".into(), "link:studio".into()],
            link: Some("jaccl".into()),
        }
    }

    fn row(
        model: &str,
        placement: PlacementKey,
        total_ms: u64,
        outcome: LoadOutcome,
    ) -> LoadRecord {
        LoadRecord {
            model: model.to_string(),
            placement,
            macs: vec!["Mihai Macbook".into()],
            weights_bytes: 31_000_000_000,
            phases_ms: LoadPhasesMs {
                starting: Some(900),
                loading: Some(total_ms.saturating_sub(2_000)),
                warming: Some(1_100),
            },
            total_ms,
            file_cache_warm: false,
            outcome,
            recorded_at_ms: 1,
        }
    }

    #[test]
    fn an_absent_store_has_no_median_and_reads_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = LoadStore::new(dir.path().join(LOADS_FILE));
        let read = store.read().unwrap();
        assert!(read.records.is_empty() && read.unreadable.is_empty());
        assert_eq!(
            median_for(&read.records, "m", &PlacementKey::single("local")),
            None
        );
    }

    #[test]
    fn appended_rows_round_trip_and_a_broken_line_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let store = LoadStore::new(dir.path().join("nested").join(LOADS_FILE));
        let a = row("m", split(), 48_000, LoadOutcome::Ready);
        store.append(&a).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(store.path())
            .unwrap()
            .write_all(b"{not json\n")
            .unwrap();
        let failed = row(
            "m",
            split(),
            3_000,
            LoadOutcome::Failed {
                words: "the peer left".into(),
            },
        );
        store.append(&failed).unwrap();
        let read = store.read().unwrap();
        assert_eq!(read.records, vec![a, failed]);
        assert_eq!(read.unreadable.len(), 1);
        assert!(
            read.unreadable[0].starts_with("line 2:"),
            "{:?}",
            read.unreadable
        );
        let json = serde_json::to_value(&read.records[1]).unwrap();
        assert_eq!(json["outcome"]["kind"], "failed");
        assert_eq!(json["phasesMs"]["starting"], 900);
    }

    #[test]
    fn the_median_counts_only_ready_loads_of_the_same_model_and_way() {
        let local = PlacementKey::single("local");
        let rows = vec![
            row("m", split(), 50_000, LoadOutcome::Ready),
            row("m", split(), 46_000, LoadOutcome::Ready),
            row("m", split(), 48_000, LoadOutcome::Ready),
            row(
                "m",
                split(),
                1_000,
                LoadOutcome::Failed { words: "x".into() },
            ),
            row("m", split(), 2_000, LoadOutcome::CancelledAfterStop),
            row("m", local.clone(), 9_000, LoadOutcome::Ready),
            row("other", split(), 99_000, LoadOutcome::Ready),
        ];
        assert_eq!(
            median_for(&rows, "m", &split()),
            Some(LoadMedian {
                total_ms: 48_000,
                count: 3
            })
        );
        assert_eq!(
            median_for(&rows, "m", &local),
            Some(LoadMedian {
                total_ms: 9_000,
                count: 1
            })
        );
        let even = vec![
            row("m", local.clone(), 10_000, LoadOutcome::Ready),
            row("m", local.clone(), 20_000, LoadOutcome::Ready),
        ];
        assert_eq!(median_for(&even, "m", &local).unwrap().total_ms, 15_000);
        let only_failed = vec![row(
            "m",
            local.clone(),
            1,
            LoadOutcome::Failed { words: "x".into() },
        )];
        assert_eq!(median_for(&only_failed, "m", &local), None);
    }
}
