//! Model replication between two in-process endpoints: a real replica listener on an
//! ephemeral loopback port serving a fake model dir, and a real [`ReplicaTracker`] pulling
//! it into another dir. A test-only layer in front of the sender cuts the first transfer of
//! the weights file mid-body, so the resume is a real interrupted HTTP transfer continued
//! with `Range`, not a pre-seeded file. The completed copy must be listed complete by
//! `goose_sidecar::hf::list_local_models` — the same function the Models tab reads.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use bytes::Bytes;
use futures::StreamExt;
use leanzero_link::netpath::LinkKind;
use leanzero_link::replica::{
    replica_router, sha256_file, OfferTicket, PullSpec, ReplicaOffers, ReplicaProgress,
    ReplicaState, ReplicaTracker,
};

const MODEL: &str = "pub/fake-model";
const WEIGHTS: &str = "model.safetensors";
const WEIGHTS_LEN: usize = 6 * 1024 * 1024 + 123;
const CUT_AFTER: usize = 1_500_000;

fn pseudo_random(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

/// A model dir `hf::list_local_models` calls complete: `config.json`, a safetensors file,
/// and a nested file to prove sub-paths survive the wire.
fn make_sender_model(models: &Path) -> PathBuf {
    let root = models.join(MODEL);
    std::fs::create_dir_all(root.join("tokenizer")).unwrap();
    std::fs::write(root.join("config.json"), br#"{"model_type":"fake"}"#).unwrap();
    std::fs::write(root.join(WEIGHTS), pseudo_random(WEIGHTS_LEN, 7)).unwrap();
    std::fs::write(
        root.join("tokenizer/tokenizer.json"),
        pseudo_random(40_000, 11),
    )
    .unwrap();
    root
}

#[derive(Clone, Default)]
struct Wire {
    /// Cut the next weights transfer after `CUT_AFTER` bytes (then clear itself).
    cut_next: Arc<AtomicBool>,
    /// Every `Range` header the sender saw on a weights request (`None` = no header).
    ranges: Arc<StdMutex<Vec<Option<String>>>>,
    /// Slow every body down (the cancel test needs a transfer that is still running).
    throttle: Arc<AtomicBool>,
}

async fn wire_layer(
    axum::extract::State(wire): axum::extract::State<Wire>,
    request: Request,
    next: Next,
) -> Response {
    let is_weights = request.uri().path().ends_with("/file")
        && request
            .uri()
            .query()
            .is_some_and(|q| q.contains(&format!("path={WEIGHTS}")));
    if is_weights {
        let range = request
            .headers()
            .get("range")
            .map(|v| v.to_str().unwrap().to_string());
        wire.ranges.lock().unwrap().push(range);
    }
    let response = next.run(request).await;
    let cut = is_weights && wire.cut_next.swap(false, Ordering::SeqCst);
    let throttle = wire.throttle.load(Ordering::SeqCst);
    if !cut && !throttle {
        return response;
    }
    let (parts, body) = response.into_parts();
    let mut sent = 0usize;
    let stream = body.into_data_stream().flat_map(move |chunk| {
        let chunk = chunk.map_err(std::io::Error::other);
        let pieces: Vec<Result<Bytes, std::io::Error>> = match chunk {
            Ok(bytes) if cut => {
                if sent >= CUT_AFTER {
                    vec![Err(std::io::Error::other("cut by the test wire"))]
                } else {
                    let take = (CUT_AFTER - sent).min(bytes.len());
                    sent += take;
                    let mut out = vec![Ok(bytes.slice(..take))];
                    if sent >= CUT_AFTER {
                        out.push(Err(std::io::Error::other("cut by the test wire")));
                    }
                    out
                }
            }
            Ok(bytes) => bytes
                .chunks(16 * 1024)
                .map(|c| Ok(Bytes::copy_from_slice(c)))
                .collect(),
            Err(e) => vec![Err(e)],
        };
        futures::stream::iter(pieces).then(move |piece| async move {
            if throttle {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            piece
        })
    });
    Response::from_parts(parts, Body::from_stream(stream))
}

async fn serve(offers: ReplicaOffers, wire: Wire) -> String {
    let router =
        replica_router(offers).layer(axum::middleware::from_fn_with_state(wire, wire_layer));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

fn spec(source_url: &str, ticket: &OfferTicket) -> PullSpec {
    PullSpec {
        model_id: MODEL.to_string(),
        source_url: source_url.to_string(),
        offer_token: ticket.token.clone(),
        link: LinkKind::Thunderbolt,
        link_detail: "test loopback".to_string(),
    }
}

fn no_preflight() -> leanzero_link::replica::Preflight {
    Arc::new(|_| Ok(()))
}

async fn until_terminal(tracker: &ReplicaTracker) -> ReplicaProgress {
    for _ in 0..2_000 {
        if let Some(progress) = tracker.progress(MODEL) {
            if !matches!(progress.state, ReplicaState::Queued | ReplicaState::Copying) {
                return progress;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the copy never ended: {:?}", tracker.progress(MODEL));
}

fn assert_identical(sender: &Path, receiver: &Path) {
    for rel in ["config.json", WEIGHTS, "tokenizer/tokenizer.json"] {
        assert_eq!(
            sha256_file(&sender.join(rel)).unwrap(),
            sha256_file(&receiver.join(rel)).unwrap(),
            "{rel} differs"
        );
        assert!(
            !receiver.join(format!("{rel}.part")).exists(),
            "{rel}.part left behind"
        );
    }
}

#[tokio::test]
async fn an_interrupted_copy_resumes_mid_file_with_range_and_verifies() {
    let sender_models = tempfile::tempdir().unwrap();
    let receiver_models = tempfile::tempdir().unwrap();
    let sender_root = make_sender_model(sender_models.path());
    let offers = ReplicaOffers::new();
    let wire = Wire::default();
    let source = serve(offers.clone(), wire.clone()).await;
    let tracker = ReplicaTracker::new();

    // Attempt 1: the weights transfer is cut mid-body.
    wire.cut_next.store(true, Ordering::SeqCst);
    let first = offers.offer(MODEL, &sender_root).unwrap();
    tracker
        .start(
            spec(&source, &first),
            receiver_models.path(),
            no_preflight(),
        )
        .unwrap();
    let failed = until_terminal(&tracker).await;
    assert_eq!(failed.state, ReplicaState::Failed, "{failed:?}");
    let error = failed.error.clone().unwrap();
    assert!(error.contains(WEIGHTS), "{error}");
    let part = receiver_models
        .path()
        .join(MODEL)
        .join(format!("{WEIGHTS}.part"));
    let kept = std::fs::metadata(&part).unwrap().len();
    assert!(
        kept > 0 && kept < WEIGHTS_LEN as u64,
        "the interrupted .part must hold a strict prefix, holds {kept}"
    );
    assert_eq!(failed.files_done, 1, "config.json landed before the cut");
    assert_eq!(offers.active(), 0, "a failed copy releases its offer");
    // The model is NOT listed complete while a .part remains.
    let listed = goose_sidecar::hf::list_local_models(receiver_models.path()).unwrap();
    assert!(
        listed.iter().all(|m| m.id != MODEL || !m.complete),
        "{listed:?}"
    );

    // Attempt 2: a fresh offer (the owner's retry), the .part continues with Range.
    let second = offers.offer(MODEL, &sender_root).unwrap();
    tracker
        .start(
            spec(&source, &second),
            receiver_models.path(),
            no_preflight(),
        )
        .unwrap();
    let done = until_terminal(&tracker).await;
    assert_eq!(done.state, ReplicaState::Done, "{done:?}");
    assert_eq!(done.resumed_files, [WEIGHTS.to_string()]);
    assert!(done.restarted_files.is_empty(), "{done:?}");
    assert_eq!(done.skipped_files, ["config.json".to_string()]);
    assert_eq!(done.copied_bytes, done.total_bytes);
    assert_eq!(done.files_done, 3);
    assert_eq!(
        done.wire_bytes,
        done.total_bytes - kept - 21,
        "only the missing suffix and the tokenizer crossed the wire (config.json is 21 bytes)"
    );
    let ranges = wire.ranges.lock().unwrap().clone();
    assert_eq!(ranges, [None, Some(format!("bytes={kept}-"))]);
    assert_identical(&sender_root, &receiver_models.path().join(MODEL));
    assert_eq!(offers.active(), 0, "a finished copy releases its offer");

    let listed = goose_sidecar::hf::list_local_models(receiver_models.path()).unwrap();
    let model = listed.iter().find(|m| m.id == MODEL).expect("listed");
    assert!(model.complete, "{model:?}");
    assert_eq!(model.size_bytes, done.total_bytes);
}

/// NEGATIVE CONTROL for verification: a `.part` whose prefix is NOT the sender's bytes
/// resumes cleanly over HTTP, and only the SHA-256 comparison can catch it. It must fail
/// loudly, remove the bad `.part`, leave no final file — and the next attempt starts clean.
#[tokio::test]
async fn a_corrupt_resumed_prefix_fails_verification_and_is_dropped() {
    let sender_models = tempfile::tempdir().unwrap();
    let receiver_models = tempfile::tempdir().unwrap();
    let sender_root = make_sender_model(sender_models.path());
    let offers = ReplicaOffers::new();
    let wire = Wire::default();
    let source = serve(offers.clone(), wire.clone()).await;
    let tracker = ReplicaTracker::new();

    let dest = receiver_models.path().join(MODEL);
    std::fs::create_dir_all(&dest).unwrap();
    let part = dest.join(format!("{WEIGHTS}.part"));
    std::fs::write(&part, pseudo_random(1_000_000, 99)).unwrap();

    let ticket = offers.offer(MODEL, &sender_root).unwrap();
    tracker
        .start(
            spec(&source, &ticket),
            receiver_models.path(),
            no_preflight(),
        )
        .unwrap();
    let failed = until_terminal(&tracker).await;
    assert_eq!(failed.state, ReplicaState::Failed, "{failed:?}");
    let error = failed.error.unwrap();
    assert!(error.contains("failed verification"), "{error}");
    assert_eq!(failed.resumed_files, [WEIGHTS.to_string()]);
    assert!(!part.exists(), "the corrupt prefix must be removed");
    assert!(
        !dest.join(WEIGHTS).exists(),
        "no unverified file takes the final name"
    );

    let retry = offers.offer(MODEL, &sender_root).unwrap();
    tracker
        .start(
            spec(&source, &retry),
            receiver_models.path(),
            no_preflight(),
        )
        .unwrap();
    let done = until_terminal(&tracker).await;
    assert_eq!(done.state, ReplicaState::Done, "{done:?}");
    assert!(done.resumed_files.is_empty());
    assert_identical(&sender_root, &dest);
}

#[tokio::test]
async fn a_preflight_refusal_writes_nothing() {
    let sender_models = tempfile::tempdir().unwrap();
    let receiver_models = tempfile::tempdir().unwrap();
    let sender_root = make_sender_model(sender_models.path());
    let offers = ReplicaOffers::new();
    let source = serve(offers.clone(), Wire::default()).await;
    let tracker = ReplicaTracker::new();
    let ticket = offers.offer(MODEL, &sender_root).unwrap();
    let refuse: leanzero_link::replica::Preflight =
        Arc::new(|manifest| Err(format!("needs {} bytes, 10 free", manifest.total_bytes)));
    tracker
        .start(spec(&source, &ticket), receiver_models.path(), refuse)
        .unwrap();
    let failed = until_terminal(&tracker).await;
    assert_eq!(failed.state, ReplicaState::Failed);
    assert!(failed.error.unwrap().contains("10 free"));
    assert!(!receiver_models.path().join(MODEL).exists());
}

#[tokio::test]
async fn cancel_stops_mid_transfer_and_deletes_the_partial_copy() {
    let sender_models = tempfile::tempdir().unwrap();
    let receiver_models = tempfile::tempdir().unwrap();
    let sender_root = make_sender_model(sender_models.path());
    let offers = ReplicaOffers::new();
    let wire = Wire::default();
    wire.throttle.store(true, Ordering::SeqCst);
    let source = serve(offers.clone(), wire.clone()).await;
    let tracker = ReplicaTracker::new();
    let ticket = offers.offer(MODEL, &sender_root).unwrap();
    tracker
        .start(
            spec(&source, &ticket),
            receiver_models.path(),
            no_preflight(),
        )
        .unwrap();
    for _ in 0..2_000 {
        let progress = tracker.progress(MODEL).unwrap();
        if progress.current_file.as_deref() == Some(WEIGHTS) && progress.copied_bytes > 100_000 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(tracker.is_active(MODEL));
    assert!(
        tracker
            .start(
                spec(&source, &ticket),
                receiver_models.path(),
                no_preflight()
            )
            .is_err(),
        "a second start while copying is refused"
    );
    tracker.cancel(MODEL).unwrap();
    let cancelled = until_terminal(&tracker).await;
    assert_eq!(cancelled.state, ReplicaState::Cancelled, "{cancelled:?}");
    assert!(!receiver_models.path().join(MODEL).exists());
    assert_eq!(offers.active(), 0);
}

#[tokio::test]
async fn the_listener_admits_only_the_offer_token_for_its_own_model() {
    let sender_models = tempfile::tempdir().unwrap();
    let sender_root = make_sender_model(sender_models.path());
    let other_root = sender_models.path().join("pub/other");
    std::fs::create_dir_all(&other_root).unwrap();
    std::fs::write(other_root.join("config.json"), b"{}").unwrap();
    let offers = ReplicaOffers::new();
    let source = serve(offers.clone(), Wire::default()).await;
    let ticket = offers.offer(MODEL, &sender_root).unwrap();
    let other = offers.offer("pub/other", &other_root).unwrap();
    let client = reqwest::Client::new();
    let manifest = format!("{source}/v1/swarm/replica/{MODEL}/manifest");
    let get = |url: String, token: Option<&str>| {
        let mut request = client.get(url);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        request
    };

    assert_eq!(
        get(manifest.clone(), None).send().await.unwrap().status(),
        401
    );
    assert_eq!(
        get(manifest.clone(), Some(&"f".repeat(64)))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        get(manifest.clone(), Some(&other.token))
            .send()
            .await
            .unwrap()
            .status(),
        401,
        "a token minted for another model never reads this one"
    );
    assert_eq!(
        get(manifest.clone(), Some(&ticket.token))
            .header("origin", "https://evil.example")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let ok = get(manifest.clone(), Some(&ticket.token))
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
    let listed: leanzero_link::replica::ReplicaManifest = ok.json().await.unwrap();
    assert_eq!(listed, ticket.manifest);

    let file = |path: &str| format!("{source}/v1/swarm/replica/{MODEL}/file?path={path}");
    assert_eq!(
        get(file("../../etc/passwd"), Some(&ticket.token))
            .send()
            .await
            .unwrap()
            .status(),
        404,
        "only manifest paths are served; nothing is joined from the request"
    );
    let past_end = get(file("config.json"), Some(&ticket.token))
        .header("range", "bytes=21-")
        .send()
        .await
        .unwrap();
    assert_eq!(past_end.status(), 416);
    assert_eq!(past_end.headers()["content-range"], "bytes */21");
    let tail = get(file("config.json"), Some(&ticket.token))
        .header("range", "bytes=15-")
        .send()
        .await
        .unwrap();
    assert_eq!(tail.status(), 206);
    assert_eq!(tail.headers()["content-range"], "bytes 15-20/21");
    assert_eq!(tail.bytes().await.unwrap().as_ref(), b"fake\"}");

    let digest: leanzero_link::replica::FileDigest = get(
        format!("{source}/v1/swarm/replica/{MODEL}/sha256?path=config.json"),
        Some(&ticket.token),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(
        digest.sha256,
        sha256_file(&sender_root.join("config.json")).unwrap().1
    );

    // A file that changed size since the offer is refused, not served as the offer.
    std::fs::write(sender_root.join("config.json"), b"{}").unwrap();
    assert_eq!(
        get(file("config.json"), Some(&ticket.token))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );

    let release = client
        .delete(format!("{source}/v1/swarm/replica/{MODEL}/offer"))
        .bearer_auth(&ticket.token)
        .send()
        .await
        .unwrap();
    assert_eq!(release.status(), 204);
    assert_eq!(
        get(manifest, Some(&ticket.token))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(offers.active(), 1, "the other model's offer is untouched");
}
