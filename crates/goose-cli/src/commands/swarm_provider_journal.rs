use goose_swarm::{ProviderLifecycleJournal, ProviderRequestReceipt, ProviderTerminalReceipt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

const JOURNAL_VERSION: u32 = 1;
const GENESIS_HASH: &str = "genesis";

#[derive(Clone, Debug, Deserialize, Serialize)]
struct JournalMaterial {
    version: u32,
    seq: u64,
    prev_hash: String,
    run_id: String,
    fleet_snapshot_id: String,
    transition: String,
    admission_id: String,
    ordinal: u32,
    provider_request_id: String,
    physical_host_id: String,
    model_instance_id: String,
    terminal_kind: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct JournalRecord {
    #[serde(flatten)]
    material: JournalMaterial,
    entry_hash: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RequestIdentity {
    admission_id: String,
    ordinal: u32,
    provider_request_id: String,
}

#[derive(Clone, Debug)]
struct OpenRequest {
    physical_host_id: String,
    model_instance_id: String,
}

struct JournalState {
    file: std::fs::File,
    next_seq: u64,
    previous_hash: String,
    expected_len: u64,
    open: HashMap<RequestIdentity, OpenRequest>,
}

pub(super) struct DurableProviderLifecycleJournal {
    path: PathBuf,
    run_id: String,
    fleet_snapshot_id: String,
    state: Mutex<JournalState>,
}

impl DurableProviderLifecycleJournal {
    pub(super) fn open(
        working_dir: &Path,
        run_id: impl Into<String>,
        fleet_snapshot_id: impl Into<String>,
    ) -> Result<Self, String> {
        let working_dir = std::fs::canonicalize(working_dir).map_err(|error| {
            format!("cannot canonicalize provider journal working directory: {error}")
        })?;
        let directory = working_dir.join(".swarm");
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("cannot create provider journal directory: {error}"))?;
        let path = directory.join("provider-lifecycle-v1.jsonl");
        let existing = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(format!("cannot read provider lifecycle journal: {error}")),
        };
        let (next_seq, previous_hash, open) = validate_journal(&existing)?;
        if !open.is_empty() {
            let mut unresolved: Vec<_> = open
                .keys()
                .map(|key| {
                    format!(
                        "{}:{}:{}",
                        key.admission_id, key.ordinal, key.provider_request_id
                    )
                })
                .collect();
            unresolved.sort();
            return Err(format!(
                "provider lifecycle journal has unresolved external request(s): {}. The affected LM Studio model instance must be reset and the reset durably reconciled before another physical run",
                unresolved.join(", ")
            ));
        }
        let created = !path.exists();
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)
            .map_err(|error| format!("cannot open provider lifecycle journal: {error}"))?;
        if created {
            file.sync_all()
                .map_err(|error| format!("cannot sync new provider lifecycle journal: {error}"))?;
            let parent = std::fs::File::open(&directory).map_err(|error| {
                format!("cannot open provider lifecycle journal directory for sync: {error}")
            })?;
            parent.sync_all().map_err(|error| {
                format!("cannot sync provider lifecycle journal directory: {error}")
            })?;
        }
        Ok(Self {
            path,
            run_id: run_id.into(),
            fleet_snapshot_id: fleet_snapshot_id.into(),
            state: Mutex::new(JournalState {
                file,
                next_seq,
                previous_hash,
                expected_len: existing.len() as u64,
                open,
            }),
        })
    }

    fn append(
        &self,
        transition: &str,
        receipt: &ProviderRequestReceipt,
        terminal_kind: Option<String>,
    ) -> Result<(), String> {
        let identity = request_identity(receipt);
        let mut state = lock(&self.state);
        match transition {
            "started" if state.open.contains_key(&identity) => {
                return Err(format!("provider request {identity:?} was journaled twice"));
            }
            "terminal" if !state.open.contains_key(&identity) => {
                return Err(format!(
                    "provider terminal {identity:?} has no durable start"
                ));
            }
            _ => {}
        }
        verify_live_file(&self.path, &state.file, state.expected_len)?;
        let material = JournalMaterial {
            version: JOURNAL_VERSION,
            seq: state.next_seq,
            prev_hash: state.previous_hash.clone(),
            run_id: self.run_id.clone(),
            fleet_snapshot_id: self.fleet_snapshot_id.clone(),
            transition: transition.to_string(),
            admission_id: receipt.admission_id.clone(),
            ordinal: receipt.key.ordinal,
            provider_request_id: receipt.key.provider_request_id.clone(),
            physical_host_id: receipt.physical_host_id.clone(),
            model_instance_id: receipt.model_instance_id.clone(),
            terminal_kind,
        };
        let entry_hash = hash_material(&material)?;
        let record = JournalRecord {
            material,
            entry_hash: entry_hash.clone(),
        };
        let mut line = serde_json::to_vec(&record)
            .map_err(|error| format!("cannot encode provider journal record: {error}"))?;
        line.push(b'\n');
        state
            .file
            .write_all(&line)
            .map_err(|error| format!("cannot append provider lifecycle journal: {error}"))?;
        state
            .file
            .sync_data()
            .map_err(|error| format!("cannot sync provider lifecycle journal: {error}"))?;
        state.expected_len = state
            .expected_len
            .checked_add(line.len() as u64)
            .ok_or_else(|| "provider lifecycle journal length overflowed".to_string())?;
        state.next_seq = state
            .next_seq
            .checked_add(1)
            .ok_or_else(|| "provider lifecycle journal sequence overflowed".to_string())?;
        state.previous_hash = entry_hash;
        if transition == "started" {
            state.open.insert(
                identity,
                OpenRequest {
                    physical_host_id: receipt.physical_host_id.clone(),
                    model_instance_id: receipt.model_instance_id.clone(),
                },
            );
        } else {
            state.open.remove(&identity);
        }
        Ok(())
    }
}

impl ProviderLifecycleJournal for DurableProviderLifecycleJournal {
    fn provider_request_started(&self, receipt: &ProviderRequestReceipt) -> Result<(), String> {
        self.append("started", receipt, None)
    }

    fn provider_terminal(&self, receipt: &ProviderTerminalReceipt) -> Result<(), String> {
        let start = ProviderRequestReceipt {
            admission_id: receipt.admission_id.clone(),
            key: receipt.key.clone(),
            physical_host_id: receipt.physical_host_id.clone(),
            model_instance_id: receipt.model_instance_id.clone(),
        };
        let identity = request_identity(&start);
        if let Some(open) = lock(&self.state).open.get(&identity).cloned() {
            if open.physical_host_id != receipt.physical_host_id
                || open.model_instance_id != receipt.model_instance_id
            {
                return Err(format!(
                    "provider terminal {identity:?} changed its physical identity"
                ));
            }
        }
        self.append(
            "terminal",
            &start,
            Some(
                serde_json::to_value(receipt.kind)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_string))
                    .unwrap_or_else(|| format!("{:?}", receipt.kind).to_lowercase()),
            ),
        )
    }
}

fn validate_journal(
    bytes: &[u8],
) -> Result<(u64, String, HashMap<RequestIdentity, OpenRequest>), String> {
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        return Err("provider lifecycle journal has a torn final record".to_string());
    }
    let mut expected_seq = 0_u64;
    let mut previous_hash = GENESIS_HASH.to_string();
    let mut open = HashMap::new();
    for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let record: JournalRecord = serde_json::from_slice(line).map_err(|error| {
            format!(
                "provider lifecycle journal record {} is invalid: {error}",
                index + 1
            )
        })?;
        if record.material.version != JOURNAL_VERSION
            || record.material.seq != expected_seq
            || record.material.prev_hash != previous_hash
            || hash_material(&record.material)? != record.entry_hash
        {
            return Err(format!(
                "provider lifecycle journal record {} breaks its hash-linked sequence",
                index + 1
            ));
        }
        let identity = RequestIdentity {
            admission_id: record.material.admission_id.clone(),
            ordinal: record.material.ordinal,
            provider_request_id: record.material.provider_request_id.clone(),
        };
        match record.material.transition.as_str() {
            "started" => {
                if record.material.terminal_kind.is_some() || open.contains_key(&identity) {
                    return Err(format!(
                        "provider lifecycle journal has an invalid duplicate start at record {}",
                        index + 1
                    ));
                }
                open.insert(
                    identity,
                    OpenRequest {
                        physical_host_id: record.material.physical_host_id,
                        model_instance_id: record.material.model_instance_id,
                    },
                );
            }
            "terminal" => {
                let started = open.remove(&identity).ok_or_else(|| {
                    format!(
                        "provider lifecycle journal terminal at record {} has no start",
                        index + 1
                    )
                })?;
                if record.material.terminal_kind.is_none()
                    || started.physical_host_id != record.material.physical_host_id
                    || started.model_instance_id != record.material.model_instance_id
                {
                    return Err(format!(
                        "provider lifecycle journal terminal at record {} changed its start",
                        index + 1
                    ));
                }
            }
            transition => {
                return Err(format!(
                    "provider lifecycle journal record {} has unknown transition {transition:?}",
                    index + 1
                ));
            }
        }
        previous_hash = record.entry_hash;
        expected_seq = expected_seq
            .checked_add(1)
            .ok_or_else(|| "provider lifecycle journal sequence overflowed".to_string())?;
    }
    Ok((expected_seq, previous_hash, open))
}

fn request_identity(receipt: &ProviderRequestReceipt) -> RequestIdentity {
    RequestIdentity {
        admission_id: receipt.admission_id.clone(),
        ordinal: receipt.key.ordinal,
        provider_request_id: receipt.key.provider_request_id.clone(),
    }
}

fn hash_material(material: &JournalMaterial) -> Result<String, String> {
    let bytes = serde_json::to_vec(material)
        .map_err(|error| format!("cannot encode provider journal hash material: {error}"))?;
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}")
            .map_err(|error| format!("cannot encode provider journal hash: {error}"))?;
    }
    Ok(encoded)
}

fn verify_live_file(path: &Path, file: &std::fs::File, expected_len: u64) -> Result<(), String> {
    let open = file
        .metadata()
        .map_err(|error| format!("cannot inspect open provider journal: {error}"))?;
    let linked = std::fs::metadata(path)
        .map_err(|error| format!("cannot inspect provider journal path: {error}"))?;
    if open.len() != expected_len || linked.len() != expected_len {
        return Err("provider lifecycle journal changed outside its writer".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if open.dev() != linked.dev() || open.ino() != linked.ino() {
            return Err("provider lifecycle journal path was replaced".to_string());
        }
    }
    Ok(())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_swarm::{ProviderRequestKey, ProviderTerminalKind};

    fn start() -> ProviderRequestReceipt {
        ProviderRequestReceipt {
            admission_id: "admission-a".to_string(),
            key: ProviderRequestKey {
                ordinal: 0,
                provider_request_id: "request-a".to_string(),
            },
            physical_host_id: "host-a".to_string(),
            model_instance_id: "instance-a".to_string(),
        }
    }

    #[test]
    fn completed_journal_reopens_but_unresolved_request_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let journal =
            DurableProviderLifecycleJournal::open(root.path(), "run-a", "fleet-a").unwrap();
        let started = start();
        journal.provider_request_started(&started).unwrap();
        assert!(DurableProviderLifecycleJournal::open(root.path(), "run-b", "fleet-b").is_err());
        journal
            .provider_terminal(&ProviderTerminalReceipt {
                admission_id: started.admission_id.clone(),
                key: started.key.clone(),
                physical_host_id: started.physical_host_id.clone(),
                model_instance_id: started.model_instance_id.clone(),
                kind: ProviderTerminalKind::Finished,
            })
            .unwrap();
        drop(journal);
        DurableProviderLifecycleJournal::open(root.path(), "run-b", "fleet-b").unwrap();
    }

    #[test]
    fn terminal_with_spliced_physical_identity_is_rejected_and_stays_unresolved() {
        let root = tempfile::tempdir().unwrap();
        let journal =
            DurableProviderLifecycleJournal::open(root.path(), "run-a", "fleet-a").unwrap();
        let started = start();
        journal.provider_request_started(&started).unwrap();

        let error = journal
            .provider_terminal(&ProviderTerminalReceipt {
                admission_id: started.admission_id.clone(),
                key: started.key.clone(),
                physical_host_id: "host-b".to_string(),
                model_instance_id: started.model_instance_id.clone(),
                kind: ProviderTerminalKind::Cancelled,
            })
            .unwrap_err();

        assert!(error.contains("changed its physical identity"));
        drop(journal);
        assert!(DurableProviderLifecycleJournal::open(root.path(), "run-b", "fleet-b").is_err());
    }

    #[test]
    fn torn_or_replaced_journal_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let journal =
            DurableProviderLifecycleJournal::open(root.path(), "run-a", "fleet-a").unwrap();
        journal.provider_request_started(&start()).unwrap();
        let path = root.path().join(".swarm/provider-lifecycle-v1.jsonl");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"torn")
            .unwrap();
        assert!(journal
            .provider_terminal(&ProviderTerminalReceipt {
                admission_id: "admission-a".to_string(),
                key: ProviderRequestKey {
                    ordinal: 0,
                    provider_request_id: "request-a".to_string(),
                },
                physical_host_id: "host-a".to_string(),
                model_instance_id: "instance-a".to_string(),
                kind: ProviderTerminalKind::Failed,
            })
            .is_err());
        drop(journal);
        assert!(DurableProviderLifecycleJournal::open(root.path(), "run-b", "fleet-b").is_err());
    }
}
