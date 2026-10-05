use crate::base::Provider;
use crate::engine_hold;
use crate::errors::ProviderError;
use async_trait::async_trait;
use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

pub const DEFAULT_MAX_RETRIES: usize = 3;
pub const DEFAULT_INITIAL_RETRY_INTERVAL_MS: u64 = 1000;
pub const DEFAULT_BACKOFF_MULTIPLIER: f64 = 2.0;
pub const DEFAULT_MAX_RETRY_INTERVAL_MS: u64 = 30_000;
/// A rate limit is waited out on its own budget, not the 3 quick resends every other failure gets.
/// 2026-10-05: qwen/qwen3.8-flash (one host, Alibaba, via OpenRouter) answered 429 "Provider returned
/// error" at call ~148 of a 150-call benchmark run and again 68 s into the next; 1 + 2 + 4 s of retries
/// ended both sessions, and a provider-ended session is not scored (Ling 3.1 Flash died the same way on
/// 10-03). `GOOSE_RATE_LIMIT_MAX_WAIT_SECS` sets the total wait; 0 restores the old behaviour.
pub const DEFAULT_RATE_LIMIT_MAX_WAIT_SECS: u64 = 1_800;
pub const DEFAULT_RATE_LIMIT_INITIAL_INTERVAL_MS: u64 = 5_000;
pub const DEFAULT_RATE_LIMIT_MAX_INTERVAL_MS: u64 = 120_000;

#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: usize,
    /// Initial interval between retries in milliseconds
    pub initial_interval_ms: u64,
    /// Multiplier for backoff (exponential)
    pub backoff_multiplier: f64,
    /// Maximum interval between retries in milliseconds
    pub max_interval_ms: u64,
    /// When true, only retry on transient errors (ServerError, NetworkError,
    /// RateLimitExceeded). RequestFailed (4xx client errors) will not be retried.
    pub transient_only: bool,
    /// Total time a run of rate-limit (429) answers may be waited out before the ordinary retries decide.
    pub rate_limit_max_wait: Duration,
    /// First rate-limit backoff (doubles per consecutive 429, capped at `rate_limit_max_interval_ms`)
    /// when the provider names no delay of its own.
    pub rate_limit_initial_interval_ms: u64,
    pub rate_limit_max_interval_ms: u64,
}

fn rate_limit_max_wait_from_env() -> Duration {
    Duration::from_secs(
        std::env::var("GOOSE_RATE_LIMIT_MAX_WAIT_SECS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_RATE_LIMIT_MAX_WAIT_SECS),
    )
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            initial_interval_ms: DEFAULT_INITIAL_RETRY_INTERVAL_MS,
            backoff_multiplier: DEFAULT_BACKOFF_MULTIPLIER,
            max_interval_ms: DEFAULT_MAX_RETRY_INTERVAL_MS,
            transient_only: false,
            rate_limit_max_wait: rate_limit_max_wait_from_env(),
            rate_limit_initial_interval_ms: DEFAULT_RATE_LIMIT_INITIAL_INTERVAL_MS,
            rate_limit_max_interval_ms: DEFAULT_RATE_LIMIT_MAX_INTERVAL_MS,
        }
    }
}

impl RetryConfig {
    pub fn new(
        max_retries: usize,
        initial_interval_ms: u64,
        backoff_multiplier: f64,
        max_interval_ms: u64,
    ) -> Self {
        Self {
            max_retries,
            initial_interval_ms,
            backoff_multiplier,
            max_interval_ms,
            transient_only: false,
            rate_limit_max_wait: rate_limit_max_wait_from_env(),
            rate_limit_initial_interval_ms: DEFAULT_RATE_LIMIT_INITIAL_INTERVAL_MS,
            rate_limit_max_interval_ms: DEFAULT_RATE_LIMIT_MAX_INTERVAL_MS,
        }
    }

    pub fn rate_limit_wait(mut self, max_wait: Duration) -> Self {
        self.rate_limit_max_wait = max_wait;
        self
    }

    /// The wait before resending after the `streak`-th consecutive rate limit (1-based), or None when
    /// `waited` plus that wait would pass `rate_limit_max_wait` (the ordinary retries then decide).
    /// The provider's own Retry-After wins over the backoff.
    pub fn next_rate_limit_wait(
        &self,
        error: &ProviderError,
        streak: usize,
        waited: Duration,
    ) -> Option<Duration> {
        let ProviderError::RateLimitExceeded { retry_delay, .. } = error else {
            return None;
        };
        if short_rate_limits() {
            return None;
        }
        let delay = retry_delay.unwrap_or_else(|| {
            let exp = streak.saturating_sub(1).min(20) as i32;
            let ms = (self.rate_limit_initial_interval_ms as f64 * 2f64.powi(exp)) as u64;
            Duration::from_millis(ms.min(self.rate_limit_max_interval_ms))
        });
        (waited + delay <= self.rate_limit_max_wait).then_some(delay)
    }

    pub fn transient_only(mut self) -> Self {
        self.transient_only = true;
        self
    }

    pub fn max_retries(&self) -> usize {
        self.max_retries
    }

    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        if attempt == 0 {
            return Duration::from_millis(0);
        }

        let exponent = (attempt - 1) as u32;
        let base_delay_ms = (self.initial_interval_ms as f64
            * self.backoff_multiplier.powi(exponent as i32)) as u64;

        let capped_delay_ms = std::cmp::min(base_delay_ms, self.max_interval_ms);

        let jitter_factor_to_avoid_thundering_herd = 0.8 + (rand::random::<f64>() * 0.4);
        let jitter_delay_ms =
            (capped_delay_ms as f64 * jitter_factor_to_avoid_thundering_herd) as u64;

        Duration::from_millis(jitter_delay_ms)
    }

    /// The wait before resend `attempt` (1-based) of a request that failed with `error`: the
    /// provider's own delay when a rate limit named one, else this policy's backoff.
    pub fn delay_before_retry(&self, error: &ProviderError, attempt: usize) -> Duration {
        match error {
            ProviderError::RateLimitExceeded {
                retry_delay: Some(provider_delay),
                ..
            } => *provider_delay,
            _ => self.delay_for_attempt(attempt),
        }
    }
}

tokio::task_local! {
    static SHORT_RATE_LIMITS: ();
}

/// Run `f` with the ordinary quick retries for rate limits instead of the long rate-limit wait: a
/// provider CONNECTION CHECK must answer in seconds ("Refresh providers"), not wait out a 429 for
/// up to `GOOSE_RATE_LIMIT_MAX_WAIT_SECS`.
pub async fn with_short_rate_limits<F: Future>(f: F) -> F::Output {
    SHORT_RATE_LIMITS.scope((), f).await
}

fn short_rate_limits() -> bool {
    SHORT_RATE_LIMITS.try_with(|_| ()).is_ok()
}

/// `GOOSE_PROVIDER_SKIP_BACKOFF=true` resends without waiting out the backoff.
pub fn backoff_skipped() -> bool {
    std::env::var("GOOSE_PROVIDER_SKIP_BACKOFF")
        .unwrap_or_default()
        .parse::<bool>()
        .unwrap_or(false)
}

/// Substrings marking a `RequestFailed` (4xx) as deterministically permanent:
/// Anthropic rejects signed `thinking`/`redacted_thinking` blocks as immutable
/// once a thinking model's config changes mid-conversation, and the identical
/// payload is rebuilt on every retry — so retrying can never succeed.
const PERMANENT_REQUEST_FAILURE_MARKERS: &[&str] = &[
    "blocks in the latest assistant message cannot be modified",
    "must remain as they were in the original response",
];

fn is_permanent_request_failure(message: &str) -> bool {
    PERMANENT_REQUEST_FAILURE_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
}

pub fn should_retry(error: &ProviderError, config: &RetryConfig) -> bool {
    match error {
        e if e.is_transient() => true,
        ProviderError::RequestFailed(message) if is_permanent_request_failure(message) => false,
        ProviderError::RequestFailed(_) => !config.transient_only,
        _ => false,
    }
}

/// Q-397: an engine's memory hold is waited out, not retried. `None` = not a hold (the ordinary
/// retries decide); `Some(Ok)` = the hold lifted, send again without counting an attempt;
/// `Some(Err)` = end with it — the caller fails over holds itself, or the wait ended without a lift.
async fn outlast_hold(error: &ProviderError) -> Option<Result<(), ProviderError>> {
    if !matches!(error, ProviderError::EngineHold { .. }) {
        return None;
    }
    if engine_hold::hold_goes_to_caller() {
        return Some(Err(error.clone()));
    }
    Some(engine_hold::wait_for_admission(error).await)
}

pub async fn retry_operation<F, Fut, T>(
    config: &RetryConfig,
    operation: F,
) -> Result<T, ProviderError>
where
    F: Fn() -> Fut + Send,
    Fut: Future<Output = Result<T, ProviderError>> + Send,
    T: Send,
{
    let mut attempts = 0;
    let (mut rl_streak, mut rl_waited) = (0usize, Duration::ZERO);

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(error) => {
                if let Some(lifted) = outlast_hold(&error).await {
                    lifted?;
                    continue;
                }
                if let Some(delay) = config.next_rate_limit_wait(&error, rl_streak + 1, rl_waited) {
                    rl_streak += 1;
                    rl_waited += delay;
                    tracing::warn!(
                        "Rate limited, waiting {:?} before resending ({:?} of {:?} waited): {:?}",
                        delay,
                        rl_waited,
                        config.rate_limit_max_wait,
                        error
                    );
                    if !backoff_skipped() {
                        sleep(delay).await;
                    }
                    continue;
                }
                if should_retry(&error, config) && attempts < config.max_retries {
                    attempts += 1;
                    tracing::warn!(
                        "Request failed, retrying ({}/{}): {:?}",
                        attempts,
                        config.max_retries,
                        error
                    );

                    sleep(config.delay_before_retry(&error, attempts)).await;
                    continue;
                }
                return Err(error);
            }
        }
    }
}

/// Trait for retry functionality to keep Provider dyn-compatible.
///
/// All `Provider` implementors get this via the blanket impl below.
#[async_trait]
pub trait ProviderRetry {
    fn retry_config(&self) -> RetryConfig {
        RetryConfig::default()
    }

    async fn with_retry<F, Fut, T>(&self, operation: F) -> Result<T, ProviderError>
    where
        F: Fn() -> Fut + Send,
        Fut: Future<Output = Result<T, ProviderError>> + Send,
        T: Send,
    {
        self.with_retry_config(operation, self.retry_config()).await
    }

    async fn with_retry_config<F, Fut, T>(
        &self,
        operation: F,
        config: RetryConfig,
    ) -> Result<T, ProviderError>
    where
        F: Fn() -> Fut + Send,
        Fut: Future<Output = Result<T, ProviderError>> + Send,
        T: Send;
}

#[async_trait]
impl<P: Provider> ProviderRetry for P {
    fn retry_config(&self) -> RetryConfig {
        Provider::retry_config(self)
    }

    async fn with_retry_config<F, Fut, T>(
        &self,
        operation: F,
        config: RetryConfig,
    ) -> Result<T, ProviderError>
    where
        F: Fn() -> Fut + Send,
        Fut: Future<Output = Result<T, ProviderError>> + Send,
        T: Send,
    {
        let mut attempts = 0;
        let mut auth_retried = false;
        let (mut rl_streak, mut rl_waited) = (0usize, Duration::ZERO);

        loop {
            return match operation().await {
                Ok(result) => Ok(result),
                Err(error) => {
                    // Auth retry is separate from transient-error retries: we get
                    // at most 1 credential refresh, independent of max_retries.
                    if matches!(error, ProviderError::Authentication(_)) && !auth_retried {
                        auth_retried = true;
                        match self.refresh_credentials().await {
                            Ok(()) => {
                                tracing::warn!(
                                    "Credentials refreshed after auth error, retrying: {:?}",
                                    error
                                );
                                continue;
                            }
                            Err(refresh_err) => {
                                tracing::warn!(
                                    "Credential refresh failed, returning original auth error: {:?}",
                                    refresh_err
                                );
                            }
                        }
                    }

                    if let Some(lifted) = outlast_hold(&error).await {
                        lifted?;
                        continue;
                    }

                    if let Some(delay) =
                        config.next_rate_limit_wait(&error, rl_streak + 1, rl_waited)
                    {
                        rl_streak += 1;
                        rl_waited += delay;
                        tracing::warn!(
                            "Rate limited, waiting {:?} before resending ({:?} of {:?} waited): {:?}",
                            delay,
                            rl_waited,
                            config.rate_limit_max_wait,
                            error
                        );
                        if !backoff_skipped() {
                            sleep(delay).await;
                        }
                        continue;
                    }

                    if should_retry(&error, &config) && attempts < config.max_retries {
                        attempts += 1;
                        tracing::warn!(
                            "Request failed, retrying ({}/{}): {:?}",
                            attempts,
                            config.max_retries,
                            error
                        );

                        let delay = config.delay_before_retry(&error, attempts);
                        if backoff_skipped() {
                            tracing::info!("Skipping backoff due to GOOSE_PROVIDER_SKIP_BACKOFF");
                        } else {
                            tracing::info!("Backing off for {:?} before retry", delay);
                            sleep(delay).await;
                        }
                        continue;
                    }

                    Err(error)
                }
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_retries_request_failed() {
        let config = RetryConfig::default();
        let error = ProviderError::RequestFailed("Bad request (400): model not found".into());
        assert!(should_retry(&error, &config));
    }

    #[test]
    fn never_retries_permanent_thinking_block_400() {
        let config = RetryConfig::default();
        let error = ProviderError::RequestFailed(
            "Bad request (400): {\"message\":\"messages.3.content.1: `thinking` or \
             `redacted_thinking` blocks in the latest assistant message cannot be \
             modified. These blocks must remain as they were in the original \
             response.\"}"
                .into(),
        );
        assert!(!should_retry(&error, &config));
    }

    #[test]
    fn permanent_request_failure_marker_detection() {
        assert!(is_permanent_request_failure(
            "messages.3.content.1: `thinking` blocks in the latest assistant message \
             cannot be modified"
        ));
        assert!(is_permanent_request_failure(
            "These blocks must remain as they were in the original response."
        ));
        assert!(!is_permanent_request_failure(
            "Bad request (400): model not found"
        ));
    }

    #[test]
    fn transient_only_skips_request_failed() {
        let config = RetryConfig::default().transient_only();
        let error = ProviderError::RequestFailed("Bad request (400): model not found".into());
        assert!(!should_retry(&error, &config));
    }

    #[test]
    fn transient_only_still_retries_server_error() {
        let config = RetryConfig::default().transient_only();
        assert!(should_retry(
            &ProviderError::ServerError("500 internal".into()),
            &config
        ));
    }

    #[test]
    fn transient_only_still_retries_network_error() {
        let config = RetryConfig::default().transient_only();
        assert!(should_retry(
            &ProviderError::NetworkError("connection refused".into()),
            &config
        ));
    }

    #[test]
    fn transient_only_still_retries_rate_limit() {
        let config = RetryConfig::default().transient_only();
        assert!(should_retry(
            &ProviderError::RateLimitExceeded {
                details: "too many requests".into(),
                retry_delay: None,
            },
            &config
        ));
    }

    fn rate_limited(delay: Option<Duration>) -> ProviderError {
        ProviderError::RateLimitExceeded {
            details: "Provider returned error".into(),
            retry_delay: delay,
        }
    }

    #[test]
    fn rate_limits_back_off_on_their_own_budget() {
        let config = RetryConfig::default().rate_limit_wait(Duration::from_secs(1_800));
        let e = rate_limited(None);
        assert_eq!(
            config.next_rate_limit_wait(&e, 1, Duration::ZERO),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            config.next_rate_limit_wait(&e, 2, Duration::ZERO),
            Some(Duration::from_secs(10))
        );
        assert_eq!(
            config.next_rate_limit_wait(&e, 9, Duration::ZERO),
            Some(Duration::from_secs(120))
        );
        // the provider's own Retry-After wins
        let named = rate_limited(Some(Duration::from_secs(42)));
        assert_eq!(
            config.next_rate_limit_wait(&named, 1, Duration::ZERO),
            Some(Duration::from_secs(42))
        );
        // the total budget ends it; then the ordinary retries decide
        assert_eq!(
            config.next_rate_limit_wait(&e, 20, Duration::from_secs(1_750)),
            None
        );
        // not a rate limit: no rate-limit wait
        assert_eq!(
            config.next_rate_limit_wait(
                &ProviderError::ServerError("500".into()),
                1,
                Duration::ZERO
            ),
            None
        );
        // zero budget = the old behaviour
        let off = RetryConfig::default().rate_limit_wait(Duration::ZERO);
        assert_eq!(off.next_rate_limit_wait(&e, 1, Duration::ZERO), None);
    }

    #[tokio::test]
    async fn a_run_of_rate_limits_longer_than_the_ordinary_retries_is_waited_out() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let mut config = RetryConfig::default().rate_limit_wait(Duration::from_millis(500));
        config.rate_limit_initial_interval_ms = 1;
        config.rate_limit_max_interval_ms = 2;
        config.initial_interval_ms = 1;
        config.max_interval_ms = 1;
        let calls = AtomicUsize::new(0);
        // 8 consecutive 429s: the 3 ordinary retries alone would give up after the 4th call
        let result = retry_operation(&config, || {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if n < 8 {
                    Err(rate_limited(None))
                } else {
                    Ok(n)
                }
            }
        })
        .await;
        assert_eq!(result.unwrap(), 8);
        // control: with the rate-limit budget off, the same run ends on the 4th call
        let off = RetryConfig {
            rate_limit_max_wait: Duration::ZERO,
            ..config.clone()
        };
        let calls = AtomicUsize::new(0);
        let result = retry_operation(&off, || {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if n < 8 {
                    Err::<usize, _>(rate_limited(None))
                } else {
                    Ok(n)
                }
            }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        // a connection check keeps the quick retries even with the budget on
        let calls = AtomicUsize::new(0);
        let result = with_short_rate_limits(retry_operation(&config, || {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if n < 8 {
                    Err::<usize, _>(rate_limited(None))
                } else {
                    Ok(n)
                }
            }
        }))
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn never_retries_auth_errors() {
        let config = RetryConfig::default();
        assert!(!should_retry(
            &ProviderError::Authentication("invalid key".into()),
            &config
        ));
    }
}
