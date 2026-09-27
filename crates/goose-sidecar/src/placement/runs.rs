//! THE reader of goose's measured runs (the measurement store, `store.rs`): which recorded runs speak
//! for a model on one way of running it, and the figure they make. Every consumer reads through
//! here — the planner's candidates (which the Run it cards and the Engine tile draw) and goosed's
//! `GET /mlx-engine/measured-runs` (the menu-bar tray and the chat's reading estimate) — so no two
//! surfaces can count runs differently again (Q-129: the planner kept only runs whose prompt fell
//! in the 2,048-token bucket — 1 of the Studio's 428 timed turns — while the store held them all).
//!
//! Comparable runs: the same model, the same way (the placement key) and the same serving stack.
//!
//! WRITING is the decode rate of this way's runs at THIS APP'S chat size (`ChatShape`: the prompt
//! bucket the conversations' reading is spent in — no fixed size is "the chat size", the recorded
//! chats say what it is). Every prompt size once counted (the 27B on the Studio: medians of 26–30
//! tok/s from 128 to 64k-token prompts), until the tensor split measured otherwise: goose's own
//! side calls — ~100–200-token prompts, ~6-token answers, 788 of its 867 timed runs — wrote at 4.7
//! tok/s, which is start-up, not writing, while its 64k-token turns wrote 10.6 and the live turn
//! 10.9 (Q-168). A run counts when it was timed over ENOUGH TOKENS: the recorder
//! times `tokens − 1` intervals between the first and the last streamed token, so one token is a
//! `1 / (tokens − 1)` step of the rate; a run counts when that step is no coarser than the spread
//! between this way's runs (the middle half's half-width over the median) — a rate over fewer
//! tokens cannot tell apart the runs it would join. Runs that all agree (one run, or the same rate
//! each time) have no spread to blur, and all count.
//!
//! READING is per prompt bucket: a turn's reading rate is its uncached tokens over its time to the
//! first token, and reading slows as the context it attends over grows (the Studio: ~131 tok/s over
//! 64k-token prompts), so only runs of the same bucket compare. The headline reading is at the same
//! chat bucket as writing (Q-168: "18.0 tok/s reading, median of 2 prompts" came from the
//! benchmark's 2,048 bucket while 49 turns at 64k read ~250 tok/s), and carries that bucket.

use super::bench::Workload;
use super::planner::{backend_of, Figure};
use super::predict::Estimate;
use super::store::{context_bucket, PlacementKey, RecordSource, SpeedRecord};

/// The chat turn THIS app's conversations make, from every chat turn goose recorded (any model,
/// any way — the conversations are the user's, not the engine's): the prompt the typical token of
/// reading belongs to, and the answer turns of that size write. What "Chat" is sized by in the
/// planner — the context a way must hold to be Best, the prompt size its rates are read at.
///
/// The prompt is the TOKEN-weighted median, not the median turn: goose's own side calls (a title,
/// a check — measured 2026-09-27: 1,516 chat rows, median prompt 164 tokens) outnumber the
/// conversation turns, while the conversation turns carry the reading (weighted median 52,976
/// tokens — the 41k–53k the chat header shows). A prompt counts by the tokens it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatShape {
    pub prompt_tokens: u64,
    pub answer_tokens: u64,
    /// Chat turns in the prompt's bucket the answer is the median of; 0 = goose has recorded no
    /// chat turn, and the shape is the "Measure speed" chat workload's own.
    pub turns: usize,
}

impl ChatShape {
    pub fn of(records: &[SpeedRecord]) -> Self {
        let mut prompts: Vec<u64> = records
            .iter()
            .filter(|r| r.source == RecordSource::Chat && r.prompt_tokens > 0)
            .map(|r| r.prompt_tokens)
            .collect();
        prompts.sort_unstable();
        let total: u64 = prompts.iter().sum();
        let mut read = 0u64;
        let Some(prompt_tokens) = prompts.into_iter().find(|p| {
            read += p;
            read * 2 >= total
        }) else {
            return Self {
                prompt_tokens: Workload::Chat.prompt_tokens(),
                answer_tokens: Workload::Chat.answer_tokens(),
                turns: 0,
            };
        };
        let bucket = context_bucket(prompt_tokens);
        let answers: Vec<f64> = records
            .iter()
            .filter(|r| r.source == RecordSource::Chat && r.context_bucket == bucket)
            .map(|r| r.completion_tokens as f64)
            .collect();
        Self {
            prompt_tokens,
            answer_tokens: Estimate::of_measurements(&answers).map_or(0, |e| e.value as u64),
            turns: answers.len(),
        }
    }

    /// The bucket its rates are read at.
    pub fn bucket(&self) -> u64 {
        context_bucket(self.prompt_tokens)
    }

    /// The context a turn of this shape needs: its prompt and its answer.
    pub fn context_needed(&self) -> u64 {
        self.prompt_tokens + self.answer_tokens
    }
}

/// This model's recorded runs on one way.
pub struct WayRuns<'a> {
    runs: Vec<&'a SpeedRecord>,
}

/// The writing figure and how it was counted.
#[derive(Debug, Clone, PartialEq)]
pub struct Writing {
    /// `None` when no timed run counts.
    pub figure: Option<Figure>,
    /// The prompt bucket the figure is read at (this app's chat prompt, `ChatShape::bucket`).
    pub bucket: u64,
    /// Runs on this way at that bucket that carry a writing rate.
    pub timed: usize,
    /// Of those, the runs timed over too few tokens to count.
    pub too_short: usize,
    /// Runs on this way timed at OTHER prompt sizes, left out of the figure.
    pub elsewhere: usize,
    /// The spread a run's one-token step is held against: the middle half's half-width ÷ median.
    pub spread: f64,
}

impl Writing {
    /// What the figure rests on, in words; `None` when nothing on this way was timed at all.
    pub fn basis(&self) -> Option<String> {
        if self.timed == 0 && self.elsewhere == 0 {
            return None;
        }
        let at = format!(
            "prompts up to {} tokens",
            super::planner::tokens_words(self.bucket)
        );
        let spread = format!("±{:.0}%", self.spread * 100.0);
        let counted = match &self.figure {
            Some(f) if self.too_short == 0 => {
                format!(
                    "writing: the median of this way's {} timed run(s) at {at}",
                    f.runs
                )
            }
            Some(f) => format!(
                "writing: the median of {} of this way's {} timed runs at {at} — {} wrote too few \
                 tokens to time finer than the runs' own spread ({spread})",
                f.runs, self.timed, self.too_short
            ),
            None if self.timed == 0 => format!(
                "writing: estimated — none of this way's {} timed run(s) read {at}, the size of \
                 this app's chats",
                self.elsewhere
            ),
            None => format!(
                "writing: estimated — this way's {} timed run(s) at {at} each wrote too few tokens \
                 to time finer than their spread ({spread})",
                self.timed
            ),
        };
        Some(if self.elsewhere > 0 && self.figure.is_some() {
            format!(
                "{counted}; {} timed at other prompt sizes are left out (a short side call's rate \
                 is mostly its start-up)",
                self.elsewhere
            )
        } else {
            counted
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

    /// Writing at prompts of `bucket` tokens — this app's chat size (`ChatShape::bucket`). Runs at
    /// other sizes are left out: goose's own side calls (a title, a check) read ~100–200 tokens and
    /// write ~6, so their rate is mostly the request's start-up (Q-168: the tensor split's 788
    /// such runs made "4.7 tok/s writing" while its 64k-token turns wrote 10.6 and the live turn
    /// 10.9).
    pub fn writing(&self, bucket: u64) -> Writing {
        let all_timed: Vec<(&SpeedRecord, f64)> = self
            .runs
            .iter()
            .filter_map(|r| r.decode_tps.map(|d| (*r, d)))
            .filter(|(_, d)| d.is_finite())
            .collect();
        let timed: Vec<(&SpeedRecord, f64)> = all_timed
            .iter()
            .copied()
            .filter(|(r, _)| r.context_bucket == bucket)
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
            bucket,
            timed: timed.len(),
            too_short: timed.len() - counted.len(),
            elsewhere: all_timed.len() - timed.len(),
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
    fn writing_reads_this_apps_chat_size_not_the_benchmarks() {
        let records = fixture();
        let runs = WayRuns::of(&records, MODEL, &studio());
        assert_eq!(runs.recorded(), 60);
        let chat = ChatShape::of(&records);
        assert_eq!(chat.bucket(), 65_536, "{chat:?}");
        let at_2048 = records
            .iter()
            .filter(|r| r.placement == studio() && r.context_bucket == 2048)
            .count();
        assert_eq!(at_2048, 1, "the old benchmark-size reader's whole sample");
        let writing = runs.writing(chat.bucket());
        let figure = writing.figure.clone().unwrap();
        assert_eq!(writing.timed, 12, "rows at 64k carrying a decode rate");
        assert_eq!(
            writing.timed + writing.elsewhere,
            55,
            "every timed row is accounted"
        );
        assert_eq!(figure.runs as usize + writing.too_short, writing.timed);
        assert!(figure.measured);
        assert!(figure.estimate.low <= figure.estimate.value);
        assert!(figure.estimate.value <= figure.estimate.high);
        let basis = writing.basis().unwrap();
        assert!(
            basis.contains("at prompts up to 66k tokens")
                && basis.contains("43 timed at other prompt sizes are left out"),
            "{basis}"
        );
    }

    /// Q-168, the tensor split as the live store held it on 2026-09-27: 788 of its 867 timed runs
    /// were goose's own side calls (prompts of 103–164 tokens, ~6-token answers) writing at ~4.7
    /// tok/s, while its 64k-token turns wrote ~10.6 (the live turn: 10.9) and read ~250 tok/s; the
    /// 2,048 bucket held two runs reading 18 tok/s. The headline was 4.7 writing / 18.0 reading.
    fn tensor_side_calls_and_turns() -> Vec<SpeedRecord> {
        let row = |prompt: u64, answer: u64, decode: f64, prefill: f64, at: u64| SpeedRecord {
            model_id: MODEL.into(),
            placement: split(),
            node_names: vec!["Mihai Macbook".into(), "Studio".into()],
            chips: vec![None, None],
            backend: "mlx_lm".into(),
            context_bucket: crate::placement::store::context_bucket(prompt),
            prompt_tokens: prompt,
            completion_tokens: answer,
            prefill_tps: Some(prefill),
            decode_tps: Some(decode),
            ttft_ms: None,
            recorded_at_ms: at,
            source: RecordSource::Chat,
            workload: None,
            kv_cache: None,
        };
        let mut rows = Vec::new();
        for i in 0..788u64 {
            rows.push(row(
                103 + (i % 2) * 61,
                6 + i % 3,
                2.8 + (i % 15) as f64 * 0.3,
                1.8,
                i,
            ));
        }
        for i in 0..49u64 {
            rows.push(row(
                52_976 + i * 10,
                512,
                10.3 + (i % 7) as f64 * 0.1,
                250.0,
                1_000 + i,
            ));
        }
        rows.push(row(1_500, 33, 10.4, 18.0, 2_000));
        rows.push(row(1_600, 34, 10.4, 18.0, 2_001));
        rows
    }

    #[test]
    fn side_calls_stay_out_of_the_writing_figure_and_reading_is_headlined_at_the_chats_size() {
        let records = tensor_side_calls_and_turns();
        let chat = ChatShape::of(&records);
        assert_eq!(chat.bucket(), 65_536);
        let runs = WayRuns::of(&records, MODEL, &split());
        let writing = runs.writing(chat.bucket());
        let value = writing.figure.as_ref().unwrap().estimate.value;
        assert!((10.3..=10.9).contains(&value), "{writing:?}");
        assert_eq!(writing.elsewhere, 790);
        let reading = runs.reading_at(chat.bucket()).unwrap();
        assert_eq!(reading.estimate.value, 250.0);
        assert_eq!(reading.runs, 49);
    }

    #[test]
    fn a_rate_over_too_few_tokens_does_not_count_and_the_basis_says_so() {
        let records = fixture();
        let writing = WayRuns::of(&records, MODEL, &studio()).writing(256);
        assert!(writing.too_short > 0, "{writing:?}");
        let counted_floor = 1.0 / writing.spread + 1.0;
        for r in records
            .iter()
            .filter(|r| r.placement == studio() && r.context_bucket == 256)
        {
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
        let writing = WayRuns::of(&records, MODEL, &studio()).writing(records[0].context_bucket);
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
