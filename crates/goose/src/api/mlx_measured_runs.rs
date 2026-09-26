//! `GET /mlx-engine/measured-runs` — goose's measured runs for the way this goose's MLX chat runs
//! now, read through the ONE reader (`goose_sidecar::placement::runs`) the placement planner uses,
//! and keyed by the ONE function the recorder keys finished turns with
//! (`providers::mlx_speed::serving_way`). Read by the desktop's MAIN process — the menu-bar tray and
//! the chat's reading estimate — which has no ACP client; the Engine tile and the Run it cards read
//! the same figures from the plan. The runs live in goose's measurement store under its data dir,
//! so a relaunch loses none of them (Q-129: the tray kept its own in-memory book and lost them all).

use axum::{http::StatusCode, routing::get, Json, Router};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasuredWay {
    /// The placement id the plan keys candidates by (`single:link:<peer>`, `tensor:jaccl:…`).
    pub placement_id: String,
    pub placement: goose_sidecar::placement::store::PlacementKey,
    pub model_id: String,
    pub node_names: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketFigure {
    /// Prompts of up to this many tokens (the power of two at or above the prompt).
    pub bucket: u64,
    pub figure: goose_sidecar::placement::planner::Figure,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasuredRunsResponse {
    /// `None` = no MLX engine serves this Mac's chat; `way_error` says why.
    pub way: Option<MeasuredWay>,
    pub way_error: Option<String>,
    /// Runs recorded on this way, timed or not.
    pub recorded: usize,
    pub writing: Option<goose_sidecar::placement::planner::Figure>,
    /// How the writing figure was counted, in words (the plan's basis line for it).
    pub writing_basis: Option<String>,
    /// Reading at the chat goal's prompt size — the figure the Run it card's chat plan shows.
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
    use goose_sidecar::placement::bench::Workload;
    use goose_sidecar::placement::runs::WayRuns;

    let way = match way {
        Ok(way) => way,
        Err(why) => {
            return MeasuredRunsResponse {
                way: None,
                way_error: Some(why),
                recorded: 0,
                writing: None,
                writing_basis: None,
                reading: None,
                reading_by_bucket: Vec::new(),
                store_errors: read.unreadable,
            }
        }
    };
    let runs = WayRuns::of(&read.records, &way.model_id, &way.key);
    let writing = runs.writing();
    MeasuredRunsResponse {
        recorded: runs.recorded(),
        writing_basis: writing.basis(),
        writing: writing.figure,
        reading: runs.reading_at(Workload::Chat.bucket()),
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
async fn measured_runs() -> Result<Json<MeasuredRunsResponse>, (StatusCode, String)> {
    Err((
        StatusCode::NOT_IMPLEMENTED,
        "the MLX measurement store requires macOS".to_string(),
    ))
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
        let expected =
            goose_sidecar::placement::runs::WayRuns::of(&read.records, MODEL, &studio().key)
                .writing();
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
        assert!(
            json["reading"].is_null(),
            "no chat-size reading run on this way"
        );
    }

    #[test]
    fn no_way_serving_says_why_never_an_empty_history() {
        let json = serde_json::to_value(answer(real_rows(), Err("nothing serves".into()))).unwrap();
        assert!(json["way"].is_null());
        assert_eq!(json["wayError"], "nothing serves");
        assert!(json["writing"].is_null());
    }
}
