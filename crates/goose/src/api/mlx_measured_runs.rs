//! `GET /mlx-engine/measured-runs` — goose's measured runs for the way this goose's MLX chat runs
//! now, read through the ONE reader (`goose_sidecar::placement::runs`) the placement planner uses,
//! and keyed by the ONE function the recorder keys finished turns with
//! (`providers::mlx_speed::serving_way`). Read by the desktop's MAIN process — the menu-bar tray and
//! the chat's reading estimate — which has no ACP client; the Engine tile and the Run it cards read
//! the same figures from the plan. The runs live in goose's measurement store under its data dir,
//! so a relaunch loses none of them (Q-129: the tray kept its own in-memory book and lost them all).

use axum::{http::StatusCode, routing::get, Router};

// `goose_sidecar::placement` compiles only on Unix, so the response it shapes does too; on any
// other platform the route answers 501 by name (`measured_runs` below).
#[cfg(unix)]
use axum::Json;
#[cfg(unix)]
use serde::Serialize;

#[cfg(unix)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasuredWay {
    /// The placement id the plan keys candidates by (`single:link:<peer>`, `tensor:jaccl:…`).
    pub placement_id: String,
    pub placement: goose_sidecar::placement::store::PlacementKey,
    pub model_id: String,
    pub node_names: Vec<String>,
}

#[cfg(unix)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketFigure {
    /// Prompts of up to this many tokens (the power of two at or above the prompt).
    pub bucket: u64,
    pub figure: goose_sidecar::placement::planner::Figure,
}

#[cfg(unix)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasuredRunsResponse {
    /// `None` = no MLX engine serves this Mac's chat; `way_error` says why.
    pub way: Option<MeasuredWay>,
    pub way_error: Option<String>,
    /// Runs recorded on this way, timed or not.
    pub recorded: usize,
    /// The prompt size `writing` and `reading` are for: prompts of up to this many tokens — the
    /// bucket of this app's typical chat prompt (`chatPromptTokens`), the benchmark's 2,048 while
    /// no chat is recorded. The tray names it beside the rates (Q-168).
    pub prompt_bucket: u64,
    /// This app's typical chat prompt in tokens (the token-weighted median of every recorded chat
    /// turn); `None` = no chat turn is recorded yet, and `promptBucket` is the benchmark's.
    pub chat_prompt_tokens: Option<u64>,
    /// Writing at `promptBucket`: goose's own side calls (a title, a check — tiny prompts, a
    /// handful of tokens written) are left out, their rate is mostly start-up.
    pub writing: Option<goose_sidecar::placement::planner::Figure>,
    /// How the writing figure was counted, in words (the plan's basis line for it).
    pub writing_basis: Option<String>,
    /// Reading at `promptBucket` — the figure the Run it card's chat plan shows.
    pub reading: Option<goose_sidecar::placement::planner::Figure>,
    /// Reading per prompt bucket, smallest first: a prompt's reading time is estimated from ITS size.
    pub reading_by_bucket: Vec<BucketFigure>,
    /// `line N: <why>` for every store line that did not parse.
    pub store_errors: Vec<String>,
}

#[cfg(unix)]
fn answer(
    read: goose_sidecar::placement::store::StoreRead,
    way: Result<crate::providers::mlx_speed::ServingWay, String>,
) -> MeasuredRunsResponse {
    use goose_sidecar::placement::runs::{ChatShape, WayRuns};

    let chat = ChatShape::of(&read.records);
    let chat_prompt_tokens = (chat.turns > 0).then_some(chat.prompt_tokens);
    let way = match way {
        Ok(way) => way,
        Err(why) => {
            return MeasuredRunsResponse {
                way: None,
                way_error: Some(why),
                recorded: 0,
                prompt_bucket: chat.bucket(),
                chat_prompt_tokens,
                writing: None,
                writing_basis: None,
                reading: None,
                reading_by_bucket: Vec::new(),
                store_errors: read.unreadable,
            }
        }
    };
    let runs = WayRuns::of(&read.records, &way.model_id, &way.key);
    let writing = runs.writing(chat.bucket());
    MeasuredRunsResponse {
        recorded: runs.recorded(),
        prompt_bucket: chat.bucket(),
        chat_prompt_tokens,
        writing_basis: writing.basis(),
        writing: writing.figure,
        reading: runs.reading_at(chat.bucket()),
        reading_by_bucket: runs
            .reading_by_bucket()
            .into_iter()
            .map(|(bucket, figure)| BucketFigure { bucket, figure })
            .collect(),
        way: Some(MeasuredWay {
            placement_id: way.key.id(),
            placement: way.key,
            model_id: way.model_id,
            node_names: way.node_names,
        }),
        way_error: None,
        store_errors: read.unreadable,
    }
}

#[cfg(unix)]
async fn measured_runs() -> Result<Json<MeasuredRunsResponse>, (StatusCode, String)> {
    use crate::providers::mlx_speed::{serving_way, speed_store};

    let read = speed_store()
        .read()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;
    Ok(Json(answer(read, serving_way().await)))
}

#[cfg(not(unix))]
async fn measured_runs() -> (StatusCode, String) {
    (
        StatusCode::NOT_IMPLEMENTED,
        "the MLX measurement store requires macOS".to_string(),
    )
}

pub fn routes() -> Router {
    Router::new().route("/mlx-engine/measured-runs", get(measured_runs))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::providers::mlx_speed::ServingWay;
    use goose_sidecar::placement::store::{PlacementKey, SpeedStore, StoreRead};

    const MODEL: &str = "owner/Qwen3.8-27B-Q8-mlx";

    /// Real rows of the measurement store (goose-sidecar's fixture, the peer anonymised).
    fn real_rows() -> StoreRead {
        SpeedStore::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../goose-sidecar/tests/fixtures/mlx-speed-measurements.sample.jsonl"
        ))
        .read()
        .unwrap()
    }

    fn studio() -> ServingWay {
        ServingWay {
            key: PlacementKey::single("link:studio-peer"),
            model_id: MODEL.into(),
            node_names: vec!["Studio".into()],
            peers: 0,
        }
    }

    #[test]
    fn the_tray_reads_the_same_figure_the_plan_counts() {
        let read = real_rows();
        let bucket = goose_sidecar::placement::runs::ChatShape::of(&read.records).bucket();
        let expected =
            goose_sidecar::placement::runs::WayRuns::of(&read.records, MODEL, &studio().key)
                .writing(bucket);
        let json = serde_json::to_value(answer(real_rows(), Ok(studio()))).unwrap();
        assert_eq!(json["way"]["placementId"], "single:link:studio-peer");
        assert_eq!(json["recorded"], 60);
        assert_eq!(
            json["writing"]["runs"],
            expected.figure.as_ref().unwrap().runs
        );
        assert_eq!(json["writing"]["measured"], true);
        assert!(json["writing"]["estimate"]["value"].is_f64());
        assert!(json["writingBasis"]
            .as_str()
            .unwrap()
            .starts_with("writing: the median of"));
        let buckets = json["readingByBucket"].as_array().unwrap();
        assert!(!buckets.is_empty());
        assert!(buckets
            .iter()
            .all(|b| b["bucket"].is_u64() && b["figure"]["runs"].is_u64()));
        assert_eq!(json["promptBucket"], 65_536);
        assert!(json["chatPromptTokens"].as_u64().unwrap() > 32_768);
        let at_64k = buckets.iter().find(|b| b["bucket"] == 65_536).unwrap();
        assert_eq!(
            json["reading"], at_64k["figure"],
            "the headline is the bucket of this app's chats"
        );
    }

    /// Q-168 (3.0.57): the tray and the Engine card read "4.7 tok/s writing" and "18.0 tok/s
    /// reading, median of 2 prompts" for the tensor split. The reading was the benchmark's 2,048
    /// bucket (2 runs) while the split's 49 turns at 64k — where this app's chats are — read ~250;
    /// the writing median was dragged by 788 side calls (~6 tokens written each) to 4.7 while its
    /// turns wrote ~10.6 and the live turn 10.9.
    #[test]
    fn the_headline_is_this_apps_chat_size_and_side_calls_do_not_write_it() {
        use goose_sidecar::placement::store::{
            context_bucket, PlacementKind, RecordSource, SpeedRecord,
        };
        let split = PlacementKey {
            kind: PlacementKind::Tensor,
            nodes: vec!["local".into(), "link:studio-peer".into()],
            link: Some("jaccl".into()),
        };
        let row = |prompt: u64, answer: u64, decode: f64, prefill: f64, at: u64| SpeedRecord {
            model_id: MODEL.into(),
            placement: split.clone(),
            node_names: vec!["Mihai Macbook".into(), "Studio".into()],
            chips: vec![None, None],
            backend: "mlx_lm".into(),
            context_bucket: context_bucket(prompt),
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
        let mut records = Vec::new();
        for i in 0..788u64 {
            records.push(row(
                103 + (i % 2) * 61,
                6 + i % 3,
                2.8 + (i % 15) as f64 * 0.3,
                1.8,
                i,
            ));
        }
        for i in 0..49u64 {
            records.push(row(
                52_976 + i * 10,
                512,
                10.3 + (i % 7) as f64 * 0.1,
                250.0,
                900 + i,
            ));
        }
        records.push(row(1_500, 33, 10.4, 18.0, 2_000));
        records.push(row(1_600, 34, 10.4, 18.0, 2_001));
        let way = ServingWay {
            key: split.clone(),
            model_id: MODEL.into(),
            node_names: vec!["Mihai Macbook".into(), "Studio".into()],
            peers: 1,
        };
        let json = serde_json::to_value(answer(
            StoreRead {
                records,
                unreadable: Vec::new(),
            },
            Ok(way),
        ))
        .unwrap();
        let reading = json["reading"]["estimate"]["value"].as_f64().unwrap();
        let writing = json["writing"]["estimate"]["value"].as_f64().unwrap();
        assert_eq!(reading, 250.0, "{json}");
        assert!((10.3..=10.9).contains(&writing), "{json}");
        assert_eq!(json["reading"]["runs"], 49);
        assert_eq!(json["promptBucket"], 65_536);
        assert!(json["chatPromptTokens"].as_u64().unwrap() >= 52_976);
    }

    #[test]
    fn no_way_serving_says_why_never_an_empty_history() {
        let json = serde_json::to_value(answer(real_rows(), Err("nothing serves".into()))).unwrap();
        assert!(json["way"].is_null());
        assert_eq!(json["wayError"], "nothing serves");
        assert!(json["writing"].is_null());
    }
}
