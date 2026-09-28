use crate::config::paths::Paths;
use anyhow::{anyhow, Result};
use fs_err::File;
use goose_providers::request_log::{install_logger, RequestLogHandle, RequestLogger};
use serde_json::Value;
use std::collections::HashMap;
use std::error::Error;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub fn filter_extensions_from_system_prompt(system: &str) -> String {
    let Some(extensions_start) = system.find("# Extensions") else {
        return system.to_string();
    };

    let Some(after_extensions) = system.get(extensions_start + 1..) else {
        return system.to_string();
    };

    if let Some(next_section_pos) = after_extensions.find("\n# ") {
        let Some(before) = system.get(..extensions_start) else {
            return system.to_string();
        };
        let Some(after) = system.get(extensions_start + next_section_pos + 1..) else {
            return system.to_string();
        };
        format!("{}{}", before.trim_end(), after)
    } else {
        system
            .get(..extensions_start)
            .map(|s| s.trim_end().to_string())
            .unwrap_or_else(|| system.to_string())
    }
}

pub fn is_google_model(payload: &Value) -> bool {
    payload
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_lowercase()
        .contains("google")
}

/// Extract the model name from a JSON object. Common with most providers to have this top level attribute.
pub fn get_model(data: &Value) -> String {
    if let Some(model) = data.get("model") {
        if let Some(model_str) = model.as_str() {
            model_str.to_string()
        } else {
            "Unknown".to_string()
        }
    } else {
        "Unknown".to_string()
    }
}

pub fn unescape_json_values(value: &Value) -> Value {
    let mut cloned = value.clone();
    unescape_json_values_in_place(&mut cloned);
    cloned
}

fn unescape_json_values_in_place(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for v in map.values_mut() {
                unescape_json_values_in_place(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                unescape_json_values_in_place(v);
            }
        }
        Value::String(s) => {
            if s.contains('\\') {
                *s = s
                    .replace("\\\\n", "\n")
                    .replace("\\\\t", "\t")
                    .replace("\\\\r", "\r")
                    .replace("\\\\\"", "\"")
                    .replace("\\n", "\n")
                    .replace("\\t", "\t")
                    .replace("\\r", "\r")
                    .replace("\\\"", "\"");
            }
        }
        _ => {}
    }
}

pub const LOGS_TO_KEEP: usize = 10;

static INIT_LOGGER: OnceLock<Result<()>> = OnceLock::new();
/// When this process's request log started: nothing it writes is older.
static LOGGER_START: OnceLock<SystemTime> = OnceLock::new();
/// The installed logger's requests still in flight, for `end_request_logs_at_exit`.
static OPEN_LOGS: OnceLock<Arc<OpenLogs>> = OnceLock::new();

pub fn init_goose_request_log() -> Result<()> {
    INIT_LOGGER
        .get_or_init(|| {
            LOGGER_START.get_or_init(SystemTime::now);
            let log = RequestLog::new(LOGS_TO_KEEP)?;
            OPEN_LOGS.get_or_init(|| log.open.clone());
            Ok(install_logger(log)?)
        })
        .as_ref()
        .map_err(|e| anyhow::anyhow!("failed to set up logger: {}", e))?;
    Ok(())
}

/// Every request is logged under an in-flight name — `llm_request.<pid>.<uuid>.jsonl`, the writer's
/// pid in it — and renamed into the numbered rotation (`llm_request.0.jsonl` newest) when its
/// handle drops, however the request ended. `goose serve` exits with `std::process::exit` after its
/// teardown, which drops nothing: `end_request_logs_at_exit` ends what is still in flight first. A
/// process that dies unannounced (a SIGKILL after the quit grace, a crash) ends nothing:
/// `sweep_request_log_leftovers` removes those at the next start (Q-343: 2,684 had piled up since
/// June, every burst stamped seconds before a `goose serve` teardown).
pub struct RequestLog {
    logs_dir: PathBuf,
    logs_to_keep: usize,
    open: Arc<OpenLogs>,
}

impl RequestLog {
    pub fn new(logs_to_keep: usize) -> Result<Self> {
        Self::in_dir(Paths::in_state_dir("logs"), logs_to_keep)
    }

    fn in_dir(logs_dir: PathBuf, logs_to_keep: usize) -> Result<Self> {
        fs_err::create_dir_all(&logs_dir)?;
        Ok(Self {
            logs_dir,
            logs_to_keep,
            open: Arc::default(),
        })
    }
}

/// One request's log while it is written.
struct OpenLog {
    writer: Option<BufWriter<File>>,
    temp_path: PathBuf,
    logs_dir: PathBuf,
    logs_to_keep: usize,
    /// The process's exit ended it: the request is cut, and what its stream still sends in the
    /// moments before the exit has nowhere to go.
    ended_by_exit: bool,
}

/// The requests in flight, by id.
#[derive(Default)]
struct OpenLogs(Mutex<HashMap<Uuid, Arc<Mutex<OpenLog>>>>);

/// A request log's lock outlives a panic in its holder: the log is still ended.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct FileLogHandle {
    id: Uuid,
    log: Arc<Mutex<OpenLog>>,
    open: Arc<OpenLogs>,
}

impl RequestLogger for RequestLog {
    fn start(&self) -> Result<Box<dyn RequestLogHandle>, Box<dyn Error + Send + Sync>> {
        fs_err::create_dir_all(&self.logs_dir)?;

        let id = Uuid::new_v4();
        let temp_name = format!("llm_request.{}.{id}.jsonl", std::process::id());
        let temp_path = self.logs_dir.join(temp_name);

        let writer = BufWriter::new(
            File::options()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temp_path)?,
        );

        let log = Arc::new(Mutex::new(OpenLog {
            writer: Some(writer),
            temp_path,
            logs_dir: self.logs_dir.clone(),
            logs_to_keep: self.logs_to_keep,
            ended_by_exit: false,
        }));
        lock(&self.open.0).insert(id, log.clone());
        Ok(Box::new(FileLogHandle {
            id,
            log,
            open: self.open.clone(),
        }))
    }
}

impl RequestLogHandle for FileLogHandle {
    fn write(&mut self, s: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut log = lock(&self.log);
        if log.ended_by_exit {
            return Ok(());
        }
        let writer = log
            .writer
            .as_mut()
            .ok_or_else(|| anyhow!("logger is finished"))?;
        writeln!(writer, "{}", s)?;
        Ok(())
    }
}

impl OpenLog {
    fn finish(&mut self) -> Result<()> {
        if let Some(mut writer) = self.writer.take() {
            writer.flush()?;
            let log_path = |i| self.logs_dir.join(format!("llm_request.{}.jsonl", i));

            if self.logs_to_keep == 0 {
                fs_err::remove_file(&self.temp_path)?;
                return Ok(());
            }

            for i in (0..self.logs_to_keep.saturating_sub(1)).rev() {
                let _ = fs_err::rename(log_path(i), log_path(i + 1));
            }

            fs_err::rename(&self.temp_path, log_path(0))?;
        }
        Ok(())
    }

    /// Leave the in-flight name: rotated, or removed with the reason named. Never panics, so a
    /// panic unwinding through a request ends its log too.
    fn close(&mut self) {
        let Err(error) = self.finish() else {
            return;
        };
        match fs_err::remove_file(&self.temp_path) {
            Ok(()) => tracing::warn!(
                log = %self.temp_path.display(),
                error = %error,
                "request log could not be rotated; removed it"
            ),
            Err(remove) if remove.kind() == std::io::ErrorKind::NotFound => tracing::warn!(
                log = %self.temp_path.display(),
                error = %error,
                "request log could not be rotated and is already gone"
            ),
            Err(remove) => tracing::warn!(
                log = %self.temp_path.display(),
                error = %error,
                remove_error = %remove,
                "request log could not be rotated or removed; the next start sweeps it"
            ),
        }
    }
}

impl Drop for FileLogHandle {
    /// However the request ended — answered, failed, its stream dropped by a cancel, or a panic
    /// unwinding through it.
    fn drop(&mut self) {
        lock(&self.log).close();
        lock(&self.open.0).remove(&self.id);
    }
}

impl OpenLogs {
    /// End every request still in flight: its log gets a last `error` line naming `cause` and is
    /// rotated. Returns how many were ended.
    fn end_all(&self, cause: &str) -> usize {
        let open: Vec<_> = lock(&self.0).drain().map(|(_, log)| log).collect();
        for log in &open {
            let mut log = lock(log);
            if let Some(writer) = log.writer.as_mut() {
                let line = serde_json::json!({ "error": cause });
                if let Err(error) = writeln!(writer, "{line}") {
                    tracing::warn!(
                        log = %log.temp_path.display(),
                        error = %error,
                        "request log: the exit's line could not be written"
                    );
                }
            }
            log.ended_by_exit = true;
            log.close();
        }
        open.len()
    }
}

/// Before an exit that drops nothing (`std::process::exit`): end every request log still in
/// flight, its last line naming `cause`, so none is left under its in-flight name. Returns how many
/// were ended.
pub fn end_request_logs_at_exit(cause: &str) -> usize {
    OPEN_LOGS.get().map_or(0, |open| open.end_all(cause))
}

/// Which writer an in-flight request log belongs to, from its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InFlightLog {
    /// `llm_request.<uuid>.jsonl`, written by a goose older than Q-343: no writer is named.
    Unnamed,
    /// `llm_request.<pid>.<uuid>.jsonl`.
    Writer(u32),
}

/// The in-flight log a file name is, or None for anything else — the numbered rotation included.
fn in_flight_log(file_name: &str) -> Option<InFlightLog> {
    let stem = file_name
        .strip_prefix("llm_request.")?
        .strip_suffix(".jsonl")?;
    if Uuid::try_parse(stem).is_ok() {
        return Some(InFlightLog::Unnamed);
    }
    let (pid, id) = stem.split_once('.')?;
    Uuid::try_parse(id).ok()?;
    pid.parse().ok().map(InFlightLog::Writer)
}

/// What one sweep of the logs folder did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LeftoverSweep {
    pub removed: usize,
    /// In-flight logs of another goose that still runs, left to it.
    pub live: usize,
    pub failed: Vec<(PathBuf, String)>,
}

/// Remove the in-flight request logs no running request will finish: only this writer's own
/// names, only files last written before `started` (this process's request log began then, so
/// nothing of its own is touched), and a named writer's only when `writer_live` says that process
/// is gone. The numbered rotation and every other file are never touched.
fn sweep_leftovers(
    logs_dir: &Path,
    started: SystemTime,
    writer_live: impl Fn(u32, SystemTime) -> bool,
) -> LeftoverSweep {
    let mut sweep = LeftoverSweep::default();
    let entries = match fs_err::read_dir(logs_dir) {
        Ok(entries) => entries,
        Err(error) => {
            sweep
                .failed
                .push((logs_dir.to_path_buf(), error.to_string()));
            return sweep;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                sweep
                    .failed
                    .push((logs_dir.to_path_buf(), error.to_string()));
                continue;
            }
        };
        let path = entry.path();
        let Some(log) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(in_flight_log)
        else {
            continue;
        };
        let written = match entry.metadata().and_then(|m| m.modified()) {
            Ok(written) => written,
            // Rotated or swept by its writer or another goose between the listing and this read.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                sweep.failed.push((path, error.to_string()));
                continue;
            }
        };
        if written >= started {
            continue;
        }
        if let InFlightLog::Writer(pid) = log {
            if writer_live(pid, written) {
                sweep.live += 1;
                continue;
            }
        }
        match fs_err::remove_file(&path) {
            Ok(()) => sweep.removed += 1,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => sweep.failed.push((path, error.to_string())),
        }
    }
    sweep
}

/// Whether the process that last wrote a log at `last_write` still runs: a live, non-zombie `pid`
/// that started no later than that write. A pid reused by a process started after it is not the
/// writer.
fn writer_still_running(pid: u32, last_write: SystemTime) -> bool {
    let Some((started, zombie)) = goose_sidecar::machine::process_start(pid) else {
        return false;
    };
    // measured: both clocks are read in whole seconds here, so a writer that started and wrote
    // within one tick can read one second "after" its own write.
    !zombie
        && last_write
            .duration_since(UNIX_EPOCH)
            .is_ok_and(|written| started <= written.as_secs() + 1)
}

/// Remove the request logs earlier goose processes left in flight (see `RequestLog`), naming what
/// was removed and what could not be. Call once tracing is up, after `init_goose_request_log`.
pub fn sweep_request_log_leftovers() {
    let Some(started) = LOGGER_START.get() else {
        tracing::warn!(
            "request log leftovers not swept: this process never started its request log"
        );
        return;
    };
    let sweep = sweep_leftovers(&Paths::in_state_dir("logs"), *started, writer_still_running);
    if sweep.removed > 0 {
        tracing::info!(
            removed = sweep.removed,
            live = sweep.live,
            "removed request logs earlier goose processes left in flight"
        );
    }
    for (path, error) in &sweep.failed {
        tracing::warn!(path = %path.display(), error = %error, "request log leftover not swept");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unescape_json_values_with_object() {
        let value = json!({"text": "Hello\\nWorld"});
        let unescaped_value = unescape_json_values(&value);
        assert_eq!(unescaped_value, json!({"text": "Hello\nWorld"}));
    }

    #[test]
    fn unescape_json_values_with_array() {
        let value = json!(["Hello\\nWorld", "Goodbye\\tWorld"]);
        let unescaped_value = unescape_json_values(&value);
        assert_eq!(unescaped_value, json!(["Hello\nWorld", "Goodbye\tWorld"]));
    }

    #[test]
    fn unescape_json_values_with_string() {
        let value = json!("Hello\\nWorld");
        let unescaped_value = unescape_json_values(&value);
        assert_eq!(unescaped_value, json!("Hello\nWorld"));
    }

    #[test]
    fn unescape_json_values_with_mixed_content() {
        let value = json!({
            "text": "Hello\\nWorld\\\\n!",
            "array": ["Goodbye\\tWorld", "See you\\rlater"],
            "nested": {
                "inner_text": "Inner\\\"Quote\\\""
            }
        });
        let unescaped_value = unescape_json_values(&value);
        assert_eq!(
            unescaped_value,
            json!({
                "text": "Hello\nWorld\n!",
                "array": ["Goodbye\tWorld", "See you\rlater"],
                "nested": {
                    "inner_text": "Inner\"Quote\""
                }
            })
        );
    }

    #[test]
    fn unescape_json_values_with_no_escapes() {
        let value = json!({"text": "Hello World"});
        let unescaped_value = unescape_json_values(&value);
        assert_eq!(unescaped_value, json!({"text": "Hello World"}));
    }

    #[test]
    fn test_is_google_model() {
        // Define the test cases as a vector of tuples
        let test_cases = vec![
            // (input, expected_result)
            (json!({ "model": "google_gemini" }), true),
            (json!({ "model": "microsoft_bing" }), false),
            (json!({ "model": "" }), false),
            (json!({}), false),
            (json!({ "model": "Google_XYZ" }), true),
            (json!({ "model": "google_abc" }), true),
        ];

        // Iterate through each test case and assert the result
        for (payload, expected_result) in test_cases {
            assert_eq!(is_google_model(&payload), expected_result);
        }
    }

    mod request_log {
        use super::super::*;
        use futures::StreamExt;
        use std::time::Duration;

        fn names(dir: &Path) -> Vec<String> {
            let mut names: Vec<String> = fs_err::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }

        fn in_flight(dir: &Path) -> Vec<String> {
            names(dir)
                .into_iter()
                .filter(|n| in_flight_log(n).is_some())
                .collect()
        }

        fn touch(dir: &Path, name: &str, written: SystemTime) {
            let file = File::create(dir.join(name)).unwrap();
            file.file().set_modified(written).unwrap();
        }

        #[test]
        fn an_in_flight_log_names_its_writer() {
            let tmp = tempfile::tempdir().unwrap();
            let log = RequestLog::in_dir(tmp.path().to_path_buf(), LOGS_TO_KEEP).unwrap();
            let _handle = log.start().unwrap();
            let names = in_flight(tmp.path());
            assert_eq!(names.len(), 1, "{names:?}");
            assert_eq!(
                in_flight_log(&names[0]),
                Some(InFlightLog::Writer(std::process::id()))
            );
        }

        /// Q-343: a request whose stream is dropped mid-answer (the person's Stop, a turn cut) is
        /// finalized into the rotation with every line it wrote — never left under its in-flight name.
        #[tokio::test]
        async fn a_cancelled_stream_is_rotated_with_what_it_wrote() {
            let tmp = tempfile::tempdir().unwrap();
            let log = RequestLog::in_dir(tmp.path().to_path_buf(), LOGS_TO_KEEP).unwrap();
            let mut handle = log.start().unwrap();
            handle.write(r#"{"input":"the request"}"#).unwrap();
            let mut stream = Box::pin(async_stream::stream! {
                handle.write(r#"{"data":"first chunk"}"#).unwrap();
                yield 1;
                futures::future::pending::<()>().await;
                handle.write(r#"{"data":"never"}"#).unwrap();
                yield 2;
            });
            assert_eq!(stream.next().await, Some(1));
            assert_eq!(in_flight(tmp.path()).len(), 1);
            let second = tokio::time::timeout(Duration::from_millis(50), stream.next()).await;
            assert!(second.is_err(), "the stream is still waiting for its model");
            drop(stream);

            assert_eq!(in_flight(tmp.path()), Vec::<String>::new());
            assert_eq!(names(tmp.path()), vec!["llm_request.0.jsonl".to_string()]);
            let text = fs_err::read_to_string(tmp.path().join("llm_request.0.jsonl")).unwrap();
            assert_eq!(
                text,
                "{\"input\":\"the request\"}\n{\"data\":\"first chunk\"}\n"
            );
        }

        #[test]
        fn a_panic_unwinding_through_a_request_still_rotates_its_log() {
            let tmp = tempfile::tempdir().unwrap();
            let log = RequestLog::in_dir(tmp.path().to_path_buf(), LOGS_TO_KEEP).unwrap();
            let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut handle = log.start().unwrap();
                handle.write("{\"input\":1}").unwrap();
                panic!("the provider panicked mid-stream");
            }));
            assert!(unwound.is_err());
            assert_eq!(in_flight(tmp.path()), Vec::<String>::new());
            assert_eq!(
                fs_err::read_to_string(tmp.path().join("llm_request.0.jsonl")).unwrap(),
                "{\"input\":1}\n"
            );
        }

        #[test]
        fn a_log_that_cannot_be_rotated_is_removed_not_left_in_flight() {
            let tmp = tempfile::tempdir().unwrap();
            // The rotation's slot 0 is a directory: the rename into it fails.
            fs_err::create_dir(tmp.path().join("llm_request.0.jsonl")).unwrap();
            let log = RequestLog::in_dir(tmp.path().to_path_buf(), 1).unwrap();
            let mut handle = log.start().unwrap();
            handle.write("{\"input\":1}").unwrap();
            drop(handle);
            assert_eq!(in_flight(tmp.path()), Vec::<String>::new());
            assert!(tmp.path().join("llm_request.0.jsonl").is_dir());
        }

        #[test]
        fn only_the_writers_own_in_flight_names_are_read_as_in_flight() {
            let id = "ec484376-bf37-4690-80bf-93c835878440";
            assert_eq!(
                in_flight_log(&format!("llm_request.{id}.jsonl")),
                Some(InFlightLog::Unnamed)
            );
            assert_eq!(
                in_flight_log(&format!("llm_request.4242.{id}.jsonl")),
                Some(InFlightLog::Writer(4242))
            );
            for other in [
                "llm_request.0.jsonl".to_string(),
                "llm_request.9.jsonl".to_string(),
                "llm_request.notes.jsonl".to_string(),
                format!("llm_request.{id}.jsonl.bak"),
                format!("llm_request.x.{id}.jsonl"),
                format!("other.{id}.jsonl"),
                "llm_request.4242.not-a-uuid.jsonl".to_string(),
            ] {
                assert_eq!(in_flight_log(&other), None, "{other}");
            }
        }

        /// The 03:57 sample: fact-check calls a killed goosed left under their in-flight names, next
        /// to the rotation and a newer process's request still running.
        #[test]
        fn the_sweep_removes_only_leftovers_no_running_request_will_finish() {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path();
            let started = SystemTime::now();
            let before = started - Duration::from_secs(3600);
            let after = started + Duration::from_secs(1);
            let id = |n: u8| format!("{n:08x}-bf37-4690-80bf-93c835878440");
            touch(dir, "llm_request.0.jsonl", before);
            touch(dir, "llm_request.9.jsonl", before);
            touch(dir, "llm_request.notes.jsonl", before);
            touch(dir, &format!("llm_request.{}.jsonl", id(1)), before);
            touch(dir, &format!("llm_request.{}.jsonl", id(2)), before);
            touch(dir, &format!("llm_request.{}.jsonl", id(3)), after);
            touch(dir, &format!("llm_request.500.{}.jsonl", id(4)), before);
            touch(dir, &format!("llm_request.600.{}.jsonl", id(5)), before);
            touch(dir, &format!("llm_request.500.{}.jsonl", id(6)), after);

            let sweep = sweep_leftovers(dir, started, |pid, _| pid == 600);

            assert_eq!(
                sweep,
                LeftoverSweep {
                    removed: 3,
                    live: 1,
                    failed: vec![]
                }
            );
            let mut kept = vec![
                "llm_request.0.jsonl".to_string(),
                "llm_request.9.jsonl".to_string(),
                "llm_request.notes.jsonl".to_string(),
                format!("llm_request.{}.jsonl", id(3)),
                format!("llm_request.600.{}.jsonl", id(5)),
                format!("llm_request.500.{}.jsonl", id(6)),
            ];
            kept.sort();
            assert_eq!(names(dir), kept);
        }

        #[test]
        fn a_writer_runs_only_while_its_pid_is_the_process_that_wrote() {
            let now = SystemTime::now();
            assert!(writer_still_running(std::process::id(), now));
            // The same pid long before this process started is an earlier process's.
            assert!(!writer_still_running(
                std::process::id(),
                UNIX_EPOCH + Duration::from_secs(1_000)
            ));
            assert!(!writer_still_running(u32::MAX - 1, now));
        }

        /// `goose serve`'s stop ends in `std::process::exit`, which drops no stream: 03:57:08's
        /// teardown left the fact checks written 5 s before it under their in-flight names.
        #[test]
        fn an_exit_ends_every_request_in_flight_with_its_cause() {
            let tmp = tempfile::tempdir().unwrap();
            let log = RequestLog::in_dir(tmp.path().to_path_buf(), LOGS_TO_KEEP).unwrap();
            let mut first = log.start().unwrap();
            first.write("{\"input\":\"fact check 1\"}").unwrap();
            let mut second = log.start().unwrap();
            second.write("{\"input\":\"fact check 2\"}").unwrap();
            let finished = log.start().unwrap();
            drop(finished);
            assert_eq!(lock(&log.open.0).len(), 2);

            let cause = "goose exited (SIGTERM) with this request in flight";
            assert_eq!(log.open.end_all(cause), 2);

            assert_eq!(in_flight(tmp.path()), Vec::<String>::new());
            assert!(lock(&log.open.0).is_empty());
            let ended: Vec<String> = ["llm_request.0.jsonl", "llm_request.1.jsonl"]
                .iter()
                .map(|n| fs_err::read_to_string(tmp.path().join(n)).unwrap())
                .collect();
            for text in &ended {
                assert_eq!(
                    text.lines().last().unwrap(),
                    format!("{{\"error\":\"{cause}\"}}")
                );
            }
            // What the cut streams still send before the exit goes nowhere, and is no error.
            first.write("{\"data\":\"late\"}").unwrap();
            drop(first);
            drop(second);
            assert_eq!(
                names(tmp.path()),
                vec![
                    "llm_request.0.jsonl".to_string(),
                    "llm_request.1.jsonl".to_string(),
                    "llm_request.2.jsonl".to_string(),
                ]
            );
            assert!(
                !fs_err::read_to_string(tmp.path().join("llm_request.0.jsonl"))
                    .unwrap()
                    .contains("late")
            );
        }
    }
}
