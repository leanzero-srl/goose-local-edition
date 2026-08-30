//! Live HuggingFace integration proof: search parsing, tree listing, and a real snapshot
//! download of a ~21 MB test repo through the DownloadTracker. Network-dependent and
//! therefore ignored by default; run explicitly with
//! `cargo test -p goose-sidecar --features rustls-tls --test hf_live -- --ignored`.

use std::time::{Duration, Instant};

use goose_sidecar::hf::{
    list_local_models, repo_files, search_mlx_models, DownloadState, DownloadTracker,
};

const TINY_REPO: &str = "trl-internal-testing/tiny-Qwen2ForCausalLM-2.5";

#[tokio::test]
#[ignore = "hits huggingface.co; run with -- --ignored"]
async fn search_returns_mlx_hits_with_metadata() {
    let hits = search_mlx_models("qwen", 5, None).await.unwrap();
    assert!(!hits.is_empty(), "search returned no hits");
    for hit in &hits {
        assert!(hit.id.contains('/'), "unexpected id shape: {}", hit.id);
        assert!(
            !hit.updated_at.is_empty(),
            "missing lastModified for {}",
            hit.id
        );
    }
    assert!(
        hits.windows(2).all(|w| w[0].downloads >= w[1].downloads),
        "hits not sorted by downloads"
    );
}

#[tokio::test]
#[ignore = "hits huggingface.co; run with -- --ignored"]
async fn repo_files_lists_blobs_with_sizes() {
    let files = repo_files(TINY_REPO, None).await.unwrap();
    let config = files
        .iter()
        .find(|f| f.path == "config.json")
        .expect("config.json listed");
    assert!(config.size > 0);
    assert!(files.iter().any(|f| f.path.ends_with(".safetensors")));
}

#[tokio::test]
#[ignore = "hits huggingface.co; run with -- --ignored"]
async fn download_tracker_snapshots_a_real_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let tracker = DownloadTracker::new();
    tracker.start_download(TINY_REPO, tmp.path(), None).unwrap();

    let deadline = Instant::now() + Duration::from_secs(180);
    let progress = loop {
        let progress = tracker.progress(TINY_REPO).expect("progress tracked");
        match progress.state {
            DownloadState::Done | DownloadState::Failed | DownloadState::Cancelled => {
                break progress
            }
            _ => {
                assert!(Instant::now() < deadline, "download did not finish in time");
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    };
    assert_eq!(
        progress.state,
        DownloadState::Done,
        "download failed: {:?}",
        progress.error
    );
    assert_eq!(progress.downloaded_bytes, progress.total_bytes);

    let models = list_local_models(tmp.path()).unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, TINY_REPO);
    assert!(models[0].complete, "downloaded snapshot must be complete");
    assert_eq!(models[0].size_bytes, progress.total_bytes);
}
