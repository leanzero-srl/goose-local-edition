//! I-33: a REMOTE provider stream that goes silent mid-answer is ended as a dropped stream, so the
//! reply loop's transient resend (`agents/transient_resend.rs`) sends the call again.
//!
//! Measured 2026-10-19 (Studio, `openrouter-cloud-1f53cbfa`, elorian/glimpse over OpenRouter): call
//! 88 streamed 134 thinking deltas in 2 s, the last one "/in" at 05:16:40Z, then nothing — no
//! finish chunk, no close. goose waited at 0 % CPU for 6 h 22 min until the run was cancelled by
//! hand.
//!
//! Why this is not a wall clock on model work (AGENTS.md "NO CAPS", `api_client.rs`):
//! - It never applies to a LOCAL host (loopback, private, link-local, CGNAT/Tailscale, `.local`,
//!   `.lan`, single-label names). A local engine is silent for minutes on purpose: a 100k-token
//!   prefill (9-15 min measured), a queued parallel slot (581 s, r2), a tool call whose arguments
//!   Rapid-MLX sends only at the end of the stream. None of those reach this guard.
//! - It arms only after the answer has STARTED (the first message that carries content), so a
//!   remote model that thinks silently before its first token is never cut.
//! - It is progress-based: every message the provider yields restarts the window. Only a stream
//!   that produced content and then produced nothing at all for the whole window is ended.
//!
//! Tunable: `GOOSE_STREAM_STALL_SECONDS` (unset = 600, `0` = off).

use std::time::Duration;

use futures::StreamExt;
use reqwest::Url;
use url::Host;

use crate::base::MessageStream;
use crate::conversation::message::Message;
use crate::errors::ProviderError;

pub const STREAM_STALL_ENV: &str = "GOOSE_STREAM_STALL_SECONDS";
pub const DEFAULT_STREAM_STALL: Duration = Duration::from_secs(600);
/// The words a stalled stream's error carries (it rides `stream_decode_error`, the transient class).
pub const STREAM_STALLED: &str = "stream stalled";

/// The window from `GOOSE_STREAM_STALL_SECONDS`: unset or unreadable = the default, `0` = off.
pub fn configured_window(value: Option<&str>) -> Option<Duration> {
    match value.map(str::trim).map(str::parse::<u64>) {
        Some(Ok(0)) => None,
        Some(Ok(seconds)) => Some(Duration::from_secs(seconds)),
        _ => Some(DEFAULT_STREAM_STALL),
    }
}

/// A host whose silence is never judged: a model on this Mac, the LAN or the tailnet.
pub fn is_local_host(url: &Url) -> bool {
    match url.host() {
        None => true,
        Some(Host::Ipv4(ip)) => {
            let [a, b, ..] = ip.octets();
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || (a == 100 && (64..128).contains(&b))
        }
        Some(Host::Ipv6(ip)) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return is_local_host(&Url::parse(&format!("http://{v4}/")).expect("an ipv4 url"));
            }
            let first = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
        Some(Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            !name.contains('.')
                || [
                    ".localhost",
                    ".local",
                    ".lan",
                    ".internal",
                    ".home.arpa",
                    ".ts.net",
                ]
                .iter()
                .any(|suffix| name.ends_with(suffix))
        }
    }
}

/// The stall window for a stream from `url`, or `None` when its silence is never judged.
pub fn stall_window(url: &Url, configured: Option<Duration>) -> Option<Duration> {
    if is_local_host(url) {
        None
    } else {
        configured
    }
}

fn carries_content(message: &Message) -> bool {
    message
        .content
        .iter()
        .any(|content| content.as_text().is_none_or(|text| !text.is_empty()))
}

/// Guards a provider's stream from `url` with the configured window.
pub fn guard(stream: MessageStream, url: &Url) -> MessageStream {
    let configured = configured_window(std::env::var(STREAM_STALL_ENV).ok().as_deref());
    match stall_window(url, configured) {
        Some(window) => guard_with(stream, window, url.host_str().unwrap_or("?").to_string()),
        None => stream,
    }
}

/// Ends `stream` with a transient stream error once it has yielded content and then yields
/// nothing for `window`.
pub fn guard_with(mut stream: MessageStream, window: Duration, host: String) -> MessageStream {
    Box::pin(async_stream::stream! {
        let mut started = false;
        loop {
            let next = if started {
                match tokio::time::timeout(window, stream.next()).await {
                    Ok(next) => next,
                    Err(_) => {
                        tracing::warn!(
                            host = %host,
                            window_secs = window.as_secs(),
                            "provider stream stalled mid-answer; ending it so the call is sent again"
                        );
                        yield Err(ProviderError::stream_decode_error(format!(
                            "{STREAM_STALLED}: {host} sent nothing for {}s after the answer started",
                            window.as_secs()
                        )));
                        break;
                    }
                }
            } else {
                stream.next().await
            };
            let Some(item) = next else { break };
            if let Ok((Some(message), _)) = &item {
                started |= carries_content(message);
            }
            yield item;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::message::Message;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn the_window_reads_the_env_value() {
        assert_eq!(configured_window(None), Some(DEFAULT_STREAM_STALL));
        assert_eq!(configured_window(Some("0")), None);
        assert_eq!(
            configured_window(Some(" 90 ")),
            Some(Duration::from_secs(90))
        );
        assert_eq!(configured_window(Some("soon")), Some(DEFAULT_STREAM_STALL));
    }

    #[test]
    fn local_hosts_are_never_judged() {
        let window = Some(Duration::from_secs(5));
        for local in [
            "http://127.0.0.1:8090/v1/chat/completions",
            "http://localhost:1234/v1",
            "http://[::1]:8090/v1",
            "http://192.168.10.161:1234/v1",
            "http://10.0.0.5/v1",
            "http://172.20.1.1/v1",
            "http://169.254.3.4/v1",
            "http://100.122.51.13:8090/v1",
            "http://WorksMacStudio.lan:1234/v1",
            "http://workhorse:8090/v1",
            "http://gabee.tail1234.ts.net/v1",
            "http://studio.local/v1",
            "http://[fd7a:115c:a1e0::1]/v1",
            "http://[::ffff:127.0.0.1]/v1",
        ] {
            assert_eq!(stall_window(&url(local), window), None, "{local}");
        }
        for remote in [
            "https://openrouter.ai/api/v1/chat/completions",
            "https://api.anthropic.com/v1/messages",
            "https://generativelanguage.googleapis.com/v1beta",
            "http://8.8.8.8/v1",
            "http://100.200.1.1/v1",
        ] {
            assert_eq!(stall_window(&url(remote), window), window, "{remote}");
        }
    }

    fn delta(
        text: &str,
    ) -> Result<
        (
            Option<Message>,
            Option<crate::conversation::token_usage::ProviderUsage>,
        ),
        ProviderError,
    > {
        Ok((Some(Message::assistant().with_text(text)), None))
    }

    /// The Glimpse shape: deltas, then the stream neither yields nor closes.
    fn stalls_after(deltas: Vec<&'static str>) -> MessageStream {
        Box::pin(
            futures::stream::iter(deltas.into_iter().map(delta)).chain(futures::stream::pending()),
        )
    }

    #[tokio::test(start_paused = true)]
    async fn a_stream_that_stops_mid_answer_ends_as_a_transient_error() {
        let mut guarded = guard_with(
            stalls_after(vec!["The", " plan", "/in"]),
            Duration::from_secs(600),
            "openrouter.ai".into(),
        );
        for _ in 0..3 {
            assert!(guarded.next().await.unwrap().is_ok());
        }
        let error = guarded.next().await.unwrap().unwrap_err();
        assert!(error.to_string().contains(STREAM_STALLED), "{error}");
        assert!(
            error.is_transient(),
            "{error:?} must be resent by the reply loop"
        );
        assert!(guarded.next().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn silence_before_the_answer_starts_is_never_cut() {
        let quiet_then_answer: MessageStream = Box::pin(async_stream::stream! {
            yield Ok((Some(Message::assistant().with_text("")), None));
            tokio::time::sleep(Duration::from_secs(3_600)).await;
            yield delta("finally");
        });
        let mut guarded = guard_with(
            quiet_then_answer,
            Duration::from_secs(600),
            "openrouter.ai".into(),
        );
        assert!(guarded.next().await.unwrap().is_ok());
        let item = guarded.next().await.unwrap().unwrap();
        assert_eq!(item.0.unwrap().as_concat_text(), "finally");
        assert!(guarded.next().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_answer_that_keeps_producing_is_never_cut() {
        let slow: MessageStream = Box::pin(async_stream::stream! {
            for word in ["a", "b", "c", "d"] {
                tokio::time::sleep(Duration::from_secs(590)).await;
                yield delta(word);
            }
        });
        let mut guarded = guard_with(slow, Duration::from_secs(600), "openrouter.ai".into());
        let mut words = 0;
        while let Some(item) = guarded.next().await {
            item.unwrap();
            words += 1;
        }
        assert_eq!(words, 4);
    }

    #[tokio::test(start_paused = true)]
    async fn a_local_engine_is_never_guarded() {
        let guarded = guard(
            stalls_after(vec!["x"]),
            &url("http://127.0.0.1:8090/v1/chat/completions"),
        );
        let mut guarded = guarded;
        assert!(guarded.next().await.unwrap().is_ok());
        let waited = tokio::time::timeout(Duration::from_secs(86_400), guarded.next()).await;
        assert!(
            waited.is_err(),
            "a local engine's silence must stay unjudged"
        );
    }
}
