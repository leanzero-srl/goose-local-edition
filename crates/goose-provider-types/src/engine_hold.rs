//! A serving engine that holds new requests until its memory recovers (Q-397).
//!
//! goose's distributed engine (goose-sidecar's supervisor) closes rank 0's admission when a node's
//! memory runs short and reopens it when memory recovered on every node. While it is closed a new
//! request is answered 503 with `error.code == "memory_hold"`, the watchdog's `reason`, and
//! `admission` — the path whose GET answers the moment admission reopens. E2E #3r turn 18
//! (2026-09-28 14:18:25): goose's retry spent its three tries in ~4 s of a hold that lasted
//! minutes and the whole turn ended in an error.
//!
//! The hold is recognised by that code, never by the message's prose, and waited out on the
//! engine's own event: no clock, no count (gate 5). The wait ends when the engine admits, when the
//! caller drops the future (the person's Stop), or when the engine goes away (the connection ends
//! with an error, said as such). Any other 503 is untouched and keeps the ordinary retries.

use std::sync::Arc;

use reqwest::StatusCode;
use serde_json::Value;

use crate::errors::ProviderError;

/// The code the refusal carries while the engine holds for memory (goose-sidecar
/// `distributed::MEMORY_HOLD_CODE`; the goose crate pins the two equal).
pub const MEMORY_HOLD_CODE: &str = "memory_hold";

/// The words the turn line shows once the hold lifted and the request goes out again.
pub const ADMITTED_WORDS: &str =
    "Memory recovered: the engine admits new requests again — sending the request";

/// What a hold does, as a surface that shows the turn's progress hears it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineHoldEvent {
    /// The engine refused the request for the hold and goose waits for the lift.
    Waiting { words: String },
    /// The hold lifted; the request is sent again.
    Admitted { words: String },
}

pub type EngineHoldObserver = Arc<dyn Fn(EngineHoldEvent) + Send + Sync>;

tokio::task_local! {
    /// Set by a surface that shows the turn's progress (the desktop's prompt loop) around the
    /// future that polls the provider. Unset — every other caller — the wait is the same and
    /// only the log names it.
    pub static ENGINE_HOLD_OBSERVER: EngineHoldObserver;

    /// Set by a caller that can send the request elsewhere while one engine holds (the swarm
    /// router's failover): the hold comes back to it at once instead of being waited out here,
    /// and the caller waits on it (`wait_for_admission`) only when nothing else can serve.
    pub static HOLD_GOES_TO_CALLER: ();
}

/// Whether the caller asked for holds back (see [`HOLD_GOES_TO_CALLER`]).
pub fn hold_goes_to_caller() -> bool {
    HOLD_GOES_TO_CALLER.try_with(|_| ()).is_ok()
}

fn notify(event: EngineHoldEvent) {
    // Unset is the designed default (every caller but the desktop's prompt loop): a pure side
    // channel, the wait itself is identical either way.
    let _ = ENGINE_HOLD_OBSERVER.try_with(|observer| observer(event));
}

/// The turn line while goose waits: the engine's own reason when it gave one (the watchdog names
/// whose memory it is and what frees it), else what the code itself says.
pub fn waiting_words(reason: Option<&str>) -> String {
    match reason {
        Some(reason) if !reason.trim().is_empty() => format!("Waiting: {reason}"),
        _ => "Waiting: the engine is holding new requests until memory recovers".to_string(),
    }
}

/// The hold a refusal names, or `None` for any other response. Only a 503 whose
/// `error.code` is [`MEMORY_HOLD_CODE`] is a hold; `details` is the framing goose gives every
/// server error. `url` is the refused request's (already sanitized) URL: the wait path is resolved
/// against its origin.
pub fn hold_of_refusal(
    status: StatusCode,
    payload: Option<&Value>,
    url: &str,
    details: impl FnOnce() -> String,
) -> Option<ProviderError> {
    if status != StatusCode::SERVICE_UNAVAILABLE {
        return None;
    }
    let error = payload?.get("error")?;
    if error.get("code").and_then(Value::as_str) != Some(MEMORY_HOLD_CODE) {
        return None;
    }
    let admission_url = error
        .get("admission")
        .and_then(Value::as_str)
        .and_then(|path| reqwest::Url::parse(url).ok()?.join(path).ok())
        .map(|url| url.to_string());
    Some(ProviderError::EngineHold {
        details: details(),
        reason: error
            .get("reason")
            .and_then(Value::as_str)
            .map(str::to_string),
        admission_url,
    })
}

/// Waits until the holding engine admits again: `Ok` = send the request again. The turn line
/// hears [`EngineHoldEvent::Waiting`] first and [`EngineHoldEvent::Admitted`] at the lift. An
/// engine that goes away while it holds (its connection ends), or a hold that names nowhere to
/// wait, ends the wait with a server error that says so beside the hold's own words.
pub async fn wait_for_admission(hold: &ProviderError) -> Result<(), ProviderError> {
    let ProviderError::EngineHold {
        details,
        reason,
        admission_url,
    } = hold
    else {
        return Err(hold.clone());
    };
    let ended = |why: String| {
        ProviderError::ServerError(format!(
            "{details} — goose waited for the engine to admit requests again, but {why}"
        ))
    };
    let Some(url) = admission_url else {
        return Err(ended(
            "its refusal named no admission endpoint to wait on".to_string(),
        ));
    };
    let words = waiting_words(reason.as_deref());
    tracing::warn!(admission = %url, "{words}");
    notify(EngineHoldEvent::Waiting { words });
    let answer = reqwest::Client::new()
        .get(url.as_str())
        .send()
        .await
        .map_err(|e| ended(format!("the engine went away while it held ({e})")))?;
    let status = answer.status();
    let text = answer.text().await.map_err(|e| {
        ended(format!(
            "{url} answered {status} and the answer broke off ({e})"
        ))
    })?;
    // A proxy between goose and the engine (the LeanZero Link relay: a peer that left mid-wait)
    // answers in words, not JSON; those words are the reason and are said as-is (Q-401).
    let admitted = serde_json::from_str::<Value>(&text)
        .is_ok_and(|body| body.get("admission_open") == Some(&Value::Bool(true)));
    if !status.is_success() || !admitted {
        return Err(ended(format!("{url} answered {status}: {text}")));
    }
    tracing::info!(admission = %url, "{ADMITTED_WORDS}");
    notify(EngineHoldEvent::Admitted {
        words: ADMITTED_WORDS.to_string(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "http://127.0.0.1:8091/v1/chat/completions";

    fn refusal(error: Value) -> Value {
        serde_json::json!({ "error": error })
    }

    #[test]
    fn a_503_naming_the_memory_hold_is_the_hold_and_names_where_to_wait() {
        let body = refusal(serde_json::json!({
            "message": "goose distributed engine is not admitting new requests: memory is low",
            "type": "server_busy",
            "code": "memory_hold",
            "reason": "memory is low; quitting other apps frees it",
            "admission": "/goose/admission",
        }));
        let hold = hold_of_refusal(StatusCode::SERVICE_UNAVAILABLE, Some(&body), URL, || {
            "503 words".to_string()
        })
        .expect("the code names the hold");
        assert_eq!(
            hold,
            ProviderError::EngineHold {
                details: "503 words".to_string(),
                reason: Some("memory is low; quitting other apps frees it".to_string()),
                admission_url: Some("http://127.0.0.1:8091/goose/admission".to_string()),
            }
        );
        assert!(!hold.is_transient(), "never blind-retried");
    }

    /// NEGATIVE CONTROLS: the words alone, another code, another status — none is the hold.
    #[test]
    fn only_the_code_on_a_503_names_the_hold() {
        let prose_only = refusal(serde_json::json!({
            "message": "goose distributed engine is not admitting new requests: the engine is holding new requests until memory recovers",
            "type": "server_busy",
        }));
        let other_code = refusal(serde_json::json!({"message": "busy", "code": "server_busy"}));
        let hold = refusal(serde_json::json!({"message": "m", "code": "memory_hold"}));
        let words = || "w".to_string();
        let unavailable = StatusCode::SERVICE_UNAVAILABLE;
        assert_eq!(
            hold_of_refusal(unavailable, Some(&prose_only), URL, words),
            None
        );
        assert_eq!(
            hold_of_refusal(unavailable, Some(&other_code), URL, words),
            None
        );
        assert_eq!(hold_of_refusal(unavailable, None, URL, words), None);
        assert_eq!(
            hold_of_refusal(StatusCode::INTERNAL_SERVER_ERROR, Some(&hold), URL, words),
            None
        );
    }

    #[tokio::test]
    async fn a_hold_that_names_no_endpoint_cannot_be_waited_out_and_says_so() {
        let hold = ProviderError::EngineHold {
            details: "503 words".to_string(),
            reason: None,
            admission_url: None,
        };
        let ended = wait_for_admission(&hold).await.unwrap_err();
        assert!(
            matches!(&ended, ProviderError::ServerError(text)
                if text.starts_with("503 words") && text.contains("named no admission endpoint")),
            "{ended:?}"
        );
    }

    #[test]
    fn the_turn_line_carries_the_engines_own_reason() {
        assert_eq!(
            waiting_words(Some("memory on X is low; quitting other apps frees it")),
            "Waiting: memory on X is low; quitting other apps frees it"
        );
        assert_eq!(
            waiting_words(None),
            "Waiting: the engine is holding new requests until memory recovers"
        );
    }
}
