//! Per-model custom fields: the request knobs one model takes, shown as controls when the model is
//! chosen, saved per provider+model, and sent on that model's requests.
//!
//! Where the fields come from:
//! - OpenRouter publishes per-model metadata (`/api/v1/models`: `supported_parameters` and a
//!   `reasoning` block with `supported_efforts`/`default_effort`/`mandatory`), so its fields are
//!   read from the listing — a model gets exactly the knobs the listing declares for it.
//! - Every other provider gets only the effort control goose's own provider code already maps
//!   (`thinking_effort`, translated per provider), and only for a model goose knows reasons.
//!
//! Saved values live in config under [`MODEL_FIELDS_CONFIG_KEY`] as
//! `{provider: {model: {field: value}}}` — the raw field values, not wire keys, so the store reads
//! the same as the controls. [`request_params`] turns them into the provider's request params.

use crate::config::{Config, ConfigError};
use anyhow::{anyhow, Result};
use goose_providers::base::DeclaredModelParameters;
use goose_providers::model::ModelConfig;
use goose_providers::thinking::ThinkingEffort;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

pub const MODEL_FIELDS_CONFIG_KEY: &str = "GOOSE_MODEL_FIELDS";
pub const EFFORT_FIELD: &str = "effort";

const OPENROUTER: &str = crate::providers::openrouter::OPENROUTER_PROVIDER_NAME;

/// OpenRouter's documented `reasoning.effort` values, offered only when the listing declares no
/// `supported_efforts` for the model (openrouter.ai/docs/use-cases/reasoning-tokens, read
/// 2026-10-04: "max", "xhigh", "high", "medium", "low", "minimal", "none").
const OPENROUTER_EFFORTS: &[&str] = &["max", "xhigh", "high", "medium", "low", "minimal", "none"];

/// OpenRouter's documented `verbosity` values (openrouter.ai/docs/api-reference/parameters, read
/// 2026-10-04: "low, medium, high, xhigh, max").
const OPENROUTER_VERBOSITY: &[&str] = &["low", "medium", "high", "xhigh", "max"];

struct NumberSpec {
    id: &'static str,
    label: &'static str,
    min: Option<f64>,
    max: Option<f64>,
    integer: bool,
}

/// The sampling parameters OpenRouter documents with their ranges
/// (openrouter.ai/docs/api-reference/parameters, read 2026-10-04).
const OPENROUTER_NUMBERS: &[NumberSpec] = &[
    NumberSpec {
        id: "temperature",
        label: "Temperature",
        min: Some(0.0),
        max: Some(2.0),
        integer: false,
    },
    NumberSpec {
        id: "top_p",
        label: "Top P",
        min: Some(0.0),
        max: Some(1.0),
        integer: false,
    },
    NumberSpec {
        id: "top_k",
        label: "Top K",
        min: Some(0.0),
        max: None,
        integer: true,
    },
    NumberSpec {
        id: "min_p",
        label: "Min P",
        min: Some(0.0),
        max: Some(1.0),
        integer: false,
    },
    NumberSpec {
        id: "top_a",
        label: "Top A",
        min: Some(0.0),
        max: Some(1.0),
        integer: false,
    },
    NumberSpec {
        id: "frequency_penalty",
        label: "Frequency penalty",
        min: Some(-2.0),
        max: Some(2.0),
        integer: false,
    },
    NumberSpec {
        id: "presence_penalty",
        label: "Presence penalty",
        min: Some(-2.0),
        max: Some(2.0),
        integer: false,
    },
    NumberSpec {
        id: "repetition_penalty",
        label: "Repetition penalty",
        min: Some(0.0),
        max: Some(2.0),
        integer: false,
    },
];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum FieldKind {
    Select {
        options: Vec<String>,
    },
    Number {
        min: Option<f64>,
        max: Option<f64>,
        integer: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelField {
    pub id: String,
    pub label: String,
    pub description: String,
    pub kind: FieldKind,
    /// The model's own value when a request sends none, where the metadata declares it.
    pub model_default: Option<Value>,
}

/// Where a model's fields were read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FieldSource {
    /// The provider's own per-model metadata (OpenRouter's models listing).
    ProviderMetadata,
    /// The provider publishes per-model metadata, but its listing does not carry this model id.
    UnlistedModel,
    /// goose's own effort mapping for a model goose knows reasons.
    GooseEffort,
    /// No field applies to this model.
    None,
}

pub type FieldValues = BTreeMap<String, Value>;
type Store = BTreeMap<String, BTreeMap<String, FieldValues>>;

/// True for a provider whose fields come from its own listing rather than goose's mapping.
pub fn reads_listing(provider: &str) -> bool {
    provider == OPENROUTER
}

fn select(id: &str, label: &str, description: String, options: Vec<String>) -> ModelField {
    ModelField {
        id: id.to_string(),
        label: label.to_string(),
        description,
        kind: FieldKind::Select { options },
        model_default: None,
    }
}

/// The fields an OpenRouter listing entry declares: `reasoning` → Effort (the model's own
/// `supported_efforts` when listed, else OpenRouter's documented values, without `none` when
/// reasoning is mandatory); `verbosity` → Verbosity; each documented sampling parameter it lists →
/// a number field. Nothing the entry does not list is offered.
pub fn openrouter_fields(declared: &DeclaredModelParameters) -> Vec<ModelField> {
    let supports = |name: &str| declared.supported_parameters.iter().any(|p| p == name);
    let mut fields = Vec::new();
    if supports("reasoning") {
        let (options, description) = if declared.effort_levels.is_empty() {
            (
                OPENROUTER_EFFORTS
                    .iter()
                    .filter(|level| !(declared.reasoning_mandatory && **level == "none"))
                    .map(|level| level.to_string())
                    .collect(),
                "Sent as reasoning.effort. OpenRouter lists no effort levels for this model, so \
                 these are OpenRouter's documented levels; OpenRouter maps them onto the model's \
                 reasoning budget."
                    .to_string(),
            )
        } else {
            (
                declared.effort_levels.clone(),
                "Sent as reasoning.effort. The levels are the ones OpenRouter lists for this model."
                    .to_string(),
            )
        };
        let mut effort = select(EFFORT_FIELD, "Effort", description, options);
        effort.model_default = declared.default_effort.clone().map(Value::String);
        fields.push(effort);
    }
    if supports("verbosity") {
        fields.push(select(
            "verbosity",
            "Verbosity",
            "Sent as verbosity: how long the model's answers run.".to_string(),
            OPENROUTER_VERBOSITY.iter().map(|v| v.to_string()).collect(),
        ));
    }
    for spec in OPENROUTER_NUMBERS {
        if !supports(spec.id) {
            continue;
        }
        fields.push(ModelField {
            id: spec.id.to_string(),
            label: spec.label.to_string(),
            description: format!("Sent as {}.", spec.id),
            kind: FieldKind::Number {
                min: spec.min,
                max: spec.max,
                integer: spec.integer,
            },
            model_default: declared.default_parameters.get(spec.id).cloned(),
        });
    }
    fields
}

/// goose's own effort control: the five levels every provider's code maps (Anthropic thinking,
/// OpenAI reasoning_effort, Gemini thinking level, …), offered only for a model goose knows
/// reasons — the same predicate the chat's thinking-effort option uses.
pub fn goose_effort_field(model: &ModelConfig) -> Option<ModelField> {
    if !model.is_reasoning_model() {
        return None;
    }
    let levels = [
        ThinkingEffort::Off,
        ThinkingEffort::Low,
        ThinkingEffort::Medium,
        ThinkingEffort::High,
        ThinkingEffort::Max,
    ];
    Some(select(
        EFFORT_FIELD,
        "Effort",
        "goose's thinking effort, which this provider's code translates into its own reasoning \
         setting."
            .to_string(),
        levels.iter().map(ToString::to_string).collect(),
    ))
}

/// The fields for `model` on `provider`. `declared` is the provider's own metadata for the model,
/// looked up only for a provider that [`reads_listing`].
pub fn fields_for(
    provider: &str,
    declared: Option<&DeclaredModelParameters>,
    model: &ModelConfig,
) -> (Vec<ModelField>, FieldSource) {
    if reads_listing(provider) {
        return match declared {
            Some(declared) => {
                let fields = openrouter_fields(declared);
                let source = if fields.is_empty() {
                    FieldSource::None
                } else {
                    FieldSource::ProviderMetadata
                };
                (fields, source)
            }
            None => (Vec::new(), FieldSource::UnlistedModel),
        };
    }
    match goose_effort_field(model) {
        Some(field) => (vec![field], FieldSource::GooseEffort),
        None => (Vec::new(), FieldSource::None),
    }
}

fn number_of(value: &Value) -> Option<f64> {
    value.as_f64()
}

/// The values to save: nulls dropped (a null clears a field back to the model's default), every
/// other value checked against the field the model declares. Errors name the field and why.
pub fn validate(fields: &[ModelField], values: &FieldValues) -> Result<FieldValues> {
    let mut clean = FieldValues::new();
    for (id, value) in values {
        if value.is_null() {
            continue;
        }
        let field = fields
            .iter()
            .find(|field| &field.id == id)
            .ok_or_else(|| anyhow!("'{id}' is not a field this model declares"))?;
        match &field.kind {
            FieldKind::Select { options } => {
                let chosen = value.as_str().ok_or_else(|| {
                    anyhow!("{} must be one of: {}", field.label, options.join(", "))
                })?;
                if !options.iter().any(|option| option == chosen) {
                    return Err(anyhow!(
                        "{} '{chosen}' is not one of: {}",
                        field.label,
                        options.join(", ")
                    ));
                }
            }
            FieldKind::Number { min, max, integer } => {
                let number =
                    number_of(value).ok_or_else(|| anyhow!("{} must be a number", field.label))?;
                if *integer && number.fract() != 0.0 {
                    return Err(anyhow!("{} must be a whole number", field.label));
                }
                if min.is_some_and(|min| number < min) || max.is_some_and(|max| number > max) {
                    return Err(anyhow!(
                        "{} {number} is outside {}..{}",
                        field.label,
                        min.map_or("".to_string(), |m| m.to_string()),
                        max.map_or("".to_string(), |m| m.to_string())
                    ));
                }
            }
        }
        clean.insert(id.clone(), value.clone());
    }
    Ok(clean)
}

fn read_store(config: &Config) -> Result<Store> {
    match config.get_param::<Store>(MODEL_FIELDS_CONFIG_KEY) {
        Ok(store) => Ok(store),
        Err(ConfigError::NotFound(_)) => Ok(Store::new()),
        Err(e) => Err(anyhow!(
            "{MODEL_FIELDS_CONFIG_KEY} could not be read, so the per-model fields are unknown: {e}"
        )),
    }
}

/// The values saved for `model` on `provider`; empty when none are saved.
pub fn saved_values_in(config: &Config, provider: &str, model: &str) -> Result<FieldValues> {
    Ok(read_store(config)?
        .get(provider)
        .and_then(|models| models.get(model))
        .cloned()
        .unwrap_or_default())
}

pub fn saved_values(provider: &str, model: &str) -> Result<FieldValues> {
    saved_values_in(Config::global(), provider, model)
}

/// Replaces the saved values for `model` on `provider`; an empty set removes the model's entry
/// (and the provider's, once it holds no model).
pub fn save_values_in(
    config: &Config,
    provider: &str,
    model: &str,
    values: FieldValues,
) -> Result<()> {
    let mut store = read_store(config)?;
    if values.is_empty() {
        if let Some(models) = store.get_mut(provider) {
            models.remove(model);
            if models.is_empty() {
                store.remove(provider);
            }
        }
    } else {
        store
            .entry(provider.to_string())
            .or_default()
            .insert(model.to_string(), values);
    }
    if store.is_empty() {
        match config.delete(MODEL_FIELDS_CONFIG_KEY) {
            Ok(()) | Err(ConfigError::NotFound(_)) => Ok(()),
            Err(e) => Err(e.into()),
        }
    } else {
        Ok(config.set_param(MODEL_FIELDS_CONFIG_KEY, store)?)
    }
}

pub fn save_values(provider: &str, model: &str, values: FieldValues) -> Result<()> {
    save_values_in(Config::global(), provider, model, values)
}

/// The request params the saved values become on `provider`'s wire: Effort is OpenRouter's
/// `reasoning: {effort}` there and goose's `thinking_effort` (mapped by the provider's code)
/// everywhere else; every other field rides under its own name, which the OpenAI-format request
/// copies into the body.
pub fn request_params(provider: &str, values: &FieldValues) -> HashMap<String, Value> {
    values
        .iter()
        .map(|(id, value)| match id.as_str() {
            EFFORT_FIELD if reads_listing(provider) => {
                ("reasoning".to_string(), json!({ "effort": value }))
            }
            EFFORT_FIELD => ("thinking_effort".to_string(), value.clone()),
            _ => (id.clone(), value.clone()),
        })
        .collect()
}

/// Adds the saved values under every param the model config does not already set — a request
/// param chosen for the session wins over the per-model default.
pub fn with_saved_values(mut model: ModelConfig, provider: &str) -> Result<ModelConfig> {
    let values = saved_values(provider, &model.model_name)?;
    if values.is_empty() {
        return Ok(model);
    }
    let params = model.request_params.get_or_insert_with(HashMap::new);
    for (key, value) in request_params(provider, &values) {
        params.entry(key).or_insert(value);
    }
    Ok(model)
}

/// Sets the saved values over whatever the config carries: a routed call's config is the
/// session's (the `swarm` model's), so its params were never chosen for this node's model.
pub fn apply_saved_values_over(model: &mut ModelConfig, provider: &str) -> Result<()> {
    let values = saved_values(provider, &model.model_name)?;
    if values.is_empty() {
        return Ok(());
    }
    model
        .request_params
        .get_or_insert_with(HashMap::new)
        .extend(request_params(provider, &values));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn declared(params: &[&str]) -> DeclaredModelParameters {
        DeclaredModelParameters {
            supported_parameters: params.iter().map(|p| p.to_string()).collect(),
            ..Default::default()
        }
    }

    fn ids(fields: &[ModelField]) -> Vec<&str> {
        fields.iter().map(|f| f.id.as_str()).collect()
    }

    fn options(field: &ModelField) -> Vec<&str> {
        match &field.kind {
            FieldKind::Select { options } => options.iter().map(String::as_str).collect(),
            FieldKind::Number { .. } => panic!("{} is a number field", field.id),
        }
    }

    #[test]
    fn a_listed_model_gets_exactly_the_knobs_its_listing_declares() {
        let mut meta = declared(&[
            "reasoning",
            "include_reasoning",
            "tools",
            "top_k",
            "temperature",
            "max_tokens",
        ]);
        meta.effort_levels = vec!["max".into(), "high".into(), "low".into()];
        meta.default_effort = Some("high".into());
        meta.default_parameters = BTreeMap::from([("temperature".to_string(), json!(1))]);

        let fields = openrouter_fields(&meta);

        assert_eq!(ids(&fields), vec!["effort", "temperature", "top_k"]);
        assert_eq!(options(&fields[0]), vec!["max", "high", "low"]);
        assert_eq!(fields[0].model_default, Some(json!("high")));
        assert_eq!(fields[1].model_default, Some(json!(1)));
        assert_eq!(
            fields[2].kind,
            FieldKind::Number {
                min: Some(0.0),
                max: None,
                integer: true
            }
        );
    }

    #[test]
    fn an_undeclared_effort_list_offers_openrouters_documented_levels() {
        let fields = openrouter_fields(&declared(&["reasoning"]));
        assert_eq!(
            options(&fields[0]),
            vec!["max", "xhigh", "high", "medium", "low", "minimal", "none"]
        );

        let mut mandatory = declared(&["reasoning"]);
        mandatory.reasoning_mandatory = true;
        let fields = openrouter_fields(&mandatory);
        assert!(!options(&fields[0]).contains(&"none"));
    }

    #[test]
    fn verbosity_rides_only_where_listed() {
        assert_eq!(
            ids(&openrouter_fields(&declared(&["verbosity"]))),
            vec!["verbosity"]
        );
        assert!(openrouter_fields(&declared(&["tools", "seed"])).is_empty());
    }

    #[test]
    fn openrouter_reads_the_listing_and_an_unlisted_model_has_no_fields() {
        let model = ModelConfig::new("vendor/typo");
        let (fields, source) = fields_for(OPENROUTER, None, &model);
        assert!(fields.is_empty());
        assert_eq!(source, FieldSource::UnlistedModel);

        let (fields, source) = fields_for(OPENROUTER, Some(&declared(&["reasoning"])), &model);
        assert_eq!(ids(&fields), vec!["effort"]);
        assert_eq!(source, FieldSource::ProviderMetadata);
    }

    #[test]
    fn other_providers_get_only_gooses_effort_and_only_for_a_reasoning_model() {
        let mut reasoning = ModelConfig::new("some-model");
        reasoning.reasoning = Some(true);
        let (fields, source) = fields_for("anthropic", None, &reasoning);
        assert_eq!(source, FieldSource::GooseEffort);
        assert_eq!(
            options(&fields[0]),
            vec!["off", "low", "medium", "high", "max"]
        );

        let mut plain = ModelConfig::new("some-model");
        plain.reasoning = Some(false);
        let (fields, source) = fields_for("anthropic", None, &plain);
        assert!(fields.is_empty());
        assert_eq!(source, FieldSource::None);
    }

    #[test]
    fn validation_refuses_what_the_model_does_not_declare() {
        let mut meta = declared(&["reasoning", "top_k", "temperature"]);
        meta.effort_levels = vec!["high".into(), "low".into()];
        let fields = openrouter_fields(&meta);

        let ok = validate(
            &fields,
            &FieldValues::from([
                ("effort".to_string(), json!("low")),
                ("top_k".to_string(), json!(40)),
                ("temperature".to_string(), Value::Null),
            ]),
        )
        .unwrap();
        assert_eq!(
            ok,
            FieldValues::from([
                ("effort".to_string(), json!("low")),
                ("top_k".to_string(), json!(40)),
            ])
        );

        for (values, needle) in [
            (vec![("effort", json!("medium"))], "not one of"),
            (vec![("effort", json!(3))], "must be one of"),
            (vec![("top_k", json!(4.5))], "whole number"),
            (vec![("temperature", json!(2.5))], "outside"),
            (vec![("verbosity", json!("low"))], "not a field"),
        ] {
            let values = values
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect();
            let err = validate(&fields, &values).unwrap_err().to_string();
            assert!(err.contains(needle), "{err}");
        }
    }

    #[test]
    fn effort_reaches_each_providers_wire_under_its_own_key() {
        let values = FieldValues::from([
            ("effort".to_string(), json!("xhigh")),
            ("verbosity".to_string(), json!("low")),
        ]);
        let openrouter = request_params(OPENROUTER, &values);
        assert_eq!(openrouter["reasoning"], json!({"effort": "xhigh"}));
        assert_eq!(openrouter["verbosity"], json!("low"));
        assert!(!openrouter.contains_key("thinking_effort"));

        let anthropic = request_params(
            "anthropic",
            &FieldValues::from([("effort".to_string(), json!("high"))]),
        );
        assert_eq!(anthropic["thinking_effort"], json!("high"));
        assert!(!anthropic.contains_key("reasoning"));
    }

    fn test_config() -> (Config, NamedTempFile, NamedTempFile) {
        let config_file = NamedTempFile::new().unwrap();
        let secrets_file = NamedTempFile::new().unwrap();
        let config =
            Config::new_with_file_secrets(config_file.path(), secrets_file.path()).unwrap();
        (config, config_file, secrets_file)
    }

    #[test]
    fn values_persist_per_provider_and_model_and_an_empty_set_removes_the_entry() {
        let (config, file, _secrets) = test_config();
        let model = "deepseek/deepseek-v4.1-flash";
        assert!(saved_values_in(&config, OPENROUTER, model)
            .unwrap()
            .is_empty());

        save_values_in(
            &config,
            OPENROUTER,
            model,
            FieldValues::from([("effort".to_string(), json!("low"))]),
        )
        .unwrap();
        save_values_in(
            &config,
            "anthropic",
            "claude-x",
            FieldValues::from([("effort".to_string(), json!("high"))]),
        )
        .unwrap();

        assert_eq!(
            saved_values_in(&config, OPENROUTER, model).unwrap(),
            FieldValues::from([("effort".to_string(), json!("low"))])
        );
        assert!(saved_values_in(&config, OPENROUTER, "other/model")
            .unwrap()
            .is_empty());
        let yaml = std::fs::read_to_string(file.path()).unwrap();
        assert!(yaml.contains(MODEL_FIELDS_CONFIG_KEY), "{yaml}");

        save_values_in(&config, OPENROUTER, model, FieldValues::new()).unwrap();
        save_values_in(&config, "anthropic", "claude-x", FieldValues::new()).unwrap();
        assert!(saved_values_in(&config, OPENROUTER, model)
            .unwrap()
            .is_empty());
        let yaml = std::fs::read_to_string(file.path()).unwrap();
        assert!(!yaml.contains(MODEL_FIELDS_CONFIG_KEY), "{yaml}");
    }

    #[test]
    fn an_unreadable_store_is_an_error_not_an_empty_set() {
        let (config, file, _secrets) = test_config();
        std::fs::write(file.path(), format!("{MODEL_FIELDS_CONFIG_KEY}: 7\n")).unwrap();
        let err = saved_values_in(&config, OPENROUTER, "m").unwrap_err();
        assert!(err.to_string().contains("could not be read"), "{err}");
    }
}
