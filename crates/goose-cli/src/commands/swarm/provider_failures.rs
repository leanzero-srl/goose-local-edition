//! PROVIDER FAILURES DISGUISED AS TEXT — the readers that tell a transport/agent error apart from
//! something the model said.
//!
//! agent.rs's provider-error arms yield the failure as the assistant's own TEXT and break the
//! stream, so `run_agent`/`run_agent_in` return `Ok(text)` for an HTTP 4xx/5xx, a dropped body or
//! a refusal. Every reader here keys on that deterministic shape (the closing sentence the agent
//! appends LAST) — never on model prose. Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases): the error-closer readers moved here
//! verbatim from swarm.rs when the Rapid-MLX admission-cap reader joined them.

use super::clip_tail;

/// The closing sentences of the assistant-authored ERROR texts in agent.rs's provider-error arms —
/// the refusal, the NetworkError arm, and the generic provider-error arm. These are the ONLY texts
/// that reach the answer channel without the model having said them, so "does `last_text` end with
/// one of these" is a deterministic test for "this is a transport/agent error, not the model's
/// answer". Matched as suffixes because `last_text` is a 400-char TAIL and each of these sentences
/// is what the agent appends LAST before breaking the stream.
pub(super) const AGENT_ERROR_CLOSERS: [&str; 3] = [
    "Please resend your message to try again.",
    "Please retry if you think this is a transient or recoverable error.",
    "resending this conversation is likely to be refused again.",
];

/// `said` when `last_text` is (the tail of) something the MODEL produced; `error` when it is one of
/// the agent's own provider-error texts. The distinction exists because r0's `ledger-core-tests`
/// showed attempt 0's "Network error: Stream decode error … Please resend your message" as the
/// lane's current answer for 24+ minutes while attempt 1 was running — the pane had no way to say
/// "this text is a dead attempt's transport error", because the digest never said which kind of
/// text it was carrying.
pub(super) fn said_kind_of(last_text: &str) -> &'static str {
    let t = last_text.trim_end();
    if AGENT_ERROR_CLOSERS.iter().any(|c| t.ends_with(c)) {
        "error"
    } else {
        "said"
    }
}

/// A-2: a supervision call's reply, with the transport-failure disguise removed. The agent loop
/// surfaces a provider failure as its own error-closer TEXT (agent.rs:2678 "Ran into this error:
/// {provider_err}…"), so `run_agent` returns `Ok` and the caller reads an HTTP 400 body as a model
/// reply. Every judge/review parser is deliberately LENIENT, which is exactly what makes it launder
/// that text: r2's parse read gabee's 400 "Invalid model identifier" as substantive-and-not-OK and
/// emitted 28 `drifting` verdicts whose hint WAS the error body, from 22:02:35Z until the run ended.
/// `Err` here is the whole error text (tail-clipped), for the caller's failed event.
pub(super) fn supervision_reply(text: &str) -> Result<&str, String> {
    if said_kind_of(text) == "error" {
        Err(clip_tail(text, 400))
    } else {
        Ok(text)
    }
}

/// #121: does this accumulated task output carry the deterministic mid-stream body-drop signature? The
/// provider surfaces a dropped HTTP body as ProviderError::NetworkError("Stream decode error: ...") which
/// the agent reply loop yields as assistant TEXT and then BREAKs — so the error sentence is guaranteed to be
/// the LAST chunk. Requiring BOTH the "Stream decode error" marker AND that exact trailing sentence makes
/// this ~zero-false-positive: a genuine verify verdict would have to literally contain the marker string and
/// end with the resend sentence.
pub(super) fn is_stream_decode_interrupt(text: &str) -> bool {
    text.contains("Stream decode error")
        && text
            .trim_end()
            .ends_with("Please resend your message to try again.")
}

/// Rapid-MLX's `--max-concurrent-requests` (the sidecar's serve arg, 8 today) is a HARD ADMISSION
/// CAP, not a queue — MEASURED by the MLX busy-signal agent, 2026-09-02: the 9th and 10th
/// concurrent requests were answered `503 "Server is busy (max concurrent requests reached)…
/// (currently 8 in-flight)"`. A swarm fan wider than the cap on one sidecar node therefore gets
/// 503s. The provider layer already retries a 503 against the SAME URL with its own exponential
/// backoff (goose-provider-types `retry::should_retry`: ServerError is transient;
/// `DEFAULT_MAX_RETRIES` looks, `delay_for_attempt`) — when those are exhausted the agent loop
/// yields "Ran into this error: Server error (503 …): HTTP 503: {body}. … Please retry if you
/// think this is a transient or recoverable error." and breaks, so the worker's "answer" is that
/// text and the owned-file gates would read it as a model that wrote nothing.
///
/// `Some(in_flight)` when `text` is that refusal — the agent's error closer AND the cap's own
/// words ("server is busy" + "max concurrent") — with the count parsed from "(currently N
/// in-flight)" (`None` inside when the body carried no count: absent, never invented). `None`
/// for anything the model said, including prose that mentions a busy server.
pub(super) fn sidecar_admission_cap_refusal(text: &str) -> Option<Option<u32>> {
    if said_kind_of(text) != "error" {
        return None;
    }
    let low = text.to_lowercase();
    if !(low.contains("server is busy") && low.contains("max concurrent")) {
        return None;
    }
    let in_flight = low
        .split("currently ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse::<u32>().ok());
    Some(in_flight)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact 503 the sidecar answers past its cap, as it reaches the dispatcher: the provider's
    /// ServerError display wrapped in the agent's generic error text. The JSON envelope around the
    /// measured message is assumed (rapid-mlx's error shape); the reader keys on the message and
    /// the closer, not on the envelope.
    const CAPPED: &str = "Ran into this error: Server error: Server error (503 Service Unavailable) at \
        http://127.0.0.1:8090/v1/chat/completions: HTTP 503: {\"error\":{\"message\":\"Server is busy \
        (max concurrent requests reached). Please try again later. (currently 8 in-flight)\",\
        \"type\":\"server_error\"}}.\n\nPlease retry if you think this is a transient or recoverable error.";

    #[test]
    fn the_sidecars_admission_cap_refusal_is_read_from_the_agents_error_text() {
        assert_eq!(sidecar_admission_cap_refusal(CAPPED), Some(Some(8)));
        assert_eq!(
            sidecar_admission_cap_refusal(&format!("{CAPPED}\n  ")),
            Some(Some(8)),
            "trailing whitespace is the tail's, not the closer's"
        );
        // Earlier assistant text before the failing call does not hide the closer.
        assert_eq!(
            sidecar_admission_cap_refusal(&format!("I will write app/store.py first.\n\n{CAPPED}")),
            Some(Some(8))
        );
        // The count is a fact from the body — absent, not invented.
        let uncounted = CAPPED.replace(" (currently 8 in-flight)", "");
        assert_eq!(sidecar_admission_cap_refusal(&uncounted), Some(None));
        // Model prose about a busy server is the model's answer, not a refusal.
        assert_eq!(
            sidecar_admission_cap_refusal(
                "The vendor answered 503 Server is busy (max concurrent requests reached); I added a retry."
            ),
            None
        );
        // A different provider error with the same closer is not the cap.
        assert_eq!(
            sidecar_admission_cap_refusal(
                "Ran into this error: Request failed with status 400 Bad Request: Invalid model \
                 identifier 'x'.\n\nPlease retry if you think this is a transient or recoverable error."
            ),
            None
        );
        // A dropped body is the stream-decode reader's, not this one's.
        assert_eq!(
            sidecar_admission_cap_refusal(
                "Network error: Stream decode error: x\n\nPlease resend your message to try again."
            ),
            None
        );
    }

    #[test]
    fn stream_decode_interrupt_predicate_is_deterministic() {
        // BOTH halves required: the marker AND the exact trailing resend sentence (the agent loop appends it
        // as the last chunk after a NetworkError, then breaks).
        let hit = "Network error: Stream decode error: error decoding response body\n\nPlease resend your message to try again.";
        assert!(is_stream_decode_interrupt(hit));
        // Trailing whitespace is tolerated (trim_end before the suffix check).
        assert!(is_stream_decode_interrupt(&format!("{hit}\n  ")));
        // Marker present but NOT the trailing sentence (e.g. a genuine verdict that merely quotes the phrase)
        // -> not an interrupt; the run's real output must be preserved.
        assert!(!is_stream_decode_interrupt(
            "The app logs a 'Stream decode error' when the socket closes; VERDICT: PASS."
        ));
        // The resend sentence WITHOUT the decode marker (some other transient) -> not this predicate.
        assert!(!is_stream_decode_interrupt(
            "Some other failure.\n\nPlease resend your message to try again."
        ));
        // A normal, clean verdict is never a false positive.
        assert!(!is_stream_decode_interrupt(
            "VERDICT: PASS — all endpoints return 200 and balances sum to zero."
        ));
    }
}
