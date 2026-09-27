//! Load measurements (design §6.4 step 10): one row per load that ends, appended to
//! `mlx-load-measurements.jsonl` (S0's `placement::loads` store) at the three ready paths — this
//! Mac's single engine (the manager's observer), a peer's single (the route's follower) and the
//! split (the run's follower) — so Run it's loads are measured too, not only the loader's. A
//! measured time is displayed, never used to decide which node loads (gate 5).
//!
//! A swap whose demanding turn was cancelled after its stops began runs to its end and is
//! recorded `cancelledAfterStop` (step 12): the loader leaves an expectation here that the ready
//! path consumes. A start refused before any load began (a memory gate, a preflight) is not a
//! load: the turn is told in the refusal's words and nothing is measured.

use goose_sidecar::engine::{EngineLoadMeasured, LoadObserver, LoadPhaseTimes};
use goose_sidecar::placement::loads::{
    LoadOutcome, LoadPhasesMs, LoadRecord, LoadStore, LOADS_FILE,
};
use goose_sidecar::placement::store::PlacementKey;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

/// A load the loader started, waiting for the ready path that ends it.
pub(crate) struct Expected {
    model: String,
    key: PlacementKey,
    cancelled: Arc<AtomicBool>,
    recorded: AtomicBool,
}

impl Expected {
    #[cfg(test)]
    pub fn recorded(&self) -> bool {
        self.recorded.load(Ordering::SeqCst)
    }
}

static EXPECTED: StdMutex<Vec<Arc<Expected>>> = StdMutex::new(Vec::new());

/// The loader starts a load of `model` on `key`; `cancelled` flips when its demanding turn goes.
pub(crate) fn expect(model: &str, key: PlacementKey, cancelled: Arc<AtomicBool>) -> Arc<Expected> {
    let expected = Arc::new(Expected {
        model: model.to_string(),
        key,
        cancelled,
        recorded: AtomicBool::new(false),
    });
    EXPECTED.lock().unwrap().push(Arc::clone(&expected));
    expected
}

pub(crate) fn forget(expected: &Arc<Expected>) {
    EXPECTED
        .lock()
        .unwrap()
        .retain(|e| !Arc::ptr_eq(e, expected));
}

/// The outcome a ready path records, consuming the loader's expectation for this load if any.
pub(crate) fn outcome_of(
    model: &str,
    key: &PlacementKey,
    result: &Result<(), String>,
) -> LoadOutcome {
    let expected = {
        let mut all = EXPECTED.lock().unwrap();
        let at = all.iter().position(|e| e.model == model && &e.key == key);
        at.map(|i| all.remove(i))
    };
    if let Some(expected) = &expected {
        expected.recorded.store(true, Ordering::SeqCst);
    }
    match result {
        Err(words) => LoadOutcome::Failed {
            words: words.clone(),
        },
        Ok(()) if expected.is_some_and(|e| e.cancelled.load(Ordering::SeqCst)) => {
            LoadOutcome::CancelledAfterStop
        }
        Ok(()) => LoadOutcome::Ready,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

pub(crate) fn store() -> LoadStore {
    LoadStore::new(crate::config::paths::Paths::in_data_dir(LOADS_FILE))
}

fn append(record: LoadRecord) {
    let store = store();
    if let Err(e) = store.append(&record) {
        tracing::error!(path = %store.path().display(), error = %format!("{e:#}"), model = %record.model, "a measured load could not be recorded");
    }
}

/// What a ready path knows about the load that ended.
pub(crate) struct Ended {
    pub model: String,
    pub key: PlacementKey,
    pub macs: Vec<String>,
    pub weights_bytes: u64,
    pub phases: LoadPhaseTimes,
    pub total_ms: u64,
    pub file_cache_warm: bool,
    pub result: Result<(), String>,
}

pub(crate) fn record(ended: Ended) {
    let outcome = outcome_of(&ended.model, &ended.key, &ended.result);
    append(LoadRecord {
        model: ended.model,
        placement: ended.key,
        macs: ended.macs,
        weights_bytes: ended.weights_bytes,
        phases_ms: LoadPhasesMs {
            starting: ended.phases.starting,
            loading: ended.phases.loading,
            warming: ended.phases.warming,
        },
        total_ms: ended.total_ms,
        file_cache_warm: ended.file_cache_warm,
        outcome,
        recorded_at_ms: now_ms(),
    });
}

/// This Mac's single engine: the manager tells of every load that ends. The warmth is measured
/// (`mincore`); when it could not be read the row is not written — its absence is logged with why,
/// never filled with a guess.
pub(crate) fn single_observer() -> LoadObserver {
    Arc::new(|measured: EngineLoadMeasured| {
        tokio::spawn(async move {
            let warm = match measured.file_cache_warm {
                Ok(warm) => warm,
                Err(why) => {
                    tracing::warn!(model = %measured.model_id, %why, "a load of this Mac's engine ended, but whether its weights were cached could not be read; not recorded");
                    return;
                }
            };
            let macs = match crate::nodes::acp::this_mac_name().await {
                Ok(name) => vec![name],
                Err(why) => {
                    tracing::warn!(%why, "this Mac's name could not be read for a load record; the row names no Mac");
                    Vec::new()
                }
            };
            record(Ended {
                model: measured.model_id,
                key: PlacementKey::single(crate::nodes::THIS_MAC),
                macs,
                weights_bytes: measured.weights_bytes,
                phases: measured.phases,
                total_ms: measured.total_ms,
                file_cache_warm: warm,
                result: measured.outcome,
            });
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_demand_records_its_swap_as_cancelled_after_stop() {
        let key = PlacementKey::single("link:rows-test-peer");
        let cancelled = Arc::new(AtomicBool::new(false));
        let expected = expect("rows/test-a", key.clone(), Arc::clone(&cancelled));
        cancelled.store(true, Ordering::SeqCst);
        assert_eq!(
            outcome_of("rows/test-a", &key, &Ok(())),
            LoadOutcome::CancelledAfterStop
        );
        assert!(expected.recorded());
        assert_eq!(
            outcome_of("rows/test-a", &key, &Ok(())),
            LoadOutcome::Ready,
            "the expectation is consumed once; a later load is an ordinary one"
        );
    }

    #[test]
    fn a_failed_load_keeps_the_engines_words_even_when_cancelled() {
        let key = PlacementKey::single("link:rows-test-peer-b");
        let expected = expect("rows/test-b", key.clone(), Arc::new(AtomicBool::new(true)));
        assert_eq!(
            outcome_of("rows/test-b", &key, &Err("short 1.6 GB".into())),
            LoadOutcome::Failed {
                words: "short 1.6 GB".into()
            }
        );
        assert!(expected.recorded());
    }
}
