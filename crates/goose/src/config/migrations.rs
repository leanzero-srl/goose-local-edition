use crate::agents::extension::PLATFORM_EXTENSIONS;
use crate::agents::platform_extensions::chatrecall;
use crate::agents::ExtensionConfig;
use crate::config::extensions::ExtensionEntry;
use crate::config::providers::ProviderEntry;
use serde_yaml::Mapping;

const EXTENSIONS_CONFIG_KEY: &str = "extensions";
const PROVIDERS_CONFIG_KEY: &str = "providers";
const ACTIVE_PROVIDER_KEY: &str = "active_provider";
/// Top-level list of the one-time migrations this config has been through. A migration that
/// changes a stored value runs once, so what the person sets afterwards stands.
const APPLIED_MIGRATIONS_KEY: &str = "applied_migrations";
const CHATRECALL_DEFAULT_ON: &str = "chatrecall_default_on";
/// What goose wrote for `chatrecall` while it was the off-by-default LIKE scan.
const CHATRECALL_OLD_DESCRIPTION: &str =
    "Search past conversations and load session summaries for contextual memory";
const CHATRECALL_OLD_DISPLAY_NAME: &str = "Chat Recall";

pub fn run_migrations(config: &mut Mapping) -> bool {
    let mut changed = false;
    changed |= migrate_chatrecall_default_on(config);
    changed |= migrate_platform_extensions(config);
    changed |= migrate_provider_config(config);
    changed
}

/// Run only non-destructive migrations suitable for in-memory read paths.
/// Provider migration is excluded because it removes flat keys that
/// `get_param()` callers may still look up directly. The chatrecall flip is included so a
/// config nobody has written to since the upgrade reads the same as it will once written.
pub fn run_read_migrations(config: &mut Mapping) {
    migrate_chatrecall_default_on(config);
    migrate_platform_extensions(config);
}

fn yaml_key(key: &str) -> serde_yaml::Value {
    serde_yaml::Value::String(key.to_string())
}

/// `None` when the marker holds something other than a list: no one-time migration can record
/// itself there, so none runs, rather than running again on every load.
fn applied_migrations(config: &Mapping) -> Option<Vec<serde_yaml::Value>> {
    match config.get(yaml_key(APPLIED_MIGRATIONS_KEY)) {
        None | Some(serde_yaml::Value::Null) => Some(Vec::new()),
        Some(serde_yaml::Value::Sequence(ids)) => Some(ids.clone()),
        Some(other) => {
            tracing::warn!(
                ?other,
                "config: `{APPLIED_MIGRATIONS_KEY}` is not a list; one-time migrations are skipped"
            );
            None
        }
    }
}

/// The stored `chatrecall` entry is exactly the old off-by-default platform entry, so its
/// `enabled: false` was goose's default and never the person's choice. `display_name`,
/// `bundled` and `available_tools` may be absent (older writers omitted them) or carry goose's
/// own values; any other key or value means the person edited the entry.
fn is_old_default_off_chatrecall(entry: &Mapping) -> bool {
    let known_keys = [
        "enabled",
        "type",
        "name",
        "description",
        "display_name",
        "bundled",
        "available_tools",
    ];
    let field = |key: &str| entry.get(yaml_key(key));
    let absent = |key: &str| matches!(field(key), None | Some(serde_yaml::Value::Null));
    let text = |key: &str| field(key).and_then(|value| value.as_str());

    entry
        .keys()
        .all(|key| key.as_str().is_some_and(|key| known_keys.contains(&key)))
        && field("enabled") == Some(&serde_yaml::Value::Bool(false))
        && text("type") == Some("platform")
        && text("name") == Some(chatrecall::EXTENSION_NAME)
        && text("description") == Some(CHATRECALL_OLD_DESCRIPTION)
        && (absent("display_name") || text("display_name") == Some(CHATRECALL_OLD_DISPLAY_NAME))
        && (absent("bundled") || field("bundled") == Some(&serde_yaml::Value::Bool(true)))
        && (absent("available_tools")
            || field("available_tools")
                .and_then(|value| value.as_sequence())
                .is_some_and(|tools| tools.is_empty()))
}

/// Q-364: chat search is on by default since it became the indexed transcript search, but the
/// old LIKE-scan `chatrecall` was off by default, every install stored `enabled: false`, and
/// `migrate_platform_extensions` keeps a stored `enabled`. Once per config the untouched old
/// default is flipped on. The marker is recorded whatever was found — no entry, an edited
/// entry, an enabled one — so the person turning chat search off afterwards stays off. Runs
/// before `migrate_platform_extensions`, which rewrites the old description it recognises.
fn migrate_chatrecall_default_on(config: &mut Mapping) -> bool {
    let Some(mut applied) = applied_migrations(config) else {
        return false;
    };
    if applied
        .iter()
        .any(|id| id.as_str() == Some(CHATRECALL_DEFAULT_ON))
    {
        return false;
    }

    if let Some(entry) = config
        .get_mut(yaml_key(EXTENSIONS_CONFIG_KEY))
        .and_then(|extensions| extensions.as_mapping_mut())
        .and_then(|extensions| extensions.get_mut(yaml_key(chatrecall::EXTENSION_NAME)))
        .and_then(|entry| entry.as_mapping_mut())
    {
        if is_old_default_off_chatrecall(entry) {
            entry.insert(yaml_key("enabled"), serde_yaml::Value::Bool(true));
        }
    }

    applied.push(yaml_key(CHATRECALL_DEFAULT_ON));
    config.insert(
        yaml_key(APPLIED_MIGRATIONS_KEY),
        serde_yaml::Value::Sequence(applied),
    );
    true
}

fn migrate_platform_extensions(config: &mut Mapping) -> bool {
    let extensions_key = serde_yaml::Value::String(EXTENSIONS_CONFIG_KEY.to_string());

    let extensions_value = config
        .get(&extensions_key)
        .cloned()
        .unwrap_or(serde_yaml::Value::Mapping(Mapping::new()));

    let mut extensions_map: Mapping = match extensions_value {
        serde_yaml::Value::Mapping(m) => m,
        _ => Mapping::new(),
    };

    let mut needs_save = false;

    for (name, def) in PLATFORM_EXTENSIONS.iter() {
        let ext_key = serde_yaml::Value::String(name.to_string());
        let existing = extensions_map.get(&ext_key);

        let needs_migration = match existing {
            None => true,
            Some(value) => match serde_yaml::from_value::<ExtensionEntry>(value.clone()) {
                Ok(entry) => match &entry.config {
                    ExtensionConfig::Platform {
                        description,
                        display_name,
                        ..
                    }
                    | ExtensionConfig::Builtin {
                        description,
                        display_name,
                        ..
                    } => {
                        description != def.description
                            || display_name.as_deref() != Some(def.display_name)
                    }
                    _ => true,
                },
                Err(_) => true,
            },
        };

        if needs_migration {
            let existing_entry =
                existing.and_then(|v| serde_yaml::from_value::<ExtensionEntry>(v.clone()).ok());

            let enabled = existing_entry
                .as_ref()
                .map(|e| e.enabled)
                .unwrap_or(def.default_enabled);

            // If the extension already exists as type 'builtin', preserve that type
            let is_existing_builtin = existing_entry
                .as_ref()
                .is_some_and(|e| matches!(e.config, ExtensionConfig::Builtin { .. }));

            let config = if is_existing_builtin {
                ExtensionConfig::Builtin {
                    name: def.name.to_string(),
                    description: def.description.to_string(),
                    display_name: Some(def.display_name.to_string()),
                    timeout: None,
                    bundled: Some(true),
                    available_tools: Vec::new(),
                }
            } else {
                ExtensionConfig::Platform {
                    name: def.name.to_string(),
                    description: def.description.to_string(),
                    display_name: Some(def.display_name.to_string()),
                    bundled: Some(true),
                    available_tools: Vec::new(),
                }
            };

            let new_entry = ExtensionEntry { config, enabled };

            if let Ok(value) = serde_yaml::to_value(&new_entry) {
                extensions_map.insert(ext_key, value);
                needs_save = true;
            }
        }
    }

    if needs_save {
        config.insert(extensions_key, serde_yaml::Value::Mapping(extensions_map));
    }

    needs_save
}

/// Remove leftover legacy flat keys when `providers:` block already exists.
fn cleanup_legacy_provider_keys(config: &mut Mapping) -> bool {
    let configured_suffix = "_configured";
    let mut changed = false;

    let stale_keys: Vec<serde_yaml::Value> = config
        .keys()
        .filter(|k| {
            k.as_str()
                .map(|s| {
                    s == "GOOSE_PROVIDER" || s == "GOOSE_MODEL" || s.ends_with(configured_suffix)
                })
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    for key in stale_keys {
        config.shift_remove(&key);
        changed = true;
    }

    changed
}

/// Migrate flat provider keys to the structured `providers:` block.
///
/// Old layout (flat keys):
/// ```yaml
/// GOOSE_PROVIDER: claude-acp
/// GOOSE_MODEL: current
/// claude-acp_configured: true
/// lmstudio_configured: true
/// ```
///
/// New layout:
/// ```yaml
/// active_provider: claude-acp
/// providers:
///   claude-acp:
///     enabled: true
///     model: current
///     configured: true
///   lmstudio:
///     enabled: true
///     model: ""
///     configured: true
/// ```
///
fn migrate_provider_config(config: &mut Mapping) -> bool {
    let providers_key = serde_yaml::Value::String(PROVIDERS_CONFIG_KEY.to_string());

    // If providers block already exists, backfill active_provider from the
    // legacy flat key when missing, then clean up leftover flat keys.
    if config.contains_key(&providers_key) {
        let ap_key = serde_yaml::Value::String(ACTIVE_PROVIDER_KEY.to_string());
        if !config.contains_key(&ap_key) {
            if let Some(legacy) = config
                .get(serde_yaml::Value::String("GOOSE_PROVIDER".to_string()))
                .and_then(|v| v.as_str())
            {
                config.insert(ap_key, serde_yaml::Value::String(legacy.to_string()));
            }
        }
        return cleanup_legacy_provider_keys(config);
    }

    // Read the old flat keys, if present.
    let active_provider = config
        .get(serde_yaml::Value::String("GOOSE_PROVIDER".to_string()))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let active_model = config
        .get(serde_yaml::Value::String("GOOSE_MODEL".to_string()))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();

    // Scan for `*_configured` keys to discover all previously-used providers.
    let configured_suffix = "_configured";
    let mut discovered_providers: Vec<String> = config
        .keys()
        .filter_map(|k| {
            k.as_str().and_then(|s| {
                if s.ends_with(configured_suffix) {
                    Some(s.trim_end_matches(configured_suffix).to_string())
                } else {
                    None
                }
            })
        })
        .collect();

    // Ensure the active provider is in the list even if no `*_configured`
    // marker exists for it yet.
    if let Some(ref ap) = active_provider {
        if !discovered_providers.contains(ap) {
            discovered_providers.push(ap.clone());
        }
    }

    // If there is nothing to migrate, bail out.
    if discovered_providers.is_empty() && active_provider.is_none() {
        return false;
    }

    // Build the providers mapping.
    let mut providers_map = Mapping::new();
    for name in &discovered_providers {
        let is_active = active_provider.as_deref() == Some(name.as_str());
        let model = if is_active {
            active_model.clone()
        } else {
            String::new()
        };
        let entry = ProviderEntry {
            enabled: true,
            model,
            configured: true,
        };
        if let Ok(value) = serde_yaml::to_value(&entry) {
            providers_map.insert(serde_yaml::Value::String(name.clone()), value);
        }
    }

    config.insert(providers_key, serde_yaml::Value::Mapping(providers_map));

    // Write `active_provider` top-level key.
    if let Some(ref ap) = active_provider {
        config.insert(
            serde_yaml::Value::String(ACTIVE_PROVIDER_KEY.to_string()),
            serde_yaml::Value::String(ap.clone()),
        );
    }

    // Remove old flat keys.
    config.shift_remove(serde_yaml::Value::String("GOOSE_PROVIDER".to_string()));
    config.shift_remove(serde_yaml::Value::String("GOOSE_MODEL".to_string()));
    for name in &discovered_providers {
        let marker_key = serde_yaml::Value::String(format!("{}{}", name, configured_suffix));
        config.shift_remove(&marker_key);
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrate_platform_extensions_empty_config() {
        let mut config = Mapping::new();
        let changed = run_migrations(&mut config);

        assert!(changed);
        let extensions_key = serde_yaml::Value::String(EXTENSIONS_CONFIG_KEY.to_string());
        assert!(config.contains_key(&extensions_key));
    }

    #[test]
    fn test_migrate_platform_extensions_preserves_enabled_state() {
        let mut config = Mapping::new();
        let mut extensions = Mapping::new();
        let todo_entry = ExtensionEntry {
            config: ExtensionConfig::Platform {
                name: "todo".to_string(),
                description: "old description".to_string(),
                display_name: Some("Old Name".to_string()),
                bundled: Some(true),
                available_tools: Vec::new(),
            },
            enabled: false,
        };
        extensions.insert(
            serde_yaml::Value::String("todo".to_string()),
            serde_yaml::to_value(&todo_entry).unwrap(),
        );
        config.insert(
            serde_yaml::Value::String(EXTENSIONS_CONFIG_KEY.to_string()),
            serde_yaml::Value::Mapping(extensions),
        );

        let changed = run_migrations(&mut config);
        assert!(changed);

        let extensions_key = serde_yaml::Value::String(EXTENSIONS_CONFIG_KEY.to_string());
        let extensions = config.get(&extensions_key).unwrap().as_mapping().unwrap();
        let todo_key = serde_yaml::Value::String("todo".to_string());
        let todo_value = extensions.get(&todo_key).unwrap();
        let todo_entry: ExtensionEntry = serde_yaml::from_value(todo_value.clone()).unwrap();

        assert!(!todo_entry.enabled);
    }

    #[test]
    fn test_migrate_platform_extensions_idempotent() {
        let mut config = Mapping::new();
        run_migrations(&mut config);

        let changed = run_migrations(&mut config);
        assert!(!changed);
    }

    /// The `chatrecall` block as it stands in the owner's ~/.config/goose/config.yaml today.
    const OLD_DEFAULT_OFF_CHATRECALL: &str = r#"
extensions:
  chatrecall:
    enabled: false
    type: platform
    name: chatrecall
    description: Search past conversations and load session summaries for contextual memory
    display_name: Chat Recall
    bundled: true
    available_tools: []
"#;

    fn chatrecall_entry(config: &Mapping) -> ExtensionEntry {
        let extensions = config
            .get(yaml_key(EXTENSIONS_CONFIG_KEY))
            .and_then(|v| v.as_mapping())
            .expect("extensions block");
        serde_yaml::from_value(
            extensions
                .get(yaml_key(chatrecall::EXTENSION_NAME))
                .expect("chatrecall entry")
                .clone(),
        )
        .expect("chatrecall entry parses")
    }

    fn set_chatrecall_enabled(config: &mut Mapping, enabled: bool) {
        config
            .get_mut(yaml_key(EXTENSIONS_CONFIG_KEY))
            .and_then(|v| v.as_mapping_mut())
            .and_then(|m| m.get_mut(yaml_key(chatrecall::EXTENSION_NAME)))
            .and_then(|v| v.as_mapping_mut())
            .expect("chatrecall entry")
            .insert(yaml_key("enabled"), serde_yaml::Value::Bool(enabled));
    }

    fn marker_recorded(config: &Mapping) -> bool {
        config
            .get(yaml_key(APPLIED_MIGRATIONS_KEY))
            .and_then(|v| v.as_sequence())
            .is_some_and(|ids| {
                ids.iter()
                    .any(|id| id.as_str() == Some(CHATRECALL_DEFAULT_ON))
            })
    }

    #[test]
    fn the_old_default_off_chatrecall_is_turned_on_once() {
        let mut config: Mapping = serde_yaml::from_str(OLD_DEFAULT_OFF_CHATRECALL).unwrap();

        assert!(run_migrations(&mut config));

        let entry = chatrecall_entry(&config);
        assert!(entry.enabled, "the stored old default must be flipped on");
        assert!(matches!(
            &entry.config,
            ExtensionConfig::Platform { description, display_name, .. }
                if description == PLATFORM_EXTENSIONS[chatrecall::EXTENSION_NAME].description
                    && display_name.as_deref() == Some("Chat Search")
        ));
        assert!(marker_recorded(&config));
    }

    #[test]
    fn chatrecall_turned_off_after_the_migration_stays_off() {
        let mut config: Mapping = serde_yaml::from_str(OLD_DEFAULT_OFF_CHATRECALL).unwrap();
        run_migrations(&mut config);
        assert!(chatrecall_entry(&config).enabled);

        set_chatrecall_enabled(&mut config, false);
        run_migrations(&mut config);
        run_read_migrations(&mut config);

        assert!(
            !chatrecall_entry(&config).enabled,
            "the person's own off must survive every later load"
        );
    }

    #[test]
    fn a_config_without_chatrecall_gets_the_default_and_the_marker() {
        let mut config = Mapping::new();
        run_migrations(&mut config);

        assert_eq!(
            chatrecall_entry(&config).enabled,
            PLATFORM_EXTENSIONS[chatrecall::EXTENSION_NAME].default_enabled
        );
        assert!(chatrecall_entry(&config).enabled);
        assert!(marker_recorded(&config));
    }

    #[test]
    fn an_edited_chatrecall_entry_is_never_flipped() {
        let edited = [
            OLD_DEFAULT_OFF_CHATRECALL.replace("available_tools: []", "available_tools: [x]"),
            OLD_DEFAULT_OFF_CHATRECALL.replace("display_name: Chat Recall", "display_name: Mine"),
            OLD_DEFAULT_OFF_CHATRECALL.replace("bundled: true", "bundled: false"),
            OLD_DEFAULT_OFF_CHATRECALL.replace("type: platform", "type: builtin"),
            format!("{OLD_DEFAULT_OFF_CHATRECALL}    timeout: 30\n"),
            OLD_DEFAULT_OFF_CHATRECALL.replace(
                "Search past conversations and load session summaries for contextual memory",
                "my own words",
            ),
        ];
        for yaml in edited {
            let mut config: Mapping = serde_yaml::from_str(&yaml).unwrap();
            run_migrations(&mut config);
            assert!(!chatrecall_entry(&config).enabled, "flipped: {yaml}");
            assert!(marker_recorded(&config), "no marker: {yaml}");
        }
    }

    #[test]
    fn the_read_path_sees_the_old_default_turned_on_before_any_write() {
        let mut config: Mapping = serde_yaml::from_str(OLD_DEFAULT_OFF_CHATRECALL).unwrap();
        run_read_migrations(&mut config);
        assert!(chatrecall_entry(&config).enabled);
    }

    #[test]
    fn a_non_list_marker_skips_the_flip_instead_of_repeating_it() {
        let mut config: Mapping = serde_yaml::from_str(OLD_DEFAULT_OFF_CHATRECALL).unwrap();
        config.insert(
            yaml_key(APPLIED_MIGRATIONS_KEY),
            serde_yaml::Value::String("garbage".to_string()),
        );
        run_migrations(&mut config);
        assert!(!chatrecall_entry(&config).enabled);
        assert_eq!(
            config.get(yaml_key(APPLIED_MIGRATIONS_KEY)),
            Some(&serde_yaml::Value::String("garbage".to_string()))
        );
    }

    #[test]
    fn the_chatrecall_migration_is_idempotent() {
        let mut config: Mapping = serde_yaml::from_str(OLD_DEFAULT_OFF_CHATRECALL).unwrap();
        run_migrations(&mut config);
        assert!(!run_migrations(&mut config), "second run must not save");
        let markers = config
            .get(yaml_key(APPLIED_MIGRATIONS_KEY))
            .and_then(|v| v.as_sequence())
            .unwrap();
        assert_eq!(markers.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Provider migration tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_migrate_provider_config_basic() {
        let mut config = Mapping::new();
        config.insert(
            serde_yaml::Value::String("GOOSE_PROVIDER".to_string()),
            serde_yaml::Value::String("claude-acp".to_string()),
        );
        config.insert(
            serde_yaml::Value::String("GOOSE_MODEL".to_string()),
            serde_yaml::Value::String("current".to_string()),
        );
        config.insert(
            serde_yaml::Value::String("claude-acp_configured".to_string()),
            serde_yaml::Value::Bool(true),
        );

        let changed = migrate_provider_config(&mut config);
        assert!(changed);

        // active_provider should be set
        let active = config
            .get(serde_yaml::Value::String("active_provider".to_string()))
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(active, "claude-acp");

        // providers block should exist with the entry
        let providers = config
            .get(serde_yaml::Value::String("providers".to_string()))
            .unwrap()
            .as_mapping()
            .unwrap();
        let entry: ProviderEntry = serde_yaml::from_value(
            providers
                .get(serde_yaml::Value::String("claude-acp".to_string()))
                .unwrap()
                .clone(),
        )
        .unwrap();
        assert!(entry.enabled);
        assert!(entry.configured);
        assert_eq!(entry.model, "current");

        // Old flat keys should be removed
        assert!(!config.contains_key(serde_yaml::Value::String("GOOSE_PROVIDER".to_string())));
        assert!(!config.contains_key(serde_yaml::Value::String("GOOSE_MODEL".to_string())));
        assert!(!config.contains_key(serde_yaml::Value::String(
            "claude-acp_configured".to_string()
        )));
    }

    #[test]
    fn test_migrate_provider_config_multiple_configured() {
        let mut config = Mapping::new();
        config.insert(
            serde_yaml::Value::String("GOOSE_PROVIDER".to_string()),
            serde_yaml::Value::String("claude-acp".to_string()),
        );
        config.insert(
            serde_yaml::Value::String("GOOSE_MODEL".to_string()),
            serde_yaml::Value::String("current".to_string()),
        );
        config.insert(
            serde_yaml::Value::String("claude-acp_configured".to_string()),
            serde_yaml::Value::Bool(true),
        );
        config.insert(
            serde_yaml::Value::String("lmstudio_configured".to_string()),
            serde_yaml::Value::Bool(true),
        );

        let changed = migrate_provider_config(&mut config);
        assert!(changed);

        let providers = config
            .get(serde_yaml::Value::String("providers".to_string()))
            .unwrap()
            .as_mapping()
            .unwrap();

        // Both providers should exist
        let claude: ProviderEntry = serde_yaml::from_value(
            providers
                .get(serde_yaml::Value::String("claude-acp".to_string()))
                .unwrap()
                .clone(),
        )
        .unwrap();
        assert_eq!(claude.model, "current");
        assert!(claude.configured);

        let lmstudio: ProviderEntry = serde_yaml::from_value(
            providers
                .get(serde_yaml::Value::String("lmstudio".to_string()))
                .unwrap()
                .clone(),
        )
        .unwrap();
        // lmstudio was not the active provider, so model should be empty
        assert_eq!(lmstudio.model, "");
        assert!(lmstudio.configured);

        // Old markers removed
        assert!(!config.contains_key(serde_yaml::Value::String(
            "claude-acp_configured".to_string()
        )));
        assert!(!config.contains_key(serde_yaml::Value::String("lmstudio_configured".to_string())));
    }

    #[test]
    fn test_migrate_provider_config_idempotent() {
        let mut config = Mapping::new();
        config.insert(
            serde_yaml::Value::String("GOOSE_PROVIDER".to_string()),
            serde_yaml::Value::String("openai".to_string()),
        );
        config.insert(
            serde_yaml::Value::String("GOOSE_MODEL".to_string()),
            serde_yaml::Value::String("gpt-4o".to_string()),
        );

        let changed_first = migrate_provider_config(&mut config);
        assert!(changed_first);

        let changed_second = migrate_provider_config(&mut config);
        assert!(!changed_second, "Second migration run should be a no-op");
    }

    #[test]
    fn test_migrate_provider_config_empty_config() {
        let mut config = Mapping::new();

        let changed = migrate_provider_config(&mut config);
        assert!(!changed, "Empty config should not trigger migration");
    }

    #[test]
    fn test_migrate_provider_config_no_model() {
        let mut config = Mapping::new();
        config.insert(
            serde_yaml::Value::String("GOOSE_PROVIDER".to_string()),
            serde_yaml::Value::String("anthropic".to_string()),
        );
        // No GOOSE_MODEL key

        let changed = migrate_provider_config(&mut config);
        assert!(changed);

        let providers = config
            .get(serde_yaml::Value::String("providers".to_string()))
            .unwrap()
            .as_mapping()
            .unwrap();
        let entry: ProviderEntry = serde_yaml::from_value(
            providers
                .get(serde_yaml::Value::String("anthropic".to_string()))
                .unwrap()
                .clone(),
        )
        .unwrap();
        assert_eq!(entry.model, "");
    }

    #[test]
    fn test_cleanup_legacy_keys_when_providers_exists() {
        let mut config = Mapping::new();
        // Simulate state: providers block exists but stale flat keys remain
        let mut providers_map = Mapping::new();
        if let Ok(value) = serde_yaml::to_value(&ProviderEntry {
            enabled: true,
            model: "current".to_string(),
            configured: true,
        }) {
            providers_map.insert(serde_yaml::Value::String("claude-acp".to_string()), value);
        }
        config.insert(
            serde_yaml::Value::String("providers".to_string()),
            serde_yaml::Value::Mapping(providers_map),
        );
        config.insert(
            serde_yaml::Value::String("GOOSE_PROVIDER".to_string()),
            serde_yaml::Value::String("lmstudio".to_string()),
        );
        config.insert(
            serde_yaml::Value::String("GOOSE_MODEL".to_string()),
            serde_yaml::Value::String("some-model".to_string()),
        );
        config.insert(
            serde_yaml::Value::String("claude-acp_configured".to_string()),
            serde_yaml::Value::Bool(true),
        );

        let changed = migrate_provider_config(&mut config);
        assert!(changed);

        // Legacy keys should be gone
        assert!(!config.contains_key(serde_yaml::Value::String("GOOSE_PROVIDER".to_string())));
        assert!(!config.contains_key(serde_yaml::Value::String("GOOSE_MODEL".to_string())));
        assert!(!config.contains_key(serde_yaml::Value::String(
            "claude-acp_configured".to_string()
        )));

        // Providers block should be untouched
        assert!(config.contains_key(serde_yaml::Value::String("providers".to_string())));

        // active_provider should be backfilled from legacy GOOSE_PROVIDER
        assert_eq!(
            config
                .get(serde_yaml::Value::String("active_provider".to_string()))
                .and_then(|v| v.as_str()),
            Some("lmstudio")
        );
    }
}
