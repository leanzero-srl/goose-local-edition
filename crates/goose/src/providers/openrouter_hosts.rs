//! Which upstream hosts OpenRouter runs a model on, how each one answers one tool-carrying call,
//! and the routing pin that keeps every OpenRouter request on one of them.
//!
//! The pin is `OPENROUTER_PARAMETERS` in config.yaml — the key the OpenRouter provider merges into
//! every request, and the key `sb72_chain.py set_pin` writes before each benchmark run. It is
//! written here byte-for-byte as that driver writes it, so either side reads the other's pin.

use std::time::Instant;

use anyhow::{bail, Result};
use goose_sdk_types::custom_requests::{
    OpenRouterHostDto, OpenRouterHostProbeResponse, OpenRouterHostsListResponse, OpenRouterPinDto,
};
use serde_json::{json, Value};

use super::api_client::ApiClient;
use super::openrouter::{parse_openrouter_parameters, OPENROUTER_PARAMETERS_CONFIG_KEY};
use crate::config::{Config, ConfigError};

/// The prompt a probe sends: enough generated text to time, then the one tool call the run needs
/// every host to make.
const PROBE_PROMPT: &str =
    "In two sentences, say why the sky is blue, then call the tool ping with x=1.";

/// The probe's answer budget — the size of the check the owner ran by hand on 2026-10-04, so a
/// reasoning model has room to think before it calls the tool (Parasail fp8 used all 3,000
/// without calling it; that is a finding, not a probe failure).
const PROBE_MAX_TOKENS: u64 = 3000;

/// The value `OPENROUTER_PARAMETERS` takes for a one-host pin, exactly as `set_pin` writes it
/// (`json.dumps(..., separators=(',', ':'))`, keys in this order).
pub fn pin_value(tag: &str) -> String {
    format!(
        r#"{{"provider":{{"order":[{}],"allow_fallbacks":false}}}}"#,
        Value::String(tag.to_string())
    )
}

/// The one host a pin keeps every request on: `provider.order` names exactly one tag and
/// `allow_fallbacks` is false. Any other routing is not a one-host pin.
fn single_host(params: &serde_json::Map<String, Value>) -> Option<String> {
    let provider = params.get("provider")?.as_object()?;
    if provider.get("allow_fallbacks") != Some(&Value::Bool(false)) {
        return None;
    }
    if provider
        .keys()
        .any(|key| key != "order" && key != "allow_fallbacks")
    {
        return None;
    }
    match provider.get("order")?.as_array()?.as_slice() {
        [Value::String(tag)] => Some(tag.clone()),
        _ => None,
    }
}

/// The pin config.yaml (or the environment, which the provider reads first) holds now.
pub fn read_pin(config: &Config) -> Result<OpenRouterPinDto> {
    let raw = match config.get_param::<Value>(OPENROUTER_PARAMETERS_CONFIG_KEY) {
        Ok(raw) => raw,
        Err(ConfigError::NotFound(_)) => return Ok(OpenRouterPinDto::default()),
        Err(err) => return Err(err.into()),
    };
    let params: serde_json::Map<String, Value> =
        parse_openrouter_parameters(raw)?.into_iter().collect();
    Ok(OpenRouterPinDto {
        raw: Some(Value::Object(params.clone()).to_string()),
        tag: single_host(&params),
    })
}

/// Pin every OpenRouter request to `tag` with no fallbacks, or — `None` — remove the key so
/// OpenRouter routes. Answers the pin as it reads back.
pub fn write_pin(config: &Config, tag: Option<&str>) -> Result<OpenRouterPinDto> {
    match tag.map(str::trim) {
        Some("") => bail!("a host pin needs the host's tag, e.g. `wafer`"),
        Some(tag) => config.set_param(OPENROUTER_PARAMETERS_CONFIG_KEY, pin_value(tag))?,
        None => config.delete(OPENROUTER_PARAMETERS_CONFIG_KEY)?,
    }
    read_pin(config)
}

/// `author/slug` → the endpoints listing's path. Only the characters OpenRouter model ids use are
/// accepted, so the id can never step out of `api/v1/models/`.
pub fn endpoints_path(model: &str) -> Result<String> {
    let segments: Vec<&str> = model.trim().split('/').collect();
    let valid = |segment: &&str| {
        !segment.is_empty()
            && *segment != "."
            && *segment != ".."
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._:~".contains(c))
    };
    if segments.len() != 2 || !segments.iter().all(valid) {
        bail!("`{model}` is not an OpenRouter model id (author/slug, e.g. qwen/qwen3.8-27b)");
    }
    Ok(format!(
        "api/v1/models/{}/{}/endpoints",
        segments[0], segments[1]
    ))
}

/// The endpoints listing's `data.endpoints` as hosts; endpoints with no tag cannot be pinned and
/// are named in `untagged`.
pub fn parse_endpoints(model: &str, json: &Value) -> Result<OpenRouterHostsListResponse> {
    let Some(endpoints) = json
        .get("data")
        .and_then(|data| data.get("endpoints"))
        .and_then(Value::as_array)
    else {
        bail!("OpenRouter's endpoints listing for {model} carries no data.endpoints: {json}");
    };
    let mut hosts = Vec::new();
    let mut untagged = Vec::new();
    for endpoint in endpoints {
        let provider_name = endpoint
            .get("provider_name")
            .and_then(Value::as_str)
            .unwrap_or("(unnamed host)")
            .to_string();
        let Some(tag) = endpoint
            .get("tag")
            .and_then(Value::as_str)
            .filter(|tag| !tag.is_empty())
        else {
            untagged.push(provider_name);
            continue;
        };
        hosts.push(OpenRouterHostDto {
            tag: tag.to_string(),
            provider_name,
            quantization: endpoint
                .get("quantization")
                .and_then(Value::as_str)
                .map(str::to_string),
            context_length: endpoint.get("context_length").and_then(Value::as_u64),
            supports_tools: endpoint
                .get("supported_parameters")
                .and_then(Value::as_array)
                .is_some_and(|params| params.iter().any(|p| p.as_str() == Some("tools"))),
            uptime_last_30m: endpoint.get("uptime_last_30m").and_then(Value::as_f64),
            status: endpoint.get("status").and_then(Value::as_i64),
        });
    }
    Ok(OpenRouterHostsListResponse {
        model: model.to_string(),
        hosts,
        untagged,
    })
}

pub async fn list_hosts(client: &ApiClient, model: &str) -> Result<OpenRouterHostsListResponse> {
    let response = client.response_get(&endpoints_path(model)?).await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        bail!(
            "OpenRouter answered {status} for {model}'s endpoints: {}",
            error_text(&body)
        );
    }
    parse_endpoints(model, &serde_json::from_str(&body)?)
}

/// One chat completion for `model` on exactly the host `tag`, carrying the `ping` tool.
pub fn probe_payload(model: &str, tag: &str) -> Value {
    json!({
        "model": model,
        "messages": [{"role": "user", "content": PROBE_PROMPT}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "ping",
                "description": "Answers pong. Call it with x=1.",
                "parameters": {
                    "type": "object",
                    "properties": {"x": {"type": "integer"}},
                    "required": ["x"]
                }
            }
        }],
        "max_tokens": PROBE_MAX_TOKENS,
        "stream": false,
        "provider": {"order": [tag], "allow_fallbacks": false}
    })
}

/// OpenRouter's error text from a body: `error.message`, plus `error.metadata.raw` (the host's own
/// words — "Provider returned error" alone names nothing); a body that is not that shape, verbatim.
fn error_text(body: &str) -> String {
    let Ok(json) = serde_json::from_str::<Value>(body) else {
        return body.trim().to_string();
    };
    error_in(&json).unwrap_or_else(|| body.trim().to_string())
}

fn error_in(json: &Value) -> Option<String> {
    let error = json.get("error")?;
    let Some(message) = error.get("message").and_then(Value::as_str) else {
        return Some(error.to_string());
    };
    match error
        .get("metadata")
        .and_then(|metadata| metadata.get("raw"))
    {
        Some(Value::String(raw)) if !raw.is_empty() => Some(format!("{message}: {raw}")),
        Some(raw) if !raw.is_null() => Some(format!("{message}: {raw}")),
        _ => Some(message.to_string()),
    }
}

/// What one probe measured, from the answer's status and body and the wall seconds it took.
pub fn probe_outcome(
    tag: &str,
    status: u16,
    body: &str,
    seconds: f64,
) -> OpenRouterHostProbeResponse {
    let mut outcome = OpenRouterHostProbeResponse {
        tag: tag.to_string(),
        seconds,
        http_status: Some(status),
        ..Default::default()
    };
    if !(200..300).contains(&status) {
        outcome.error = Some(error_text(body));
        return outcome;
    }
    let json: Value = match serde_json::from_str(body) {
        Ok(json) => json,
        Err(err) => {
            outcome.error = Some(format!("the answer is not JSON ({err}): {}", body.trim()));
            return outcome;
        }
    };
    if let Some(error) = error_in(&json) {
        outcome.error = Some(error);
        return outcome;
    }
    let choice = json.get("choices").and_then(|choices| choices.get(0));
    outcome.finish_reason = choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str)
        .map(str::to_string);
    outcome.tool_call = choice
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("tool_calls"))
        .and_then(Value::as_array)
        .is_some_and(|calls| {
            calls.iter().any(|call| {
                call.get("function")
                    .and_then(|function| function.get("name"))
                    .and_then(Value::as_str)
                    == Some("ping")
            })
        });
    if let Some(error) = choice.and_then(error_in) {
        outcome.error = Some(error);
    }
    outcome.completion_tokens = json
        .get("usage")
        .and_then(|usage| usage.get("completion_tokens"))
        .and_then(Value::as_u64);
    outcome.tokens_per_second = outcome
        .completion_tokens
        .filter(|_| seconds > 0.0)
        .map(|tokens| tokens as f64 / seconds);
    outcome
}

/// Send the probe to `tag` and measure it. Never an `Err`: a host that fails answers its own error
/// in the outcome, so one host's failure never hides another's result.
pub async fn probe_host(client: &ApiClient, model: &str, tag: &str) -> OpenRouterHostProbeResponse {
    let started = Instant::now();
    let sent = client
        .response_post("api/v1/chat/completions", &probe_payload(model, tag))
        .await;
    let response = match sent {
        Ok(response) => response,
        Err(err) => {
            return OpenRouterHostProbeResponse {
                tag: tag.to_string(),
                seconds: started.elapsed().as_secs_f64(),
                error: Some(format!("the request did not complete: {err}")),
                ..Default::default()
            }
        }
    };
    let status = response.status().as_u16();
    match response.text().await {
        Ok(body) => probe_outcome(tag, status, &body, started.elapsed().as_secs_f64()),
        Err(err) => OpenRouterHostProbeResponse {
            tag: tag.to_string(),
            seconds: started.elapsed().as_secs_f64(),
            http_status: Some(status),
            error: Some(format!("the answer could not be read: {err}")),
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::api_client::AuthMethod;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// The shape of OpenRouter's listing for qwen/qwen3.8-27b on 2026-10-04, trimmed to four hosts.
    fn listing() -> Value {
        json!({"data": {"id": "qwen/qwen3.8-27b", "endpoints": [
            {"provider_name": "Wafer", "tag": "wafer", "quantization": "fp8", "context_length": 262144,
             "supported_parameters": ["tools", "tool_choice", "max_tokens"], "uptime_last_30m": 99.2, "status": 0},
            {"provider_name": "DeepInfra", "tag": "deepinfra/bf16", "quantization": "bf16", "context_length": 131072,
             "supported_parameters": ["max_tokens"], "uptime_last_30m": null, "status": -2},
            {"provider_name": "Alibaba", "tag": "alibaba", "quantization": null, "context_length": 262144,
             "supported_parameters": ["tools"]},
            {"provider_name": "Mystery", "quantization": "fp4", "supported_parameters": ["tools"]}
        ]}})
    }

    #[test]
    fn the_listing_reads_every_field_the_picker_shows() {
        let hosts = parse_endpoints("qwen/qwen3.8-27b", &listing()).unwrap();
        assert_eq!(
            hosts.hosts[0],
            OpenRouterHostDto {
                tag: "wafer".into(),
                provider_name: "Wafer".into(),
                quantization: Some("fp8".into()),
                context_length: Some(262144),
                supports_tools: true,
                uptime_last_30m: Some(99.2),
                status: Some(0),
            }
        );
        assert_eq!(hosts.hosts[1].tag, "deepinfra/bf16");
        assert!(!hosts.hosts[1].supports_tools);
        assert_eq!(hosts.hosts[1].uptime_last_30m, None);
        assert_eq!(hosts.hosts[2].quantization, None);
        assert_eq!(hosts.hosts.len(), 3);
        assert_eq!(hosts.untagged, vec!["Mystery".to_string()]);
    }

    #[test]
    fn a_listing_without_endpoints_is_an_error_naming_the_body() {
        let err = parse_endpoints("a/b", &json!({"error": {"message": "Model not found"}}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("Model not found"), "{err}");
    }

    #[test]
    fn a_model_id_is_author_slash_slug_and_nothing_else() {
        assert_eq!(
            endpoints_path(" qwen/qwen3.8-27b ").unwrap(),
            "api/v1/models/qwen/qwen3.8-27b/endpoints"
        );
        assert_eq!(
            endpoints_path("qwen/qwen3-coder:free").unwrap(),
            "api/v1/models/qwen/qwen3-coder:free/endpoints"
        );
        for bad in [
            "qwen", "a/b/c", "../x", "a/..", "a/b?x=1", "a/b#c", "/b", "a b/c",
        ] {
            assert!(endpoints_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_pin_is_written_byte_for_byte_as_the_benchmark_driver_writes_it() {
        assert_eq!(
            pin_value("wafer"),
            r#"{"provider":{"order":["wafer"],"allow_fallbacks":false}}"#
        );
        assert_eq!(
            pin_value("deepinfra/bf16"),
            r#"{"provider":{"order":["deepinfra/bf16"],"allow_fallbacks":false}}"#
        );
    }

    fn temp_config(dir: &tempfile::TempDir, yaml: &str) -> Config {
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, yaml).unwrap();
        Config::new_with_file_secrets(&path, dir.path().join("secrets.yaml")).unwrap()
    }

    #[test]
    fn writing_a_pin_lands_the_drivers_exact_line_and_reads_back_as_that_host() {
        let dir = tempfile::tempdir().unwrap();
        let config = temp_config(&dir, "OPENROUTER_HOST: https://openrouter.ai\n");
        // The env var outranks the file; the test must read the file.
        let _env = env_lock::lock_env([(OPENROUTER_PARAMETERS_CONFIG_KEY, None::<&str>)]);

        let pin = write_pin(&config, Some("wafer")).unwrap();
        assert_eq!(pin.tag.as_deref(), Some("wafer"));
        let text = std::fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(
            text.lines().any(|line| line
                == r#"OPENROUTER_PARAMETERS: '{"provider":{"order":["wafer"],"allow_fallbacks":false}}'"#),
            "{text}"
        );
        assert!(
            text.contains("OPENROUTER_HOST: https://openrouter.ai"),
            "{text}"
        );

        let cleared = write_pin(&config, None).unwrap();
        assert_eq!(cleared, OpenRouterPinDto::default());
        let text = std::fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(!text.contains("OPENROUTER_PARAMETERS"), "{text}");
        assert!(write_pin(&config, Some("  ")).is_err());
    }

    #[test]
    fn a_pin_the_driver_wrote_reads_as_its_host_and_other_routing_reads_as_raw() {
        let dir = tempfile::tempdir().unwrap();
        let _env = env_lock::lock_env([(OPENROUTER_PARAMETERS_CONFIG_KEY, None::<&str>)]);
        let driver = temp_config(
            &dir,
            "OPENROUTER_PARAMETERS: '{\"provider\":{\"order\":[\"novita\"],\"allow_fallbacks\":false}}'\n",
        );
        assert_eq!(read_pin(&driver).unwrap().tag.as_deref(), Some("novita"));

        let other = temp_config(
            &dir,
            "OPENROUTER_PARAMETERS: '{\"provider\":{\"order\":[\"deepseek\"],\"ignore\":[\"relace\"]}}'\n",
        );
        let pin = read_pin(&other).unwrap();
        assert_eq!(pin.tag, None);
        assert!(pin.raw.unwrap().contains("relace"));

        let mapping = temp_config(
            &dir,
            "OPENROUTER_PARAMETERS:\n  provider:\n    order: [wafer]\n    allow_fallbacks: false\n",
        );
        assert_eq!(read_pin(&mapping).unwrap().tag.as_deref(), Some("wafer"));

        let fallbacks = temp_config(
            &dir,
            "OPENROUTER_PARAMETERS: '{\"provider\":{\"order\":[\"wafer\"]}}'\n",
        );
        assert_eq!(read_pin(&fallbacks).unwrap().tag, None);

        let none = temp_config(&dir, "GOOSE_PROVIDER: openrouter\n");
        assert_eq!(read_pin(&none).unwrap(), OpenRouterPinDto::default());
    }

    fn completion(tool: bool, finish: &str, tokens: u64) -> String {
        let message = if tool {
            json!({"role": "assistant", "content": null, "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "ping", "arguments": "{\"x\":1}"}}
            ]})
        } else {
            json!({"role": "assistant", "content": "The sky scatters blue light."})
        };
        json!({"choices": [{"message": message, "finish_reason": finish}],
               "usage": {"prompt_tokens": 40, "completion_tokens": tokens}})
        .to_string()
    }

    #[test]
    fn an_answer_with_the_tool_call_reads_speed_and_finish() {
        let outcome = probe_outcome("wafer", 200, &completion(true, "tool_calls", 794), 10.0);
        assert!(outcome.tool_call);
        assert_eq!(outcome.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(outcome.completion_tokens, Some(794));
        assert_eq!(outcome.tokens_per_second, Some(79.4));
        assert_eq!(outcome.error, None);
        assert_eq!(outcome.http_status, Some(200));
    }

    #[test]
    fn an_answer_that_spent_the_budget_without_the_tool_is_no_tool_call_not_an_error() {
        let outcome = probe_outcome(
            "parasail/fp8",
            200,
            &completion(false, "length", 3000),
            33.67,
        );
        assert!(!outcome.tool_call);
        assert_eq!(outcome.finish_reason.as_deref(), Some("length"));
        assert_eq!(outcome.error, None);
        assert!((outcome.tokens_per_second.unwrap() - 89.1).abs() < 0.01);
    }

    #[test]
    fn a_refusal_carries_the_hosts_words_verbatim() {
        let age = json!({"error": {"code": 403, "message":
            "This model requires 18+ age confirmation. Visit https://openrouter.ai/settings"}})
        .to_string();
        let outcome = probe_outcome("chutes", 403, &age, 0.4);
        assert_eq!(outcome.http_status, Some(403));
        assert_eq!(
            outcome.error.as_deref(),
            Some("This model requires 18+ age confirmation. Visit https://openrouter.ai/settings")
        );
        assert!(!outcome.tool_call);
        assert_eq!(outcome.tokens_per_second, None);

        let upstream = json!({"error": {"code": 400, "message": "Provider returned error",
            "metadata": {"raw": "{\"code\":\"InvalidParameter\",\"message\":\"tools not supported\"}",
                         "provider_name": "Alibaba"}}})
        .to_string();
        let outcome = probe_outcome("alibaba", 400, &upstream, 1.2);
        assert_eq!(
            outcome.error.as_deref(),
            Some(
                r#"Provider returned error: {"code":"InvalidParameter","message":"tools not supported"}"#
            )
        );

        let outcome = probe_outcome("x", 429, "Too Many Requests\n", 0.1);
        assert_eq!(outcome.error.as_deref(), Some("Too Many Requests"));

        let in_band = json!({"error": {"message": "upstream timeout", "code": 502}}).to_string();
        let outcome = probe_outcome("x", 200, &in_band, 5.0);
        assert_eq!(outcome.error.as_deref(), Some("upstream timeout"));
    }

    fn client(server: &MockServer) -> ApiClient {
        ApiClient::new_with_tls(
            server.uri(),
            AuthMethod::BearerToken("or-test".into()),
            None,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn the_probe_pins_its_host_with_no_fallbacks_and_offers_one_ping_tool() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/chat/completions"))
            .and(header("Authorization", "Bearer or-test"))
            .and(body_partial_json(json!({
                "model": "qwen/qwen3.8-27b",
                "provider": {"order": ["wafer"], "allow_fallbacks": false},
                "max_tokens": 3000,
                "stream": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_string(completion(
                true,
                "tool_calls",
                120,
            )))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/chat/completions"))
            .and(body_partial_json(json!({"provider": {"order": ["phala"]}})))
            .respond_with(
                ResponseTemplate::new(429).set_body_json(
                    json!({"error": {"code": 429, "message": "Rate limit exceeded"}}),
                ),
            )
            .mount(&server)
            .await;

        let client = client(&server);
        let (wafer, phala) = tokio::join!(
            probe_host(&client, "qwen/qwen3.8-27b", "wafer"),
            probe_host(&client, "qwen/qwen3.8-27b", "phala"),
        );
        assert!(wafer.tool_call, "{wafer:?}");
        assert_eq!(wafer.completion_tokens, Some(120));
        assert!(wafer.tokens_per_second.unwrap() > 0.0);
        assert_eq!(phala.error.as_deref(), Some("Rate limit exceeded"));
        assert_eq!(phala.http_status, Some(429));

        let sent: Value =
            serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
        let tools = sent["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "ping");
        assert!(sent["messages"][0]["content"]
            .as_str()
            .unwrap()
            .ends_with("then call the tool ping with x=1."));
    }

    #[tokio::test]
    async fn an_unreachable_host_answers_its_failure_instead_of_an_error() {
        let client =
            ApiClient::new_with_tls("http://127.0.0.1:9".into(), AuthMethod::NoAuth, None).unwrap();
        let outcome = probe_host(&client, "a/b", "wafer").await;
        assert!(outcome
            .error
            .unwrap()
            .starts_with("the request did not complete"));
        assert_eq!(outcome.http_status, None);
    }

    #[tokio::test]
    async fn the_hosts_come_from_the_models_endpoints_listing() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models/qwen/qwen3.8-27b/endpoints"))
            .and(header("Authorization", "Bearer or-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(listing()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models/nobody/nothing/endpoints"))
            .respond_with(ResponseTemplate::new(404).set_body_json(
                json!({"error": {"code": 404, "message": "No model nobody/nothing"}}),
            ))
            .mount(&server)
            .await;
        let client = client(&server);
        let hosts = list_hosts(&client, "qwen/qwen3.8-27b").await.unwrap();
        assert_eq!(hosts.hosts.len(), 3);
        let err = list_hosts(&client, "nobody/nothing")
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("404") && err.contains("No model nobody/nothing"),
            "{err}"
        );
    }
}
