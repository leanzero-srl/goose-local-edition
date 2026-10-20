//! I-33: a REMOTE provider stream that goes silent mid-answer is ended as a dropped stream, so the
//! reply loop's transient resend (`agents/transient_resend.rs`) sends the call again.
//!
//! Measured 2026-10-19 (Studio, `openrouter-cloud-1f53cbfa`, elorian/glimpse over OpenRouter): call
//! 88 streamed 134 thinking deltas in 2 s, the last one "/in" at 05:16:40Z, then nothing — no
//! finish chunk, no close. goose waited at 0 % CPU for 6 h 22 min until the run was cancelled by
//! hand.
//!
//! Why this is not a wall clock on model work (AGENTS.md "NO CAPS", `api_client.rs`):
//! - It never applies to a LOCAL host or peer (loopback, private, link-local, ULA, CGNAT/Tailscale,
//!   `.local`, `.lan`, single-label names — by the URL's host AND by the connection's peer address,
//!   so a self-hosted engine behind a public DNS name is local too). A local engine is silent for
//!   minutes on purpose: a 100k-token prefill (9-15 min measured), a queued parallel slot (581 s,
//!   r2), a tool call whose arguments Rapid-MLX sends only at the end of the stream.
//! - It arms only after the answer has STARTED (the first message that carries content), so a
//!   remote model that thinks silently before its first token is never cut.
//! - It is progress-based: every byte of a non-comment line restarts the window — tool-call
//!   argument deltas the parsers accumulate without yielding a message count (review of
//!   2026-10-19: arguments every 0.9 s were cut by a 2 s window). An SSE comment
//!   (`: OPENROUTER PROCESSING`) is not progress: a relay can send those forever over a dead
//!   upstream.
//!
//! Tunable: `GOOSE_STREAM_STALL_SECONDS` (unset = 600, `0` = off).

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use reqwest::{Response, Url};
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

/// An address on this Mac, the LAN or the tailnet.
pub fn is_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, ..] = ip.octets();
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || (a == 100 && (64..128).contains(&b))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return is_local_ip(IpAddr::V4(v4));
            }
            let first = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
    }
}

/// A host whose silence is never judged: a model on this Mac, the LAN or the tailnet.
pub fn is_local_host(url: &Url) -> bool {
    match url.host() {
        None => true,
        Some(Host::Ipv4(ip)) => is_local_ip(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => is_local_ip(IpAddr::V6(ip)),
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

/// The stall window for a stream from `url` over a connection to `peer`, or `None` when its
/// silence is never judged.
pub fn stall_window(
    url: &Url,
    peer: Option<SocketAddr>,
    configured: Option<Duration>,
) -> Option<Duration> {
    if is_local_host(url) || peer.is_some_and(|peer| is_local_ip(peer.ip())) {
        None
    } else {
        configured
    }
}

/// One stream's stall rule: its window (`None` = never judged), its host for the words, and the
/// count of data-line chunks its body has delivered.
#[derive(Clone)]
pub struct StallWatch {
    window: Option<Duration>,
    host: String,
    lines: Arc<AtomicU64>,
}

impl StallWatch {
    /// The rule for `response`, decided by its URL, its connection's peer address and
    /// `GOOSE_STREAM_STALL_SECONDS`. Call before the body is taken.
    pub fn of(response: &Response) -> Self {
        let configured = configured_window(std::env::var(STREAM_STALL_ENV).ok().as_deref());
        Self {
            window: stall_window(response.url(), response.remote_addr(), configured),
            host: response.url().host_str().unwrap_or("?").to_string(),
            lines: Arc::default(),
        }
    }

    /// A rule that judges with `window` whatever the host: tests, and callers that decided already.
    pub fn judging(window: Duration, host: &str) -> Self {
        Self {
            window: Some(window),
            host: host.to_string(),
            lines: Arc::default(),
        }
    }

    pub fn window(&self) -> Option<Duration> {
        self.window
    }

    /// Counts the body's progress: `inspect_ok` it onto the raw byte stream. A chunk carrying any
    /// byte of a non-comment, non-empty line is progress; an SSE comment line is not.
    pub fn data_lines<T: AsRef<[u8]>>(&self) -> impl FnMut(&T) + Send + 'static {
        let lines = self.lines.clone();
        let mut at_line_start = true;
        let mut in_comment = false;
        move |chunk: &T| {
            let mut progressed = false;
            for &byte in chunk.as_ref() {
                if at_line_start && byte != b'\n' && byte != b'\r' {
                    in_comment = byte == b':';
                }
                if byte == b'\n' {
                    at_line_start = true;
                    in_comment = false;
                    continue;
                }
                if byte != b'\r' {
                    at_line_start = false;
                    progressed |= !in_comment;
                }
            }
            if progressed {
                lines.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

fn carries_content(message: &Message) -> bool {
    message
        .content
        .iter()
        .any(|content| content.as_text().is_none_or(|text| !text.is_empty()))
}

/// Guards a provider's stream with its `StallWatch`: unchanged when the watch never judges.
pub fn guard(stream: MessageStream, watch: StallWatch) -> MessageStream {
    match watch.window {
        Some(window) => guard_with(stream, window, watch.host, watch.lines),
        None => stream,
    }
}

/// Ends `stream` with a transient stream error once it has yielded content and then, for a whole
/// `window`, neither yields a message nor delivers a data line (`lines` counts those).
pub fn guard_with(
    mut stream: MessageStream,
    window: Duration,
    host: String,
    lines: Arc<AtomicU64>,
) -> MessageStream {
    Box::pin(async_stream::stream! {
        let mut started = false;
        loop {
            let next = if started {
                loop {
                    let seen = lines.load(Ordering::Relaxed);
                    match tokio::time::timeout(window, stream.next()).await {
                        Ok(next) => break Some(next),
                        Err(_) if lines.load(Ordering::Relaxed) != seen => continue,
                        Err(_) => break None,
                    }
                }
            } else {
                Some(stream.next().await)
            };
            let Some(next) = next else {
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
    use crate::conversation::token_usage::ProviderUsage;

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
            assert_eq!(stall_window(&url(local), None, window), None, "{local}");
        }
        for remote in [
            "https://openrouter.ai/api/v1/chat/completions",
            "https://api.anthropic.com/v1/messages",
            "https://generativelanguage.googleapis.com/v1beta",
            "http://8.8.8.8/v1",
            "http://100.200.1.1/v1",
        ] {
            assert_eq!(stall_window(&url(remote), None, window), window, "{remote}");
        }
    }

    /// Review 2026-10-19: a self-hosted engine behind a public DNS name (or a global IPv6 on the
    /// LAN) is local by its connection's peer address.
    #[test]
    fn a_public_name_on_a_local_peer_is_never_judged() {
        let window = Some(Duration::from_secs(5));
        let public = url("https://models.example.com/v1/chat/completions");
        for peer in [
            "192.168.1.20:443",
            "127.0.0.1:8090",
            "100.101.1.2:443",
            "[fd00::5]:443",
            "[fe80::1]:443",
        ] {
            let peer: SocketAddr = peer.parse().unwrap();
            assert_eq!(stall_window(&public, Some(peer), window), None, "{peer}");
        }
        let remote: SocketAddr = "104.18.2.3:443".parse().unwrap();
        assert_eq!(stall_window(&public, Some(remote), window), window);
    }

    #[test]
    fn data_lines_count_every_chunk_but_comments() {
        let watch = StallWatch::judging(Duration::from_secs(1), "x");
        let mut count = watch.data_lines::<Vec<u8>>();
        let seen = |w: &StallWatch| w.lines.load(Ordering::Relaxed);
        count(&b": OPENROUTER PROCESSING\n\n".to_vec());
        assert_eq!(seen(&watch), 0, "an SSE comment is not progress");
        count(&b"data: {\"choices\":[{\"delta\":{\"tool_calls\"".to_vec());
        assert_eq!(seen(&watch), 1);
        count(&b":[]}}]}\n".to_vec());
        assert_eq!(
            seen(&watch),
            2,
            "the rest of a data line is progress, even if it starts with ':'"
        );
        count(&b"\r\n\n".to_vec());
        assert_eq!(seen(&watch), 2, "blank lines are not progress");
        count(&b": keepalive\r\ndata: x".to_vec());
        assert_eq!(seen(&watch), 3);
    }

    type Item = Result<(Option<Message>, Option<ProviderUsage>), ProviderError>;

    fn delta(text: &str) -> Item {
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
            Arc::default(),
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

    /// Review 2026-10-19: a parser yields nothing while it accumulates tool-call arguments, but the
    /// body is still delivering them — that is progress, not a stall.
    #[tokio::test(start_paused = true)]
    async fn data_lines_without_messages_keep_the_stream_alive() {
        let lines: Arc<AtomicU64> = Arc::default();
        let feeder = lines.clone();
        tokio::spawn(async move {
            for _ in 0..10 {
                tokio::time::sleep(Duration::from_millis(900)).await;
                feeder.fetch_add(1, Ordering::Relaxed);
            }
        });
        let mut guarded = guard_with(
            stalls_after(vec!["calling a tool"]),
            Duration::from_secs(2),
            "openrouter.ai".into(),
            lines,
        );
        assert!(guarded.next().await.unwrap().is_ok());
        let started = tokio::time::Instant::now();
        let error = guarded.next().await.unwrap().unwrap_err();
        assert!(error.to_string().contains(STREAM_STALLED), "{error}");
        // Ten lines 0.9 s apart keep it alive for 9 s; it stalls only once they stop.
        assert!(
            started.elapsed() >= Duration::from_secs(9),
            "{:?}",
            started.elapsed()
        );
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
            Arc::default(),
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
        let mut guarded = guard_with(
            slow,
            Duration::from_secs(600),
            "openrouter.ai".into(),
            Arc::default(),
        );
        let mut words = 0;
        while let Some(item) = guarded.next().await {
            item.unwrap();
            words += 1;
        }
        assert_eq!(words, 4);
    }
}
