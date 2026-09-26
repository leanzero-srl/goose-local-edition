//! Q-115: listing sessions must not write more than one copy of what it returns.
//!
//! macOS reported the desktop's `goose` dirtying 137.44 GB in 1,267 s. The writer was the
//! session list: `sessions LEFT JOIN messages GROUP BY s.id` built its GROUP BY in a SQLite
//! temp B-tree holding one row per MESSAGE, each carrying the session's wide columns, so one
//! call on the owner's 1.9 GB store wrote ~106 MB of temp file, and the LeanZero Link pollers
//! call it continuously (the report's 108.5 MB/s is about one such call per second). This test
//! lives in its own binary because it reads the process-wide write counter, which a parallel
//! unit test would pollute.

#![cfg(any(target_os = "macos", target_os = "linux"))]

use goose::config::GooseMode;
use goose::conversation::message::Message;
use goose::conversation::Conversation;
use goose::session::{ExtensionData, SessionManager, SessionType};

/// Bytes this process has written so far, through the counter each OS keeps: on macOS the
/// logical-writes counter behind the disk-writes resource report, on Linux `wchar`.
fn process_bytes_written() -> u64 {
    #[cfg(target_os = "macos")]
    {
        let mut info = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
        let rc = unsafe {
            libc::proc_pid_rusage(
                libc::getpid(),
                libc::RUSAGE_INFO_V4,
                info.as_mut_ptr() as *mut libc::rusage_info_t,
            )
        };
        assert_eq!(rc, 0, "proc_pid_rusage failed");
        unsafe { info.assume_init() }.ri_logical_writes
    }
    #[cfg(target_os = "linux")]
    {
        let io = std::fs::read_to_string("/proc/self/io").expect("/proc/self/io");
        io.lines()
            .find_map(|line| line.strip_prefix("wchar: "))
            .expect("wchar line")
            .trim()
            .parse()
            .expect("wchar value")
    }
}

#[tokio::test(flavor = "current_thread")]
async fn listing_sessions_writes_less_than_one_copy_of_the_listing() {
    // Wide session rows (the real store's extension_data is ~3 KB) and enough messages per
    // session that a per-message copy of them cannot fit SQLite's in-memory temp cache.
    let sessions = 40;
    let messages_per_session = 250;
    let wide_state = "x".repeat(4096);

    let data_root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let manager = SessionManager::new(data_root.path().to_path_buf());

    for i in 0..sessions {
        let session = manager
            .create_session(
                cwd.path().to_path_buf(),
                format!("session {i}"),
                SessionType::User,
                GooseMode::default(),
            )
            .await
            .unwrap();
        let mut extension_data = ExtensionData::new();
        extension_data.set_extension_state("q115", "v0", serde_json::json!(wide_state));
        manager
            .update(&session.id)
            .extension_data(extension_data)
            .apply()
            .await
            .unwrap();
        let messages = (0..messages_per_session)
            .map(|n| {
                let mut message = Message::user().with_text(format!("message {n}"));
                message.created = 1_790_000_000 + i * 1_000 + n;
                message
            })
            .collect::<Vec<_>>();
        manager
            .replace_conversation(&session.id, &Conversation::new_unvalidated(messages))
            .await
            .unwrap();
    }

    let listing = manager.list_sessions().await.unwrap();
    let listed_bytes: u64 = listing
        .iter()
        .map(|session| serde_json::to_string(session).unwrap().len() as u64)
        .sum();

    let before = process_bytes_written();
    let listing = manager.list_sessions().await.unwrap();
    let written = process_bytes_written() - before;

    assert_eq!(listing.len(), sessions as usize);
    assert!(listing
        .iter()
        .all(|session| session.message_count == messages_per_session as usize));
    assert!(
        listing
            .windows(2)
            .all(|pair| pair[0].last_message_at >= pair[1].last_message_at),
        "newest activity first"
    );
    assert!(
        written < listed_bytes,
        "listing {} sessions wrote {written} bytes, more than the {listed_bytes} bytes it returned",
        listing.len()
    );
}
