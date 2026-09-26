//! THE reader of goose's measured runs (the measurement store, `store.rs`): which recorded runs speak
//! for a model on one way of running it, and the figure they make. Every consumer reads through
//! here — the planner's candidates (which the Run it cards and the Engine tile draw) and goosed's
//! `GET /mlx-engine/measured-runs` (the menu-bar tray and the chat's reading estimate) — so no two
//! surfaces can count runs differently again (Q-129: the planner kept only runs whose prompt fell
//! in the 2,048-token bucket — 1 of the Studio's 428 timed turns — while the store held them all).
//!
//! Comparable runs: the same model, the same way (the placement key) and the same serving stack.
//!
//! WRITING is every such run's decode rate, whatever its prompt size: a chat turn's prompt is the
//! whole conversation, so no fixed size is "the chat size" — the figure is this way's typical turn
//! over its real conversations (measured, the 27B on the Studio: medians of 26–30 tok/s from 128 to
//! 64k-token prompts, 22.9 at 128k). A run counts when it was timed over ENOUGH TOKENS: the recorder
//! times `tokens − 1` intervals between the first and the last streamed token, so one token is a
//! `1 / (tokens − 1)` step of the rate; a run counts when that step is no coarser than the spread
//! between this way's runs (the middle half's half-width over the median) — a rate over fewer
//! tokens cannot tell apart the runs it would join. Runs that all agree (one run, or the same rate
//! each time) have no spread to blur, and all count.
//!
//! READING is per prompt bucket: a turn's reading rate is its uncached tokens over its time to the
//! first token, and reading slows as the context it attends over grows (the Studio: ~131 tok/s over
//! 64k-token prompts), so only runs of the same bucket compare.

use super::planner::{backend_of, Figure};
use super::predict::Estimate;
use super::store::{PlacementKey, SpeedRecord};

/// This model's recorded runs on one way.
pub struct WayRuns<'a> {
    runs: Vec<&'a SpeedRecord>,
}

/// The writing figure and how it was counted.
#[derive(Debug, Clone, PartialEq)]
pub struct Writing {
    /// `None` when no timed run counts.
    pub figure: Option<Figure>,
    /// Runs on this way that carry a writing rate.
    pub timed: usize,
    /// Of those, the runs timed over too few tokens to count.
    pub too_short: usize,
    /// The spread a run's one-token step is held against: the middle half's half-width ÷ median.
    pub spread: f64,
}

impl Writing {
    /// What the figure rests on, in words; `None` when nothing on this way was timed.
    pub fn basis(&self) -> Option<String> {
        if self.timed == 0 {
            return None;
        }
        let spread = format!("±{:.0}%", self.spread * 100.0);
        Some(match &self.figure {
            Some(f) if self.too_short == 0 => {
                format!("writing: the median of this way's {} timed run(s)", f.runs)
            }
            Some(f) => format!(
                "writing: the median of {} of this way's {} timed runs — {} wrote too few tokens \
                 to time finer than the runs' own spread ({spread})",
                f.runs, self.timed, self.too_short
            ),
            None => format!(
                "writing: estimated — this way's {} timed run(s) each wrote too few tokens to time \
                 finer than their spread ({spread})",
                self.timed
            ),
        })
    }
}

impl<'a> WayRuns<'a> {
    pub fn of(records: &'a [SpeedRecord], model_id: &str, key: &PlacementKey) -> Self {
        let backend = backend_of(key.kind);
        Self {
            runs: records
                .iter()
                .filter(|r| r.model_id == model_id && &r.placement == key && r.backend == backend)
                .collect(),
        }
    }

    pub fn recorded(&self) -> usize {
        self.runs.len()
    }

    pub fn writing(&self) -> Writing {
        let timed: Vec<(&SpeedRecord, f64)> = self
            .runs
            .iter()
            .filter_map(|r| r.decode_tps.map(|d| (*r, d)))
            .filter(|(_, d)| d.is_finite())
            .collect();
        let rates: Vec<f64> = timed.iter().map(|(_, d)| *d).collect();
        let spread = Estimate::of_measurements(&rates)
            .filter(|e| e.value > 0.0)
            .map_or(0.0, |e| (e.high - e.low) / 2.0 / e.value);
        let counted: Vec<&SpeedRecord> = timed
            .iter()
            .filter(|(r, _)| spread == 0.0 || one_token_step(r) <= spread)
            .map(|(r, _)| *r)
            .collect();
        Writing {
            figure: figure_of(&counted, |r| r.decode_tps),
            timed: timed.len(),
            too_short: timed.len() - counted.len(),
            spread,
        }
    }

    /// Reading measured at prompts of `bucket` tokens.
    pub fn reading_at(&self, bucket: u64) -> Option<Figure> {
        let at: Vec<&SpeedRecord> = self
            .runs
            .iter()
            .copied()
            .filter(|r| r.context_bucket == bucket)
            .collect();
        figure_of(&at, |r| r.prefill_tps)
    }

    /// Reading per prompt bucket, smallest first — only buckets with a measured run.
    pub fn reading_by_bucket(&self) -> Vec<(u64, Figure)> {
        let mut buckets: Vec<u64> = self.runs.iter().map(|r| r.context_bucket).collect();
        buckets.sort_unstable();
        buckets.dedup();
        buckets
            .into_iter()
            .filter_map(|b| self.reading_at(b).map(|f| (b, f)))
            .collect()
    }
}

/// One token as a share of the rate: the recorder times `tokens − 1` intervals.
fn one_token_step(r: &SpeedRecord) -> f64 {
    1.0 / r.completion_tokens.saturating_sub(1).max(1) as f64
}

/// The median and middle half of `pick` over `runs`, as a measured figure.
fn figure_of(runs: &[&SpeedRecord], pick: impl Fn(&SpeedRecord) -> Option<f64>) -> Option<Figure> {
    let picked: Vec<&SpeedRecord> = runs.iter().copied().filter(|r| pick(r).is_some()).collect();
    let values: Vec<f64> = picked.iter().filter_map(|r| pick(r)).collect();
    Estimate::of_measurements(&values).map(|estimate| Figure {
        estimate,
        measured: true,
        runs: values.iter().filter(|v| v.is_finite()).count() as u32,
        last_measured_ms: picked.iter().map(|r| r.recorded_at_ms).max(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::store::{PlacementKind, SpeedStore};

    const MODEL: &str = "owner/Qwen3.8-27B-Q8-mlx";

    fn studio() -> PlacementKey {
        PlacementKey::single("link:studio-peer")
    }

    fn split() -> PlacementKey {
        PlacementKey {
            kind: PlacementKind::Tensor,
            nodes: vec!["local".into(), "link:studio-peer".into()],
            link: Some("jaccl".into()),
        }
    }

    /// Real rows of `~/.local/share/goose/mlx-speed-measurements.jsonl` (2026-09-26): 60 of the
    /// Studio's single-engine chat turns and 12 of the tensor split's, the peer's id and names
    /// anonymised. Prompts span 128 to 131,072 tokens; ONE Studio row sits in the 2,048 bucket.
    fn fixture() -> Vec<SpeedRecord> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/mlx-speed-measurements.sample.jsonl"
        );
        let read = SpeedStore::new(path).read().unwrap();
        assert!(read.unreadable.is_empty(), "{:?}", read.unreadable);
        read.records
    }

    #[test]
    fn writing_reads_every_bucket_not_only_the_benchmark_size() {
        let records = fixture();
        let runs = WayRuns::of(&records, MODEL, &studio());
        assert_eq!(runs.recorded(), 60);
        let at_2048 = records
            .iter()
            .filter(|r| r.placement == studio() && r.context_bucket == 2048)
            .count();
        assert_eq!(at_2048, 1, "the old reader's whole sample");
        let writing = runs.writing();
        let figure = writing.figure.clone().unwrap();
        assert_eq!(writing.timed, 55, "rows carrying a decode rate");
        assert!(
            figure.runs as usize > at_2048 * 10,
            "the real turns reach the figure: {writing:?}"
        );
        assert_eq!(figure.runs as usize + writing.too_short, writing.timed);
        assert!(figure.measured);
        assert!(figure.estimate.low <= figure.estimate.value);
        assert!(figure.estimate.value <= figure.estimate.high);
    }

    #[test]
    fn a_rate_over_too_few_tokens_does_not_count_and_the_basis_says_so() {
        let records = fixture();
        let writing = WayRuns::of(&records, MODEL, &studio()).writing();
        assert!(writing.too_short > 0, "{writing:?}");
        let counted_floor = 1.0 / writing.spread + 1.0;
        for r in records.iter().filter(|r| r.placement == studio()) {
            if r.decode_tps.is_some() && (r.completion_tokens as f64) < counted_floor.floor() {
                assert!(one_token_step(r) > writing.spread);
            }
        }
        let basis = writing.basis().unwrap();
        assert!(basis.contains("wrote too few tokens"), "{basis}");
        assert!(basis.contains(&format!("of this way's {} timed runs", writing.timed)));
    }

    #[test]
    fn the_way_is_the_whole_key_model_and_stack() {
        let records = fixture();
        assert_eq!(WayRuns::of(&records, MODEL, &split()).recorded(), 12);
        assert_eq!(
            WayRuns::of(&records, MODEL, &PlacementKey::single("local")).recorded(),
            0
        );
        assert_eq!(
            WayRuns::of(&records, "owner/other-model", &studio()).recorded(),
            0
        );
        let mut other_stack = records.clone();
        for r in &mut other_stack {
            r.backend = "mlx_lm".into();
        }
        assert_eq!(WayRuns::of(&other_stack, MODEL, &studio()).recorded(), 0);
    }

    #[test]
    fn runs_that_agree_all_count_and_one_run_is_one_run() {
        let mut records = fixture();
        records.retain(|r| r.placement == studio() && r.decode_tps.is_some());
        records.truncate(1);
        let writing = WayRuns::of(&records, MODEL, &studio()).writing();
        assert_eq!(writing.spread, 0.0);
        let figure = writing.figure.unwrap();
        assert_eq!(figure.runs, 1);
        assert_eq!(figure.estimate.low, figure.estimate.high);
    }

    #[test]
    fn reading_compares_only_the_same_prompt_bucket() {
        let records = fixture();
        let runs = WayRuns::of(&records, MODEL, &studio());
        let by_bucket = runs.reading_by_bucket();
        assert!(!by_bucket.is_empty());
        for (bucket, figure) in &by_bucket {
            let expected = records
                .iter()
                .filter(|r| {
                    r.placement == studio()
                        && r.context_bucket == *bucket
                        && r.prefill_tps.is_some()
                })
                .count();
            assert_eq!(figure.runs as usize, expected, "bucket {bucket}");
        }
        assert!(
            runs.reading_at(2048).is_none(),
            "the 2k row carries no reading rate"
        );
    }
}
