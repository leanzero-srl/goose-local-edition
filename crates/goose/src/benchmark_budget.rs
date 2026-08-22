use fs2::FileExt;
use goose_providers::conversation::token_usage::ProviderUsage;
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const CONFIG_ENV: &str = "GOOSE_BENCH_BUDGET_CONFIG";
const CONFIG_SHA_ENV: &str = "GOOSE_BENCH_BUDGET_CONFIG_SHA256";
const LEDGER_ENV: &str = "GOOSE_BENCH_BUDGET_LEDGER";
const TOKENS_PER_MILLION: f64 = 1_000_000.0;

#[derive(Clone, Debug, Deserialize)]
struct BudgetConfig {
    schema_version: u32,
    currency: String,
    total_cap: f64,
    provider_caps: HashMap<String, f64>,
    models: HashMap<String, ModelBudget>,
}

#[derive(Clone, Debug, Deserialize)]
struct ModelBudget {
    provider: String,
    model: String,
    context_limit: usize,
    max_output_tokens: i32,
    pricing: Pricing,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Pricing {
    input_per_million: f64,
    output_per_million: f64,
    #[serde(default)]
    tier_threshold_tokens: Option<usize>,
    #[serde(default)]
    input_over_threshold_per_million: Option<f64>,
    #[serde(default)]
    output_over_threshold_per_million: Option<f64>,
    source: String,
    verified_at: String,
    #[serde(default)]
    valid_through: Option<String>,
}

impl Pricing {
    fn rates(&self, input_tokens: usize) -> (f64, f64) {
        let over_threshold = self
            .tier_threshold_tokens
            .is_some_and(|threshold| input_tokens > threshold);
        if over_threshold {
            (
                self.input_over_threshold_per_million
                    .unwrap_or(self.input_per_million),
                self.output_over_threshold_per_million
                    .unwrap_or(self.output_per_million),
            )
        } else {
            (self.input_per_million, self.output_per_million)
        }
    }

    fn cost(&self, input_tokens: usize, output_tokens: usize) -> f64 {
        let (input_rate, output_rate) = self.rates(input_tokens);
        (input_tokens as f64 * input_rate + output_tokens as f64 * output_rate) / TOKENS_PER_MILLION
    }

    fn validate(&self) -> Result<(), ProviderError> {
        let rates = [
            self.input_per_million,
            self.output_per_million,
            self.input_over_threshold_per_million
                .unwrap_or(self.input_per_million),
            self.output_over_threshold_per_million
                .unwrap_or(self.output_per_million),
        ];
        if rates.iter().any(|rate| !rate.is_finite() || *rate < 0.0)
            || !self.source.starts_with("https://")
            || self.verified_at.is_empty()
        {
            return Err(ProviderError::ExecutionError(
                "benchmark budget pricing is invalid".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct OutstandingReservation {
    request_id: String,
    provider: String,
    model: String,
    reserved_usd: f64,
    input_reserve_tokens: usize,
    output_reserve_tokens: usize,
    created_at_unix_ms: u128,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Settlement {
    request_id: String,
    provider: String,
    model: String,
    reported_model: String,
    input_tokens: usize,
    output_tokens: usize,
    charged_upper_bound_usd: f64,
    reserved_usd: f64,
    settled_at_unix_ms: u128,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BudgetLedger {
    schema_version: u32,
    currency: String,
    total_cap: f64,
    provider_caps: HashMap<String, f64>,
    spent_upper_bound: f64,
    provider_spent_upper_bound: HashMap<String, f64>,
    outstanding: HashMap<String, OutstandingReservation>,
    settled: Vec<Settlement>,
    updated_at: String,
}

pub struct BenchmarkBudgetReservation {
    request_id: String,
    provider: String,
    model: String,
    reserved_usd: f64,
    pricing: Pricing,
    ledger_path: PathBuf,
}

impl BenchmarkBudgetReservation {
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn release_unadmitted(self) -> Result<(), ProviderError> {
        with_locked_ledger(&self.ledger_path, |ledger| {
            ledger.outstanding.remove(&self.request_id).ok_or_else(|| {
                ProviderError::ExecutionError(format!(
                    "budget reservation {} is missing",
                    self.request_id
                ))
            })?;
            Ok(())
        })
    }

    pub fn settle(self, usage: &ProviderUsage) -> Result<(), ProviderError> {
        let input_tokens = usage.usage.input_tokens.ok_or_else(|| {
            ProviderError::UsageError(format!(
                "{} returned no input usage; reservation {} remains charged",
                self.model, self.request_id
            ))
        })?;
        let output_tokens = usage.usage.output_tokens.ok_or_else(|| {
            ProviderError::UsageError(format!(
                "{} returned no output usage; reservation {} remains charged",
                self.model, self.request_id
            ))
        })?;
        if input_tokens < 0 || output_tokens < 0 {
            return Err(ProviderError::UsageError(format!(
                "{} returned negative usage; reservation {} remains charged",
                self.model, self.request_id
            )));
        }
        let input_tokens = input_tokens as usize;
        let output_tokens = output_tokens as usize;
        let actual = self.pricing.cost(input_tokens, output_tokens);
        if actual > self.reserved_usd + f64::EPSILON {
            return Err(ProviderError::UsageError(format!(
                "reported usage costs ${actual:.6}, above reservation ${:.6}; reservation {} remains charged",
                self.reserved_usd, self.request_id
            )));
        }

        with_locked_ledger(&self.ledger_path, |ledger| {
            let record = ledger.outstanding.remove(&self.request_id).ok_or_else(|| {
                ProviderError::ExecutionError(format!(
                    "budget reservation {} is missing",
                    self.request_id
                ))
            })?;
            ledger.spent_upper_bound += actual;
            *ledger
                .provider_spent_upper_bound
                .entry(self.provider.clone())
                .or_default() += actual;
            ledger.settled.push(Settlement {
                request_id: self.request_id.clone(),
                provider: self.provider.clone(),
                model: self.model.clone(),
                reported_model: usage.model.clone(),
                input_tokens,
                output_tokens,
                charged_upper_bound_usd: actual,
                reserved_usd: record.reserved_usd,
                settled_at_unix_ms: unix_ms(),
            });
            Ok(())
        })
    }
}

pub fn reserve_request(
    provider: &str,
    model_config: &ModelConfig,
) -> Result<Option<BenchmarkBudgetReservation>, ProviderError> {
    let config_path = std::env::var_os(CONFIG_ENV).map(PathBuf::from);
    let config_sha = std::env::var(CONFIG_SHA_ENV).ok();
    let ledger_path = std::env::var_os(LEDGER_ENV).map(PathBuf::from);
    match (config_path, config_sha, ledger_path) {
        (None, None, None) => Ok(None),
        (Some(config_path), Some(config_sha), Some(ledger_path)) => reserve_from_paths(
            &config_path,
            &config_sha,
            &ledger_path,
            provider,
            model_config,
        )
        .map(Some),
        _ => Err(ProviderError::ExecutionError(format!(
            "benchmark budget requires {CONFIG_ENV}, {CONFIG_SHA_ENV}, and {LEDGER_ENV} together"
        ))),
    }
}

fn reserve_from_paths(
    config_path: &Path,
    expected_sha: &str,
    ledger_path: &Path,
    provider: &str,
    model_config: &ModelConfig,
) -> Result<BenchmarkBudgetReservation, ProviderError> {
    let raw = fs::read(config_path).map_err(|error| {
        ProviderError::ExecutionError(format!("cannot read benchmark budget config: {error}"))
    })?;
    let actual_sha = sha256_hex(&raw);
    if !actual_sha.eq_ignore_ascii_case(expected_sha) {
        return Err(ProviderError::ExecutionError(
            "benchmark budget config hash changed after freeze".to_string(),
        ));
    }
    let config: BudgetConfig = serde_json::from_slice(&raw).map_err(|error| {
        ProviderError::ExecutionError(format!("cannot parse benchmark budget config: {error}"))
    })?;
    validate_config(&config)?;
    let key = format!("{provider}/{}", model_config.model_name);
    let model = config.models.get(&key).ok_or_else(|| {
        ProviderError::ExecutionError(format!("no benchmark budget profile for {key}"))
    })?;
    if model.provider != provider || model.model != model_config.model_name {
        return Err(ProviderError::ExecutionError(format!(
            "benchmark budget identity mismatch for {key}"
        )));
    }
    if model_config.context_limit() != model.context_limit
        || model_config.max_output_tokens() != model.max_output_tokens
    {
        return Err(ProviderError::ExecutionError(format!(
            "benchmark request limits drifted for {key}: context {}/{} output {}/{}",
            model_config.context_limit(),
            model.context_limit,
            model_config.max_output_tokens(),
            model.max_output_tokens
        )));
    }

    let input_reserve_tokens = model.context_limit;
    let output_reserve_tokens = usize::try_from(model.max_output_tokens)
        .map_err(|_| ProviderError::ExecutionError(format!("invalid max output for {key}")))?;
    let reserved_usd = model
        .pricing
        .cost(input_reserve_tokens, output_reserve_tokens);
    let request_id = Uuid::new_v4().to_string();
    let provider_owned = provider.to_string();
    let model_owned = model_config.model_name.clone();

    with_locked_ledger(ledger_path, |ledger| {
        validate_ledger(ledger, &config)?;
        let outstanding_total: f64 = ledger
            .outstanding
            .values()
            .map(|reservation| reservation.reserved_usd)
            .sum();
        let provider_outstanding: f64 = ledger
            .outstanding
            .values()
            .filter(|reservation| reservation.provider == provider_owned)
            .map(|reservation| reservation.reserved_usd)
            .sum();
        let provider_spent = ledger
            .provider_spent_upper_bound
            .get(&provider_owned)
            .copied()
            .unwrap_or_default();
        let provider_cap = config.provider_caps[&provider_owned];
        if ledger.spent_upper_bound + outstanding_total + reserved_usd > config.total_cap
            || provider_spent + provider_outstanding + reserved_usd > provider_cap
        {
            return Err(ProviderError::CreditsExhausted {
                details: format!(
                    "benchmark reserve ${reserved_usd:.6} for {key} does not fit remaining campaign/provider envelope"
                ),
                top_up_url: None,
            });
        }
        ledger.outstanding.insert(
            request_id.clone(),
            OutstandingReservation {
                request_id: request_id.clone(),
                provider: provider_owned.clone(),
                model: model_owned.clone(),
                reserved_usd,
                input_reserve_tokens,
                output_reserve_tokens,
                created_at_unix_ms: unix_ms(),
            },
        );
        Ok(())
    })?;

    Ok(BenchmarkBudgetReservation {
        request_id,
        provider: provider_owned,
        model: model_owned,
        reserved_usd,
        pricing: model.pricing.clone(),
        ledger_path: ledger_path.to_path_buf(),
    })
}

fn validate_config(config: &BudgetConfig) -> Result<(), ProviderError> {
    if config.schema_version != 1
        || config.currency != "USD"
        || !config.total_cap.is_finite()
        || config.total_cap <= 0.0
        || config
            .provider_caps
            .values()
            .any(|cap| !cap.is_finite() || *cap <= 0.0)
        || config.provider_caps.values().sum::<f64>() > config.total_cap
    {
        return Err(ProviderError::ExecutionError(
            "benchmark budget config is invalid".to_string(),
        ));
    }
    for (key, model) in &config.models {
        if key != &format!("{}/{}", model.provider, model.model)
            || !config.provider_caps.contains_key(&model.provider)
            || model.context_limit == 0
            || model.max_output_tokens <= 0
        {
            return Err(ProviderError::ExecutionError(format!(
                "benchmark model budget is invalid: {key}"
            )));
        }
        model.pricing.validate()?;
    }
    Ok(())
}

fn validate_ledger(ledger: &BudgetLedger, config: &BudgetConfig) -> Result<(), ProviderError> {
    if ledger.schema_version != 1
        || ledger.currency != config.currency
        || ledger.total_cap != config.total_cap
        || ledger.provider_caps != config.provider_caps
        || !ledger.spent_upper_bound.is_finite()
        || ledger.spent_upper_bound < 0.0
    {
        return Err(ProviderError::ExecutionError(
            "benchmark budget ledger does not match its frozen config".to_string(),
        ));
    }
    Ok(())
}

fn with_locked_ledger<T>(
    ledger_path: &Path,
    mutate: impl FnOnce(&mut BudgetLedger) -> Result<T, ProviderError>,
) -> Result<T, ProviderError> {
    let lock_path = ledger_path.with_extension("lock");
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| {
            ProviderError::ExecutionError(format!("cannot open budget lock: {error}"))
        })?;
    lock.lock_exclusive().map_err(|error| {
        ProviderError::ExecutionError(format!("cannot lock budget ledger: {error}"))
    })?;

    let result = (|| {
        let file = File::open(ledger_path).map_err(|error| {
            ProviderError::ExecutionError(format!("cannot open budget ledger: {error}"))
        })?;
        let mut ledger: BudgetLedger =
            serde_json::from_reader(BufReader::new(file)).map_err(|error| {
                ProviderError::ExecutionError(format!("cannot parse budget ledger: {error}"))
            })?;
        let value = mutate(&mut ledger)?;
        ledger.updated_at = chrono::Utc::now().to_rfc3339();
        let name = ledger_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("budget-ledger.json");
        let temp_path = ledger_path.with_file_name(format!(".{name}.{}.tmp", Uuid::new_v4()));
        let temp = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .map_err(|error| {
                ProviderError::ExecutionError(format!("cannot create budget ledger temp: {error}"))
            })?;
        let mut writer = BufWriter::new(temp);
        serde_json::to_writer_pretty(&mut writer, &ledger).map_err(|error| {
            ProviderError::ExecutionError(format!("cannot serialize budget ledger: {error}"))
        })?;
        writer.write_all(b"\n").map_err(|error| {
            ProviderError::ExecutionError(format!("cannot write budget ledger: {error}"))
        })?;
        writer.flush().map_err(|error| {
            ProviderError::ExecutionError(format!("cannot flush budget ledger: {error}"))
        })?;
        writer.get_ref().sync_all().map_err(|error| {
            ProviderError::ExecutionError(format!("cannot sync budget ledger: {error}"))
        })?;
        fs::rename(&temp_path, ledger_path).map_err(|error| {
            let _ = fs::remove_file(&temp_path);
            ProviderError::ExecutionError(format!("cannot replace budget ledger: {error}"))
        })?;
        Ok(value)
    })();
    let unlock_result = FileExt::unlock(&lock);
    match (result, unlock_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(ProviderError::ExecutionError(format!(
            "cannot unlock budget ledger: {error}"
        ))),
    }
}

fn unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_providers::conversation::token_usage::Usage;

    fn fixture(root: &Path, provider_cap: f64) -> (PathBuf, String, PathBuf, ModelConfig) {
        let config_path = root.join("budget-config.json");
        let ledger_path = root.join("budget-ledger.json");
        let config = serde_json::json!({
            "schema_version": 1,
            "currency": "USD",
            "total_cap": provider_cap,
            "provider_caps": {"test": provider_cap},
            "models": {
                "test/model": {
                    "provider": "test",
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
        });
        let raw = serde_json::to_vec_pretty(&config).unwrap();
        fs::write(&config_path, &raw).unwrap();
        let sha = sha256_hex(&raw);
        fs::write(
            &ledger_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "currency": "USD",
                "total_cap": provider_cap,
                "provider_caps": {"test": provider_cap},
                "spent_upper_bound": 0.0,
                "provider_spent_upper_bound": {"test": 0.0},
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
        (config_path, sha, ledger_path, model)
    }

    #[test]
    fn reserves_worst_case_then_settles_reported_usage() {
        let root = tempfile::tempdir().unwrap();
        let (config, sha, ledger, model) = fixture(root.path(), 1.0);
        let reservation = reserve_from_paths(&config, &sha, &ledger, "test", &model).unwrap();
        let after_reserve: BudgetLedger =
            serde_json::from_slice(&fs::read(&ledger).unwrap()).unwrap();
        assert_eq!(after_reserve.outstanding.len(), 1);
        assert!(
            (after_reserve.outstanding[reservation.request_id()].reserved_usd - 0.2).abs() < 1e-9
        );

        reservation
            .settle(&ProviderUsage::new(
                "reported-model".to_string(),
                Usage::new(Some(100), Some(200), Some(300)),
            ))
            .unwrap();
        let settled: BudgetLedger = serde_json::from_slice(&fs::read(&ledger).unwrap()).unwrap();
        assert!(settled.outstanding.is_empty());
        assert!((settled.spent_upper_bound - 0.03).abs() < 1e-9);
        assert_eq!(settled.settled[0].reported_model, "reported-model");
    }

    #[test]
    fn refuses_reservation_that_does_not_fit() {
        let root = tempfile::tempdir().unwrap();
        let (config, sha, ledger, model) = fixture(root.path(), 0.1);
        let error = reserve_from_paths(&config, &sha, &ledger, "test", &model)
            .err()
            .expect("reserve must fail");
        assert!(matches!(error, ProviderError::CreditsExhausted { .. }));
    }

    #[test]
    fn missing_usage_keeps_the_full_reservation() {
        let root = tempfile::tempdir().unwrap();
        let (config, sha, ledger, model) = fixture(root.path(), 1.0);
        let reservation = reserve_from_paths(&config, &sha, &ledger, "test", &model).unwrap();
        let error = reservation
            .settle(&ProviderUsage::new(
                "reported-model".to_string(),
                Usage::new(Some(10), None, None),
            ))
            .unwrap_err();
        assert!(matches!(error, ProviderError::UsageError(_)));
        let after: BudgetLedger = serde_json::from_slice(&fs::read(&ledger).unwrap()).unwrap();
        assert_eq!(after.outstanding.len(), 1);
        assert_eq!(after.spent_upper_bound, 0.0);
    }

    #[test]
    fn changed_config_hash_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let (config, _sha, ledger, model) = fixture(root.path(), 1.0);
        let error = reserve_from_paths(&config, "00", &ledger, "test", &model)
            .err()
            .expect("hash mismatch must fail");
        assert!(error.to_string().contains("hash changed"));
    }

    #[test]
    fn unadmitted_release_returns_the_reserve() {
        let root = tempfile::tempdir().unwrap();
        let (config, sha, ledger, model) = fixture(root.path(), 1.0);
        let reservation = reserve_from_paths(&config, &sha, &ledger, "test", &model).unwrap();
        reservation.release_unadmitted().unwrap();
        let after: BudgetLedger = serde_json::from_slice(&fs::read(&ledger).unwrap()).unwrap();
        assert!(after.outstanding.is_empty());
        assert_eq!(after.spent_upper_bound, 0.0);
    }
}
