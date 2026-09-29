//! The swarm config's load and save: the `swarm:` block of goose's config.yaml read OVER the struct
//! default (`load_config`, `merge_config_over_defaults`, `merge_json`), the one durable note of a
//! parse failure (`CONFIG_PARSE_ERROR`), and the CLI's save (`save_config`).
//!
//! Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). Extracted verbatim from swarm.rs with
//! its two merge tests; the save then became a read-modify-write that keeps what it does not own
//! (Q-207).

use anyhow::{anyhow, bail, Result};
use goose::config::{Config, ConfigError};
use serde::de::{self, DeserializeOwned};
use serde_yaml::{Mapping, Value as Yaml};

use super::{SwarmConfig, SwarmDevice, SWARM_CONFIG_KEY};

/// Deep-merge `over` INTO `base`: for objects, recurse key-by-key; a NULL in `over` means "leave the
/// base default" (never overwrite a real default with null); any non-object leaf in `over` replaces base.
fn merge_json(base: &mut serde_json::Value, over: serde_json::Value) {
    match (base, over) {
        (serde_json::Value::Object(b), serde_json::Value::Object(o)) => {
            for (k, v) in o {
                if v.is_null() {
                    continue;
                }
                merge_json(b.entry(k).or_insert(serde_json::Value::Null), v);
            }
        }
        (b, o) => *b = o,
    }
}

/// Load the swarm config with the STRUCT DEFAULT as the base, so a key OMITTED from config.yaml keeps the
/// intended default rather than serde's TYPE default (None/false).
///
/// `get_param::<SwarmConfig>()` deserializes the `swarm:` block directly, and serde fills a missing key with
/// the FIELD TYPE's default (None/false), NOT the `Default for SwarmConfig` value — so every non-type
/// default (e.g. `sink_max_turns: Some(120)`, the whole baked golden formula) was silently reverted whenever
/// a swarm block existed, which is always. Merging the raw config OVER `SwarmConfig::default()` fixes the
/// entire class at once, and makes the resolvers correct too (a `cfg.unwrap_or(false)` now sees the merged
/// `Some(golden)`). Falls back to the old typed read on any serialization hiccup, so it can only be safer.
/// GEN-6a #2 (fallback rule): the one durable note of a swarm-config parse failure. load_config
/// runs on hot paths with no event sink, so a broken config block used to silently run the
/// DEFAULTS while levers_resolved echoed those defaults as if chosen — the operator's yaml was
/// ignored and nothing said so. levers_resolved reads this note into the event.
pub(super) static CONFIG_PARSE_ERROR: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub(super) fn load_config() -> SwarmConfig {
    let cfg = Config::global();
    let raw = match cfg.get(SWARM_CONFIG_KEY, false) {
        Ok(v) => Some(v),
        // A missing key is a fresh install — defaults are the honest answer, silently.
        Err(ConfigError::NotFound(_)) => None,
        Err(e) => {
            // Any OTHER failure means the operator's config EXISTS but never reached the run.
            // MEASURED 2026-08-30 (run swarm-20260830-222740116): a duplicate top-level
            // `mlx_engine:` key failed the whole config-file parse, `.ok()` erased the evidence,
            // and the run silently used defaults — allow_model_load fell to off, the configured
            // sidecar device and planner vanished, and the red run was misdiagnosed as an engine
            // defect. Same OnceLock as the block-level arm in merge_config_over_defaults, so the
            // levers echo carries `config_parse_error` and the red banner names it.
            let _ = CONFIG_PARSE_ERROR.set(format!(
                "the goose config FILE failed to load ({e}) — the operator's swarm block never \
                 reached the run"
            ));
            None
        }
    };
    merge_config_over_defaults(raw, || {
        cfg.get_param::<SwarmConfig>(SWARM_CONFIG_KEY)
            .unwrap_or_default()
    })
}

/// The pure half of `load_config`, split out so the merge composition is PINNED by a test
/// (`an_omitted_key_keeps_the_baked_golden_through_the_real_merge`): repeat_break/omni_judge
/// stay armed for a yaml that lacks those keys ONLY because the operator's raw block is merged
/// OVER `serde_json::to_value(SwarmConfig::default())` here. A de-merge refactor — reading the
/// typed block directly, or basing the merge on serde's type defaults — silently reverts the
/// entire baked golden formula (every Some(...) default) the moment a swarm block exists, which
/// is always; the test fails it loudly instead.
fn merge_config_over_defaults(
    raw: Option<serde_json::Value>,
    typed_fallback: impl Fn() -> SwarmConfig,
) -> SwarmConfig {
    let Ok(mut base) = serde_json::to_value(SwarmConfig::default()) else {
        return typed_fallback();
    };
    if let Some(raw) = raw {
        merge_json(&mut base, raw);
    }
    serde_json::from_value(base).unwrap_or_else(|e| {
        let _ = CONFIG_PARSE_ERROR.set(format!(
            "swarm config block failed to deserialize ({e}) — the run is on DEFAULTS, not the \
             operator's config"
        ));
        typed_fallback()
    })
}

/// Q-207: the save used to write the typed struct over the whole `swarm:` block, so every key the
/// struct does not know — a field a newer goose or the desktop wrote, a device field from another
/// build, the operator's own — vanished on the next `goose swarm pool …` edit, and nothing said so.
/// The save is now a read-modify-write of the block on disk under the config's write lock: the typed
/// fields are written exactly as before (struct order, so a block holding only known keys saves
/// byte-identical to the old path) and every key the struct does not own is carried after them. A
/// field that cannot be placed refuses the save, names the field, and leaves the block as it was.
pub(super) fn save_config(cfg: &SwarmConfig) -> Result<()> {
    save_config_to(Config::global(), cfg)
}

fn save_config_to(config: &Config, cfg: &SwarmConfig) -> Result<()> {
    let typed =
        serde_yaml::to_value(cfg).map_err(|e| anyhow!("failed to serialize swarm config: {e}"))?;
    let mut refused = None;
    config
        .update_param::<Yaml, Yaml, _>(SWARM_CONFIG_KEY, |on_disk| {
            match keep_unowned_fields(&on_disk, typed) {
                Ok(merged) => merged,
                Err(e) => {
                    refused = Some(e);
                    on_disk
                }
            }
        })
        .map_err(|e| anyhow!("failed to save swarm config: {e}"))?;
    refused.map_or(Ok(()), Err)
}

fn keep_unowned_fields(on_disk: &Yaml, typed: Yaml) -> Result<Yaml> {
    // No block (a fresh install) or a block that is not a mapping has no fields to keep: the load
    // already ran on defaults and noted the unparseable block in CONFIG_PARSE_ERROR.
    let Some(on_disk) = block_mapping(on_disk) else {
        return Ok(typed);
    };
    let Yaml::Mapping(mut out) = typed else {
        bail!("the swarm config serialized to a non-mapping; the block on disk was left as it was");
    };
    let config_fields = struct_fields::<SwarmConfig>()?;
    for (key, value) in &on_disk {
        if !owns(config_fields, key) {
            out.insert(key.clone(), value.clone());
        }
    }
    carry_device_fields(&on_disk, &mut out)?;
    Ok(Yaml::Mapping(out))
}

/// The block as a mapping. `Config::get_param` also accepts the block as a JSON string, so a string
/// that parses to a mapping is that mapping.
fn block_mapping(on_disk: &Yaml) -> Option<Mapping> {
    match on_disk {
        Yaml::Mapping(m) => Some(m.clone()),
        Yaml::String(s) => match serde_yaml::from_str::<Yaml>(s) {
            Ok(Yaml::Mapping(m)) => Some(m),
            _ => None,
        },
        _ => None,
    }
}

/// A device's unknown fields ride on the saved device with the same `id`. A device the save removed
/// (`pool rm`) takes its fields with it — that removal is the operator's edit. An id that is missing
/// or not unique cannot be matched, so the save refuses rather than drop or misplace the fields.
fn carry_device_fields(on_disk: &Mapping, out: &mut Mapping) -> Result<()> {
    let Some(Yaml::Sequence(disk_devices)) = on_disk.get("devices") else {
        return Ok(());
    };
    let device_fields = struct_fields::<SwarmDevice>()?;
    for disk_device in disk_devices {
        let Yaml::Mapping(disk_device) = disk_device else {
            continue;
        };
        let extras: Vec<(&Yaml, &Yaml)> = disk_device
            .iter()
            .filter(|(k, _)| !owns(device_fields, k))
            .collect();
        if extras.is_empty() {
            continue;
        }
        let names = extras
            .iter()
            .map(|(k, _)| key_name(k))
            .collect::<Vec<_>>()
            .join(", ");
        let Some(id) = disk_device.get("id").and_then(Yaml::as_str) else {
            bail!(
                "refusing to save the swarm config: a device with no string `id` carries fields \
                 this goose does not own ({names}) and they cannot be matched to a saved device; \
                 the block on disk was left as it was"
            );
        };
        let same_id_on_disk = disk_devices
            .iter()
            .filter(|d| device_id(d) == Some(id))
            .count();
        let Some(Yaml::Sequence(saved)) = out.get_mut("devices") else {
            bail!(
                "refusing to save the swarm config: device `{id}` carries fields this goose does \
                 not own ({names}) but the saved block has no device list; the block on disk was \
                 left as it was"
            );
        };
        let mut matches: Vec<&mut Yaml> = saved
            .iter_mut()
            .filter(|d| device_id(d) == Some(id))
            .collect();
        if same_id_on_disk > 1 || matches.len() > 1 {
            bail!(
                "refusing to save the swarm config: device id `{id}` is not unique, so its fields \
                 this goose does not own ({names}) cannot be placed; the block on disk was left \
                 as it was"
            );
        }
        if let Some(Yaml::Mapping(saved_device)) = matches.pop() {
            for (k, v) in extras {
                saved_device.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(())
}

fn device_id(device: &Yaml) -> Option<&str> {
    device.get("id").and_then(Yaml::as_str)
}

fn owns(fields: &[&str], key: &Yaml) -> bool {
    key.as_str().is_some_and(|k| fields.contains(&k))
}

fn key_name(key: &Yaml) -> String {
    match key.as_str() {
        Some(k) => format!("`{k}`"),
        None => format!("{key:?}"),
    }
}

/// The keys serde reads `T` from, taken from the derive itself: a derived struct hands its field
/// list to `deserialize_struct`, so the probe records it and stops. A struct whose derive does not
/// go through `deserialize_struct` (a `#[serde(flatten)]` field) has no fixed list, and the save
/// refuses rather than guess which keys it owns.
fn struct_fields<T: DeserializeOwned>() -> Result<&'static [&'static str]> {
    struct FieldsProbe<'a>(&'a mut Option<&'static [&'static str]>);

    impl<'de> de::Deserializer<'de> for FieldsProbe<'_> {
        type Error = de::value::Error;

        fn deserialize_any<V: de::Visitor<'de>>(
            self,
            _visitor: V,
        ) -> Result<V::Value, Self::Error> {
            Err(de::Error::custom("not a struct"))
        }

        fn deserialize_struct<V: de::Visitor<'de>>(
            self,
            _name: &'static str,
            fields: &'static [&'static str],
            _visitor: V,
        ) -> Result<V::Value, Self::Error> {
            *self.0 = Some(fields);
            Err(de::Error::custom("fields recorded"))
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
            option unit unit_struct newtype_struct seq tuple tuple_struct map enum identifier
            ignored_any
        }
    }

    let mut fields = None;
    // The probe never yields a value: its only output is the field list it recorded.
    let _ = T::deserialize(FieldsProbe(&mut fields));
    fields.ok_or_else(|| {
        anyhow!(
            "refusing to save the swarm config: {} has no fixed field list, so the fields this \
             goose does not own cannot be told apart; the block on disk was left as it was",
            std::any::type_name::<T>()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE MERGE PIN (refactor hazard, works-prover). repeat_break/omni_judge stay armed for a
    /// yaml that lacks those keys ONLY because load_config merges the operator's raw block OVER
    /// SwarmConfig::default() — serde's type default for both is None, which every resolver
    /// reads as off. This traverses the REAL path (merge_config_over_defaults is load_config's
    /// body minus the global-config read), so a future de-merge refactor fails here loudly
    /// instead of silently disarming the golden formula on every configured machine.
    #[test]
    fn an_omitted_key_keeps_the_baked_golden_through_the_real_merge() {
        let never = || panic!("a parseable block must never fall back to the typed read");
        // An empty swarm block — the minimal configured machine.
        let merged = merge_config_over_defaults(Some(serde_json::json!({})), never);
        assert_eq!(merged.repeat_break, Some(true), "golden survives {{}}");
        assert_eq!(merged.omni_judge, Some(true), "golden survives {{}}");
        // A minimal operator yaml: one real key set, everything omitted stays golden.
        let merged = merge_config_over_defaults(
            Some(serde_json::json!({"planner_model": "operator-model"})),
            never,
        );
        assert_eq!(merged.planner_model, "operator-model");
        assert_eq!(merged.repeat_break, Some(true));
        assert_eq!(merged.omni_judge, Some(true));
        // merge_json's null rule: an explicit null leaves the baked default, never disarms it.
        let merged =
            merge_config_over_defaults(Some(serde_json::json!({"repeat_break": null})), never);
        assert_eq!(merged.repeat_break, Some(true));
        // No block at all (a fresh machine) is the struct default outright.
        let merged = merge_config_over_defaults(None, never);
        assert_eq!(merged.repeat_break, Some(true));
        assert_eq!(merged.omni_judge, Some(true));
    }

    /// load_config MERGES the config over the struct Default, so the baked golden formula survives an
    /// omitted key even though a bare #[serde(default)] on the field alone would not. This exercises the
    /// exact merge path load_config uses (to_value(default) -> merge partial -> from_value).
    #[test]
    fn the_merge_path_keeps_baked_golden_defaults_for_omitted_keys() {
        let mut base = serde_json::to_value(SwarmConfig::default()).unwrap();
        // A user config that sets ONE unrelated key and omits the whole golden bundle.
        let partial: serde_json::Value = serde_json::json!({ "persona": false });
        merge_json(&mut base, partial);
        let cfg: SwarmConfig = serde_json::from_value(base).unwrap();
        // The omitted golden levers keep their baked ON default (this is what a bare serde default lost).
        assert!(cfg.require_tests && cfg.author_pitfalls);
        assert_eq!(cfg.spec_wins, Some(true));
        assert_eq!(
            cfg.straggler_stop, None,
            "retired (r6e): no baked value to keep"
        );
        assert_eq!(cfg.parallel_tests, Some(true));
        assert_eq!(cfg.spiral_break_chars, Some(12000));
        assert_eq!(cfg.struct_stop, 80);
        assert_eq!(cfg.sink_max_turns, Some(120));
        // The explicitly-set key wins over the default, and a NULL would keep the default.
        assert!(
            !cfg.persona,
            "an explicit value overrides the baked default"
        );
    }

    fn config_at(dir: &std::path::Path, yaml: &str) -> (Config, std::path::PathBuf) {
        let path = dir.join("config.yaml");
        std::fs::write(&path, yaml).unwrap();
        let config = Config::new_with_file_secrets(&path, dir.join("secrets.yaml")).unwrap();
        (config, path)
    }

    fn on_disk(path: &std::path::Path) -> Yaml {
        serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn loaded(config: &Config) -> SwarmConfig {
        let raw = config.get(SWARM_CONFIG_KEY, false).unwrap();
        merge_config_over_defaults(Some(raw), || panic!("the block must parse"))
    }

    /// Q-207: a `pool` edit loads the typed config, changes it and saves it. The old save wrote the
    /// struct over the whole block, so `nodes` and the device's `label` were gone after this test's
    /// save. The edit itself must still land: planner changed, `d2` removed with its field, `d3`
    /// added bare.
    #[test]
    fn a_save_keeps_every_field_it_does_not_own() {
        let dir = tempfile::tempdir().unwrap();
        let (config, path) = config_at(
            dir.path(),
            "swarm:
  planner_model: planner-a
  nodes:
    studio:
      host: studio.lan
  devices:
  - id: d1
    model_id: m1
    weight: 2
    enabled: true
    label: kitchen
  - id: d2
    model_id: m2
    weight: 1
    enabled: true
    label: removed-with-its-device
unrelated_key: kept
",
        );
        let mut cfg = loaded(&config);
        cfg.planner_model = "planner-b".into();
        cfg.devices.retain(|d| d.id != "d2");
        let mut d3 = cfg.devices[0].clone();
        d3.id = "d3".into();
        cfg.devices.push(d3);

        save_config_to(&config, &cfg).unwrap();

        let saved = on_disk(&path);
        let swarm = &saved["swarm"];
        assert_eq!(
            swarm["nodes"]["studio"]["host"].as_str(),
            Some("studio.lan")
        );
        assert_eq!(swarm["planner_model"].as_str(), Some("planner-b"));
        let devices = swarm["devices"].as_sequence().unwrap();
        let ids: Vec<_> = devices.iter().filter_map(device_id).collect();
        assert_eq!(ids, ["d1", "d3"]);
        assert_eq!(devices[0]["label"].as_str(), Some("kitchen"));
        assert!(
            devices[1].get("label").is_none(),
            "a new device has no carried fields"
        );
        assert_eq!(saved["unrelated_key"].as_str(), Some("kept"));
        assert_eq!(loaded(&config).planner_model, "planner-b");
    }

    /// A block holding only fields the struct owns saves exactly as the old whole-struct write did.
    #[test]
    fn a_block_of_known_fields_saves_as_the_typed_struct() {
        let dir = tempfile::tempdir().unwrap();
        let (config, path) = config_at(dir.path(), "swarm:\n  planner_model: planner-a\n");
        let mut cfg = loaded(&config);
        cfg.planner_model = "planner-b".into();
        save_config_to(&config, &cfg).unwrap();
        assert_eq!(on_disk(&path)["swarm"], serde_yaml::to_value(&cfg).unwrap());

        let fresh = tempfile::tempdir().unwrap();
        let (config, path) = config_at(fresh.path(), "");
        save_config_to(&config, &cfg).unwrap();
        assert_eq!(on_disk(&path)["swarm"], serde_yaml::to_value(&cfg).unwrap());
    }

    /// Gate 1: a field that cannot be placed (its device id is not unique) refuses the save by name
    /// and leaves the block as it was — never a silent drop.
    #[test]
    fn a_field_that_cannot_be_placed_refuses_the_save() {
        let dir = tempfile::tempdir().unwrap();
        let (config, path) = config_at(
            dir.path(),
            "swarm:
  devices:
  - {id: d1, model_id: m1, weight: 1, enabled: true, label: first}
  - {id: d1, model_id: m2, weight: 1, enabled: true}
",
        );
        let before = on_disk(&path)["swarm"].clone();
        let mut cfg = loaded(&config);
        cfg.planner_model = "planner-b".into();
        let err = save_config_to(&config, &cfg).unwrap_err().to_string();
        assert!(err.contains("`label`") && err.contains("`d1`"), "{err}");
        assert_eq!(on_disk(&path)["swarm"], before);
    }

    #[test]
    fn the_owned_fields_are_read_from_the_derive() {
        let fields = struct_fields::<SwarmConfig>().unwrap();
        assert!(fields.contains(&"planner_model") && fields.contains(&"devices"));
        assert!(!fields.contains(&"nodes"));
        let fields = struct_fields::<SwarmDevice>().unwrap();
        assert!(fields.contains(&"id") && fields.contains(&"engine"));
    }
}
