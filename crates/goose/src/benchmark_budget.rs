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

pub(crate) const CONFIG_ENV: &str = "GOOSE_BENCH_BUDGET_CONFIG";
pub(crate) const CONFIG_SHA_ENV: &str = "GOOSE_BENCH_BUDGET_CONFIG_SHA256";
pub(crate) const LEDGER_ENV: &str = "GOOSE_BENCH_BUDGET_LEDGER";
pub(crate) const EXPECTED_PROVIDER_ENV: &str = "GOOSE_BENCH_EXPECTED_PROVIDER";
pub(crate) const SECRET_ENV_NAME_ENV: &str = "GOOSE_BENCH_SECRET_ENV_NAME";
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
    accepted_reported_models: Vec<String>,
    context_limit: usize,
    max_output_tokens: i32,
    pricing: Pricing,
    #[serde(default)]
    billing: Option<BillingSemantics>,
}

#[derive(Clone, Debug, Deserialize)]
struct BillingSemantics {
    budget_guard_is_actual_charge: bool,
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
    total_tokens: usize,
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
    config: BudgetConfig,
    ledger_path: PathBuf,
}

impl BenchmarkBudgetReservation {
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn release_unadmitted(self) -> Result<(), ProviderError> {
        with_locked_ledger(&self.ledger_path, |ledger| {
            validate_ledger(ledger, &self.config)?;
            ledger.outstanding.remove(&self.request_id).ok_or_else(|| {
                ProviderError::ExecutionError(format!(
                    "budget reservation {} is missing",
                    self.request_id
                ))
            })?;
            validate_ledger(ledger, &self.config)
        })
    }

    pub fn settle(self, usage: &ProviderUsage) -> Result<(), ProviderError> {
        let model_key = format!("{}/{}", self.provider, self.model);
        let model = self.config.models.get(&model_key).ok_or_else(|| {
            ProviderError::ExecutionError(format!(
                "benchmark model profile disappeared for {model_key}; reservation {} remains charged",
                self.request_id
            ))
        })?;
        if !model
            .accepted_reported_models
            .iter()
            .any(|accepted| accepted == &usage.model)
        {
            return Err(ProviderError::UsageError(format!(
                "{} reported unapproved model identity {:?}; reservation {} remains charged",
                self.model, usage.model, self.request_id
            )));
        }
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
        let total_tokens = usage.usage.total_tokens.ok_or_else(|| {
            ProviderError::UsageError(format!(
                "{} returned no total usage; reservation {} remains charged",
                self.model, self.request_id
            ))
        })?;
        if input_tokens < 0 || output_tokens < 0 || total_tokens < 0 {
            return Err(ProviderError::UsageError(format!(
                "{} returned negative usage; reservation {} remains charged",
                self.model, self.request_id
            )));
        }
        if input_tokens.checked_add(output_tokens) != Some(total_tokens) {
            return Err(ProviderError::UsageError(format!(
                "{} returned inconsistent total usage; reservation {} remains charged",
                self.model, self.request_id
            )));
        }
        let input_tokens = usize::try_from(input_tokens).map_err(|_| {
            ProviderError::UsageError(format!(
                "{} returned unrepresentable input usage; reservation {} remains charged",
                self.model, self.request_id
            ))
        })?;
        let output_tokens = usize::try_from(output_tokens).map_err(|_| {
            ProviderError::UsageError(format!(
                "{} returned unrepresentable output usage; reservation {} remains charged",
                self.model, self.request_id
            ))
        })?;
        let total_tokens = usize::try_from(total_tokens).map_err(|_| {
            ProviderError::UsageError(format!(
                "{} returned unrepresentable total usage; reservation {} remains charged",
                self.model, self.request_id
            ))
        })?;
        if input_tokens > model.context_limit
            || output_tokens > usize::try_from(model.max_output_tokens).unwrap_or_default()
        {
            return Err(ProviderError::UsageError(format!(
                "{} returned usage above its frozen request limits; reservation {} remains charged",
                self.model, self.request_id
            )));
        }
        let actual = self.pricing.cost(input_tokens, output_tokens);
        if actual > self.reserved_usd + f64::EPSILON {
            return Err(ProviderError::UsageError(format!(
                "reported usage costs ${actual:.6}, above reservation ${:.6}; reservation {} remains charged",
                self.reserved_usd, self.request_id
            )));
        }

        with_locked_ledger(&self.ledger_path, |ledger| {
            validate_ledger(ledger, &self.config)?;
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
                total_tokens,
                charged_upper_bound_usd: actual,
                reserved_usd: record.reserved_usd,
                settled_at_unix_ms: unix_ms(),
            });
            validate_ledger(ledger, &self.config)
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

pub(crate) fn guard_requested() -> bool {
    [CONFIG_ENV, CONFIG_SHA_ENV, LEDGER_ENV]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
        || std::env::var_os(crate::agents::provider_lifecycle::LIFECYCLE_STRICT_ENV).is_some()
}

pub(crate) fn scrub_bootstrap_secret(provider: &str) -> Result<(), ProviderError> {
    if !guard_requested() {
        return Ok(());
    }
    let expected = std::env::var(EXPECTED_PROVIDER_ENV).map_err(|_| {
        ProviderError::ExecutionError(format!("benchmark guard requires {EXPECTED_PROVIDER_ENV}"))
    })?;
    if provider != expected {
        return Ok(());
    }
    let name = std::env::var(SECRET_ENV_NAME_ENV).map_err(|_| {
        ProviderError::ExecutionError(format!("benchmark guard requires {SECRET_ENV_NAME_ENV}"))
    })?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        || std::env::var_os(&name).is_none()
    {
        return Err(ProviderError::ExecutionError(
            "benchmark bootstrap secret declaration is invalid".to_string(),
        ));
    }
    std::env::remove_var(&name);
    if std::env::var_os(&name).is_some() {
        return Err(ProviderError::ExecutionError(
            "benchmark bootstrap secret could not be removed from the agent environment"
                .to_string(),
        ));
    }
    Ok(())
}

pub fn assert_bootstrap_secret_scrubbed() -> Result<(), ProviderError> {
    if !guard_requested() {
        return Ok(());
    }
    let name = std::env::var(SECRET_ENV_NAME_ENV).map_err(|_| {
        ProviderError::ExecutionError(format!("benchmark guard requires {SECRET_ENV_NAME_ENV}"))
    })?;
    if std::env::var_os(&name).is_some() {
        return Err(ProviderError::ExecutionError(
            "benchmark provider was not initialized and its bootstrap secret remains exposed"
                .to_string(),
        ));
    }
    Ok(())
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
            let reserve_label = if model
                .billing
                .as_ref()
                .is_some_and(|billing| !billing.budget_guard_is_actual_charge)
            {
                "PAYG-equivalent shadow guard"
            } else {
                "benchmark"
            };
            return Err(ProviderError::CreditsExhausted {
                details: format!(
                    "{reserve_label} reserve ${reserved_usd:.6} for {key} does not fit remaining campaign/provider envelope"
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
        config,
        ledger_path: ledger_path.to_path_buf(),
    })
}

#[cfg(test)]
pub(crate) fn reserve_from_paths_for_test(
    config_path: &Path,
    ledger_path: &Path,
    provider: &str,
    model_config: &ModelConfig,
) -> Result<BenchmarkBudgetReservation, ProviderError> {
    let config = fs::read(config_path).map_err(|error| {
        ProviderError::ExecutionError(format!("cannot read benchmark budget config: {error}"))
    })?;
    reserve_from_paths(
        config_path,
        &sha256_hex(&config),
        ledger_path,
        provider,
        model_config,
    )
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
            || model.accepted_reported_models.is_empty()
            || model
                .accepted_reported_models
                .iter()
                .any(|reported| reported.is_empty())
            || model
                .accepted_reported_models
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != model.accepted_reported_models.len()
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
        || ledger
            .provider_spent_upper_bound
            .keys()
            .collect::<std::collections::HashSet<_>>()
            != config
                .provider_caps
                .keys()
                .collect::<std::collections::HashSet<_>>()
    {
        return Err(ProviderError::ExecutionError(
            "benchmark budget ledger does not match its frozen config".to_string(),
        ));
    }

    let invalid = || {
        ProviderError::ExecutionError(
            "benchmark budget ledger contains inconsistent accounting evidence".to_string(),
        )
    };
    if ledger
        .provider_spent_upper_bound
        .values()
        .any(|amount| !amount.is_finite() || *amount < 0.0)
        || ledger.spent_upper_bound > config.total_cap + f64::EPSILON
    {
        return Err(invalid());
    }

    let mut request_ids = std::collections::HashSet::new();
    let mut outstanding_total = 0.0;
    let mut outstanding_by_provider: HashMap<&str, f64> = HashMap::new();
    for (request_id, reservation) in &ledger.outstanding {
        let model_key = format!("{}/{}", reservation.provider, reservation.model);
        let Some(model) = config.models.get(&model_key) else {
            return Err(invalid());
        };
        let Ok(max_output_tokens) = usize::try_from(model.max_output_tokens) else {
            return Err(invalid());
        };
        let expected_reserve = model.pricing.cost(model.context_limit, max_output_tokens);
        if request_id != &reservation.request_id
            || !request_ids.insert(request_id.as_str())
            || reservation.request_id.is_empty()
            || reservation.provider != model.provider
            || reservation.model != model.model
            || !reservation.reserved_usd.is_finite()
            || reservation.reserved_usd < 0.0
            || !money_eq(reservation.reserved_usd, expected_reserve)
            || reservation.input_reserve_tokens != model.context_limit
            || reservation.output_reserve_tokens != max_output_tokens
        {
            return Err(invalid());
        }
        outstanding_total += reservation.reserved_usd;
        *outstanding_by_provider
            .entry(reservation.provider.as_str())
            .or_default() += reservation.reserved_usd;
    }

    let mut settled_total = 0.0;
    let mut settled_by_provider: HashMap<&str, f64> = HashMap::new();
    for settlement in &ledger.settled {
        let model_key = format!("{}/{}", settlement.provider, settlement.model);
        let Some(model) = config.models.get(&model_key) else {
            return Err(invalid());
        };
        let Ok(max_output_tokens) = usize::try_from(model.max_output_tokens) else {
            return Err(invalid());
        };
        let expected_reserve = model.pricing.cost(model.context_limit, max_output_tokens);
        let expected_charge = model
            .pricing
            .cost(settlement.input_tokens, settlement.output_tokens);
        let expected_total = settlement
            .input_tokens
            .checked_add(settlement.output_tokens);
        if settlement.request_id.is_empty()
            || !request_ids.insert(settlement.request_id.as_str())
            || settlement.provider != model.provider
            || settlement.model != model.model
            || !model
                .accepted_reported_models
                .contains(&settlement.reported_model)
            || expected_total != Some(settlement.total_tokens)
            || settlement.input_tokens > model.context_limit
            || settlement.output_tokens > max_output_tokens
            || !settlement.charged_upper_bound_usd.is_finite()
            || settlement.charged_upper_bound_usd < 0.0
            || !settlement.reserved_usd.is_finite()
            || settlement.reserved_usd < 0.0
            || !money_eq(settlement.reserved_usd, expected_reserve)
            || !money_eq(settlement.charged_upper_bound_usd, expected_charge)
            || settlement.charged_upper_bound_usd > settlement.reserved_usd + f64::EPSILON
        {
            return Err(invalid());
        }
        settled_total += settlement.charged_upper_bound_usd;
        *settled_by_provider
            .entry(settlement.provider.as_str())
            .or_default() += settlement.charged_upper_bound_usd;
    }

    if !money_eq(ledger.spent_upper_bound, settled_total)
        || !money_eq(
            ledger.spent_upper_bound,
            ledger.provider_spent_upper_bound.values().sum(),
        )
    {
        return Err(invalid());
    }
    for (provider, cap) in &config.provider_caps {
        let recorded = ledger.provider_spent_upper_bound[provider];
        let derived = settled_by_provider
            .get(provider.as_str())
            .copied()
            .unwrap_or_default();
        let outstanding = outstanding_by_provider
            .get(provider.as_str())
            .copied()
            .unwrap_or_default();
        if !money_eq(recorded, derived) || recorded + outstanding > *cap + f64::EPSILON {
            return Err(invalid());
        }
    }
    if ledger.spent_upper_bound + outstanding_total > config.total_cap + f64::EPSILON {
        return Err(invalid());
    }
    Ok(())
}

fn money_eq(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-9_f64.max(left.abs().max(right.abs()) * 1e-12)
}

fn with_locked_ledger<T>(
    ledger_path: &Path,
    mutate: impl FnOnce(&mut BudgetLedger) -> Result<T, ProviderError>,
) -> Result<T, ProviderError> {
    let lock_path = ledger_path.with_extension("lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
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
                    "accepted_reported_models": ["reported-model"],
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
    fn shadow_guard_reservation_error_cannot_be_read_as_actual_spend() {
        let root = tempfile::tempdir().unwrap();
        let (config, _sha, ledger, model) = fixture(root.path(), 0.1);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        value["models"]["test/model"]["billing"] = serde_json::json!({
            "budget_guard_is_actual_charge": false
        });
        let raw = serde_json::to_vec_pretty(&value).unwrap();
        fs::write(&config, &raw).unwrap();
        let error = reserve_from_paths(&config, &sha256_hex(&raw), &ledger, "test", &model)
            .err()
            .expect("shadow guard reserve must fail");
        match error {
            ProviderError::CreditsExhausted { details, .. } => {
                assert!(details.contains("PAYG-equivalent shadow guard reserve $"));
                assert!(!details.starts_with("benchmark reserve $"));
            }
            other => panic!("unexpected error: {other}"),
        }
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
    fn unapproved_reported_model_keeps_the_full_reservation() {
        let root = tempfile::tempdir().unwrap();
        let (config, sha, ledger, model) = fixture(root.path(), 1.0);
        let reservation = reserve_from_paths(&config, &sha, &ledger, "test", &model).unwrap();
        let error = reservation
            .settle(&ProviderUsage::new(
                "different-model".to_string(),
                Usage::new(Some(10), Some(20), Some(30)),
            ))
            .unwrap_err();

        assert!(matches!(error, ProviderError::UsageError(_)));
        let after: BudgetLedger = serde_json::from_slice(&fs::read(&ledger).unwrap()).unwrap();
        assert_eq!(after.outstanding.len(), 1);
        assert_eq!(after.spent_upper_bound, 0.0);
    }

    #[test]
    fn inconsistent_or_over_limit_usage_keeps_the_full_reservation() {
        for usage in [
            Usage::new(Some(10), Some(20), Some(31)),
            Usage::new(Some(1001), Some(20), Some(1021)),
            Usage::new(Some(10), Some(1001), Some(1011)),
            Usage::new(Some(i64::MAX), Some(i64::MAX), Some(i64::MAX)),
        ] {
            let root = tempfile::tempdir().unwrap();
            let (config, sha, ledger, model) = fixture(root.path(), 1.0);
            let reservation = reserve_from_paths(&config, &sha, &ledger, "test", &model).unwrap();
            let error = reservation
                .settle(&ProviderUsage::new("reported-model".to_string(), usage))
                .unwrap_err();

            assert!(matches!(error, ProviderError::UsageError(_)));
            let after: BudgetLedger = serde_json::from_slice(&fs::read(&ledger).unwrap()).unwrap();
            assert_eq!(after.outstanding.len(), 1);
            assert_eq!(after.spent_upper_bound, 0.0);
        }
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

    fn assert_ledger_tamper_refused(tamper: impl FnOnce(&mut serde_json::Value)) {
        let root = tempfile::tempdir().unwrap();
        let (config, sha, ledger, model) = fixture(root.path(), 1.0);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&ledger).unwrap()).unwrap();
        tamper(&mut value);
        fs::write(&ledger, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let error = reserve_from_paths(&config, &sha, &ledger, "test", &model)
            .err()
            .expect("tampered accounting must fail closed");
        assert!(error.to_string().contains("budget ledger"));
    }

    #[test]
    fn rejects_negative_missing_and_unreconciled_accounting() {
        assert_ledger_tamper_refused(|ledger| ledger["spent_upper_bound"] = (-0.01).into());
        assert_ledger_tamper_refused(|ledger| {
            ledger["provider_spent_upper_bound"] = serde_json::json!({})
        });
        assert_ledger_tamper_refused(|ledger| ledger["spent_upper_bound"] = 0.01.into());
    }

    #[test]
    fn rejects_forged_outstanding_reservations() {
        assert_ledger_tamper_refused(|ledger| {
            ledger["outstanding"] = serde_json::json!({
                "forged": {
                    "request_id": "different-id",
                    "provider": "test",
                    "model": "model",
                    "reserved_usd": 0.2,
                    "input_reserve_tokens": 1000,
                    "output_reserve_tokens": 1000,
                    "created_at_unix_ms": 1
                }
            });
        });
        assert_ledger_tamper_refused(|ledger| {
            ledger["outstanding"] = serde_json::json!({
                "forged": {
                    "request_id": "forged",
                    "provider": "unknown",
                    "model": "model",
                    "reserved_usd": 0.2,
                    "input_reserve_tokens": 1000,
                    "output_reserve_tokens": 1000,
                    "created_at_unix_ms": 1
                }
            });
        });
    }

    #[test]
    fn rejects_removed_or_mutated_settlement_evidence() {
        for mutation in ["remove", "negative", "duplicate"] {
            let root = tempfile::tempdir().unwrap();
            let (config, sha, ledger, model) = fixture(root.path(), 1.0);
            reserve_from_paths(&config, &sha, &ledger, "test", &model)
                .unwrap()
                .settle(&ProviderUsage::new(
                    "reported-model".to_string(),
                    Usage::new(Some(100), Some(200), Some(300)),
                ))
                .unwrap();
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&ledger).unwrap()).unwrap();
            match mutation {
                "remove" => value["settled"] = serde_json::json!([]),
                "negative" => value["settled"][0]["charged_upper_bound_usd"] = (-0.03).into(),
                "duplicate" => {
                    let duplicate = value["settled"][0].clone();
                    value["settled"].as_array_mut().unwrap().push(duplicate);
                }
                _ => unreachable!(),
            }
            fs::write(&ledger, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            let error = reserve_from_paths(&config, &sha, &ledger, "test", &model)
                .err()
                .expect("corrupt settlement evidence must fail closed");
            assert!(error.to_string().contains("budget ledger"));
        }
    }

    #[test]
    fn benchmark_secret_must_be_consumed_by_the_expected_provider() {
        let _guard = env_lock::lock_env([
            (
                crate::agents::provider_lifecycle::LIFECYCLE_STRICT_ENV,
                Some("true"),
            ),
            (EXPECTED_PROVIDER_ENV, Some("google")),
            (SECRET_ENV_NAME_ENV, Some("GOOSE_TEST_PROVIDER_SECRET")),
            ("GOOSE_TEST_PROVIDER_SECRET", Some("never-visible-to-tools")),
        ]);

        scrub_bootstrap_secret("lmstudio").unwrap();
        assert!(assert_bootstrap_secret_scrubbed().is_err());
        scrub_bootstrap_secret("google").unwrap();
        assert_bootstrap_secret_scrubbed().unwrap();
        assert!(std::env::var_os("GOOSE_TEST_PROVIDER_SECRET").is_none());
    }
}
