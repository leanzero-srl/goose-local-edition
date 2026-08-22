use crate::benchmark_budget::BenchmarkBudgetReservation;
use crate::providers::base::MessageStream;
use fs2::FileExt;
use futures::StreamExt;
use goose_providers::errors::ProviderError;
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use uuid::Uuid;

pub(crate) const LIFECYCLE_FILE_ENV: &str = "GOOSE_PROVIDER_LIFECYCLE_FILE";
pub(crate) const LIFECYCLE_STRICT_ENV: &str = "GOOSE_PROVIDER_LIFECYCLE_STRICT";
static PROCESS_APPEND_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug)]
pub(crate) struct LifecycleSettings {
    path: Option<PathBuf>,
    strict_terminal: bool,
}

impl LifecycleSettings {
    pub(crate) fn from_env() -> Self {
        Self {
            path: std::env::var_os(LIFECYCLE_FILE_ENV)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            strict_terminal: env_flag(LIFECYCLE_STRICT_ENV),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(path: PathBuf, strict_terminal: bool) -> Self {
        Self {
            path: Some(path),
            strict_terminal,
        }
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LifecycleState {
    Queued,
    Admitted,
    FirstItem,
    UsageReported,
    ProviderTerminal,
    StreamAmbiguous,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreAdmissionDisposition {
    ReleaseReservation,
    PreserveReservation,
}

pub(crate) fn pre_admission_disposition(error: &ProviderError) -> PreAdmissionDisposition {
    match error {
        ProviderError::NetworkError(_)
        | ProviderError::RequestFailed(_)
        | ProviderError::ServerError(_) => PreAdmissionDisposition::PreserveReservation,
        _ => PreAdmissionDisposition::ReleaseReservation,
    }
}

#[derive(Debug, Serialize)]
struct LifecycleUsage {
    reported_model: String,
    input_tokens: Option<i32>,
    output_tokens: Option<i32>,
    total_tokens: Option<i32>,
}

impl From<&goose_providers::conversation::token_usage::ProviderUsage> for LifecycleUsage {
    fn from(usage: &goose_providers::conversation::token_usage::ProviderUsage) -> Self {
        Self {
            reported_model: usage.model.clone(),
            input_tokens: usage.usage.input_tokens,
            output_tokens: usage.usage.output_tokens,
            total_tokens: usage.usage.total_tokens,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct LifecycleEvent {
    schema_version: u8,
    timestamp: String,
    request_id: String,
    provider: String,
    model: String,
    session: String,
    state: LifecycleState,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<LifecycleUsage>,
}

pub(crate) struct ProviderRequestLifecycle {
    settings: LifecycleSettings,
    request_id: String,
    provider: String,
    model: String,
    session: String,
    reservation: Option<BenchmarkBudgetReservation>,
    admitted: AtomicBool,
    finalized: AtomicBool,
}

impl ProviderRequestLifecycle {
    pub(crate) fn begin(
        settings: LifecycleSettings,
        provider: String,
        model: String,
        session: String,
        reservation: Option<BenchmarkBudgetReservation>,
    ) -> Result<Self, ProviderError> {
        let request_id = reservation
            .as_ref()
            .map(|reservation| reservation.request_id().to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut lifecycle = Self {
            settings,
            request_id,
            provider,
            model,
            session,
            reservation,
            admitted: AtomicBool::new(false),
            finalized: AtomicBool::new(false),
        };
        if let Err(record_error) = lifecycle.record(LifecycleState::Queued, None, None) {
            lifecycle.finalized.store(true, Ordering::Release);
            if let Some(reservation) = lifecycle.reservation.take() {
                reservation.release_unadmitted()?;
            }
            return Err(record_error);
        }
        Ok(lifecycle)
    }

    pub(crate) fn admitted(&self) -> Result<(), ProviderError> {
        self.admitted.store(true, Ordering::Release);
        self.record(LifecycleState::Admitted, None, None)
    }

    pub(crate) fn pre_admission_error(
        &mut self,
        error: &ProviderError,
    ) -> Result<PreAdmissionDisposition, ProviderError> {
        let disposition = pre_admission_disposition(error);
        let state = match disposition {
            PreAdmissionDisposition::ReleaseReservation => LifecycleState::Error,
            PreAdmissionDisposition::PreserveReservation => LifecycleState::StreamAmbiguous,
        };
        if disposition == PreAdmissionDisposition::ReleaseReservation {
            if let Some(reservation) = self.reservation.take() {
                if let Err(release_error) = reservation.release_unadmitted() {
                    let _ = self.finalize(
                        LifecycleState::StreamAmbiguous,
                        Some("budget_release_failed"),
                        None,
                    );
                    return Err(release_error);
                }
            }
        }
        self.finalize(state, Some(error.telemetry_type()), None)?;
        Ok(disposition)
    }

    pub(crate) fn provider_terminal(
        &mut self,
        usage: &goose_providers::conversation::token_usage::ProviderUsage,
    ) -> Result<(), ProviderError> {
        if let Some(reservation) = self.reservation.take() {
            reservation.settle(usage)?;
        }
        self.finalize(LifecycleState::ProviderTerminal, None, Some(usage))
    }

    pub(crate) fn wrap(mut self, mut stream: MessageStream) -> MessageStream {
        Box::pin(async_stream::stream! {
            let mut first_item_seen = false;
            let mut final_usage = None;

            while let Some(item) = stream.next().await {
                match item {
                    Ok((message, usage)) => {
                        if !first_item_seen {
                            first_item_seen = true;
                            if let Err(error) = self.record(LifecycleState::FirstItem, None, None) {
                                yield Err(error);
                                return;
                            }
                        }
                        if let Some(reported_usage) = usage.as_ref() {
                            if final_usage.is_none() {
                                if let Err(error) = self.record(
                                    LifecycleState::UsageReported,
                                    None,
                                    Some(reported_usage),
                                ) {
                                    yield Err(error);
                                    return;
                                }
                            }
                            final_usage = Some(reported_usage.clone());
                        }
                        yield Ok((message, usage));
                    }
                    Err(error) => {
                        let _ = self.finalize(
                            LifecycleState::StreamAmbiguous,
                            Some(error.telemetry_type()),
                            None,
                        );
                        yield Err(error);
                        return;
                    }
                }
            }

            if let Some(usage) = final_usage.as_ref() {
                if let Err(error) = self.provider_terminal(usage) {
                    let _ = self.finalize(
                        LifecycleState::StreamAmbiguous,
                        Some("budget_or_terminal_evidence"),
                        None,
                    );
                    yield Err(error);
                }
            } else {
                let _ = self.finalize(
                    LifecycleState::StreamAmbiguous,
                    Some("missing_usage"),
                    None,
                );
                if self.settings.strict_terminal {
                    yield Err(ProviderError::UsageError(
                        "Provider stream ended without usage; terminal state is unproven"
                            .to_string(),
                    ));
                }
            }
        })
    }

    fn finalize(
        &self,
        state: LifecycleState,
        reason: Option<&'static str>,
        usage: Option<&goose_providers::conversation::token_usage::ProviderUsage>,
    ) -> Result<(), ProviderError> {
        if !self.finalized.swap(true, Ordering::AcqRel) {
            self.record(state, reason, usage)?;
        }
        Ok(())
    }

    fn record(
        &self,
        state: LifecycleState,
        reason: Option<&'static str>,
        usage: Option<&goose_providers::conversation::token_usage::ProviderUsage>,
    ) -> Result<(), ProviderError> {
        let Some(path) = self.settings.path.as_ref() else {
            if self.settings.strict_terminal {
                return Err(ProviderError::ExecutionError(format!(
                    "{LIFECYCLE_STRICT_ENV} requires {LIFECYCLE_FILE_ENV}"
                )));
            }
            return Ok(());
        };
        let event = LifecycleEvent {
            schema_version: 1,
            timestamp: chrono::Utc::now().to_rfc3339(),
            request_id: self.request_id.clone(),
            provider: self.provider.clone(),
            model: self.model.clone(),
            session: self.session.clone(),
            state,
            reason,
            usage: usage.map(LifecycleUsage::from),
        };
        if let Err(error) = append_jsonl(path, &event) {
            if self.settings.strict_terminal {
                return Err(ProviderError::ExecutionError(format!(
                    "cannot append strict provider lifecycle evidence: {error}"
                )));
            }
            tracing::warn!(error = %error, "failed to append provider lifecycle event");
        }
        Ok(())
    }
}

impl Drop for ProviderRequestLifecycle {
    fn drop(&mut self) {
        if self.finalized.swap(true, Ordering::AcqRel) {
            return;
        }
        if self.admitted.load(Ordering::Acquire) {
            let _ = self.record(LifecycleState::StreamAmbiguous, Some("consumer_drop"), None);
        } else {
            let _ = self.record(
                LifecycleState::Error,
                Some("cancelled_before_admission"),
                None,
            );
        }
    }
}

fn append_jsonl(path: &PathBuf, event: &LifecycleEvent) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut payload = serde_json::to_vec(event).map_err(io::Error::other)?;
    payload.push(b'\n');

    let _process_guard = PROCESS_APPEND_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.lock_exclusive()?;
    file.write_all(&payload)?;
    file.flush()?;
    file.sync_data()
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_providers::conversation::token_usage::{ProviderUsage, Usage};
    use goose_providers::model::ModelConfig;

    fn budget_fixture(root: &std::path::Path) -> (PathBuf, PathBuf, ModelConfig) {
        let config_path = root.join("budget-config.json");
        let ledger_path = root.join("budget-ledger.json");
        std::fs::write(
            &config_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "currency": "USD",
                "total_cap": 1.0,
                "provider_caps": {"provider": 1.0},
                "models": {
                    "provider/model": {
                        "provider": "provider",
                        "model": "model",
                        "context_limit": 1000,
                        "max_output_tokens": 1000,
                        "pricing": {
                            "input_per_million": 100.0,
                            "output_per_million": 100.0,
                            "source": "https://example.test/pricing",
                            "verified_at": "2026-08-23"
                        }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            &ledger_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "currency": "USD",
                "total_cap": 1.0,
                "provider_caps": {"provider": 1.0},
                "spent_upper_bound": 0.0,
                "provider_spent_upper_bound": {"provider": 0.0},
                "outstanding": {},
                "settled": [],
                "updated_at": "now"
            }))
            .unwrap(),
        )
        .unwrap();
        let model = ModelConfig::new("model")
            .with_context_limit(Some(1000))
            .with_max_tokens(Some(1000));
        (config_path, ledger_path, model)
    }

    #[test]
    fn ambiguous_pre_admission_failures_preserve_reservations() {
        assert_eq!(
            pre_admission_disposition(&ProviderError::NetworkError("reset".into())),
            PreAdmissionDisposition::PreserveReservation
        );
        assert_eq!(
            pre_admission_disposition(&ProviderError::RequestFailed("unknown".into())),
            PreAdmissionDisposition::PreserveReservation
        );
        assert_eq!(
            pre_admission_disposition(&ProviderError::RateLimitExceeded {
                details: "rejected".into(),
                retry_delay: None,
            }),
            PreAdmissionDisposition::ReleaseReservation
        );
        assert_eq!(
            pre_admission_disposition(&ProviderError::ServerError("rejected".into())),
            PreAdmissionDisposition::PreserveReservation
        );
    }

    #[test]
    fn dropping_an_admitted_request_is_ambiguous() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("lifecycle.jsonl");
        let lifecycle = ProviderRequestLifecycle::begin(
            LifecycleSettings::for_test(path.clone(), true),
            "provider".into(),
            "model".into(),
            "session".into(),
            None,
        )
        .unwrap();
        lifecycle.admitted().unwrap();
        drop(lifecycle);

        let events: Vec<serde_json::Value> = std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let states: Vec<&str> = events
            .iter()
            .map(|event| event["state"].as_str().unwrap())
            .collect();
        assert_eq!(states, vec!["queued", "admitted", "stream_ambiguous"]);
        assert_eq!(events[2]["reason"], "consumer_drop");
    }

    #[test]
    fn terminal_usage_settles_the_correlated_budget_reservation() {
        let temp = tempfile::tempdir().unwrap();
        let lifecycle_path = temp.path().join("lifecycle.jsonl");
        let (config_path, ledger_path, model) = budget_fixture(temp.path());
        let reservation = crate::benchmark_budget::reserve_from_paths_for_test(
            &config_path,
            &ledger_path,
            "provider",
            &model,
        )
        .unwrap();
        let request_id = reservation.request_id().to_string();
        let mut lifecycle = ProviderRequestLifecycle::begin(
            LifecycleSettings::for_test(lifecycle_path.clone(), true),
            "provider".into(),
            "model".into(),
            "session".into(),
            Some(reservation),
        )
        .unwrap();
        lifecycle.admitted().unwrap();
        lifecycle
            .provider_terminal(&ProviderUsage::new(
                "model".into(),
                Usage::new(Some(100), Some(200), Some(300)),
            ))
            .unwrap();

        let ledger: serde_json::Value =
            serde_json::from_slice(&std::fs::read(ledger_path).unwrap()).unwrap();
        assert_eq!(ledger["outstanding"].as_object().unwrap().len(), 0);
        assert!((ledger["spent_upper_bound"].as_f64().unwrap() - 0.03).abs() < 1e-9);
        let events: Vec<serde_json::Value> = std::fs::read_to_string(lifecycle_path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(events.iter().all(|event| event["request_id"] == request_id));
        assert_eq!(events.last().unwrap()["state"], "provider_terminal");
    }

    #[test]
    fn ambiguous_pre_admission_error_retains_the_full_reservation() {
        let temp = tempfile::tempdir().unwrap();
        let lifecycle_path = temp.path().join("lifecycle.jsonl");
        let (config_path, ledger_path, model) = budget_fixture(temp.path());
        let reservation = crate::benchmark_budget::reserve_from_paths_for_test(
            &config_path,
            &ledger_path,
            "provider",
            &model,
        )
        .unwrap();
        let request_id = reservation.request_id().to_string();
        let mut lifecycle = ProviderRequestLifecycle::begin(
            LifecycleSettings::for_test(lifecycle_path, true),
            "provider".into(),
            "model".into(),
            "session".into(),
            Some(reservation),
        )
        .unwrap();
        assert_eq!(
            lifecycle
                .pre_admission_error(&ProviderError::NetworkError("reset".into()))
                .unwrap(),
            PreAdmissionDisposition::PreserveReservation
        );

        let ledger: serde_json::Value =
            serde_json::from_slice(&std::fs::read(ledger_path).unwrap()).unwrap();
        assert!(ledger["outstanding"].get(&request_id).is_some());
        assert_eq!(ledger["spent_upper_bound"], 0.0);
    }
}
