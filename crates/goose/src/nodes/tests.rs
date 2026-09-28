use std::collections::HashMap;

use goose_sdk_types::custom_requests::{
    NodeEnsureServing, NodeLoadRefusalCode, NodeResidency, NodeServedTurnDto, NodesServingKind,
    NodesServingWayDto,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::project::{build_eligibility, project, BuildInputs};
use super::residency::{residency_of, ServingFacts};
use super::resolve::{resolve, sentence_facts, Decision, EntryFact, SentenceFacts, ShareState};
use super::*;
use crate::config::Config;

// ---------------------------------------------------------------------------------------------
// The shared fixture: the desktop's suite runs the very same file.
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    configs: Vec<ConfigCase>,
    model_ids: Vec<ModelIdCase>,
    effective_role: Vec<EffectiveCase>,
    resolve: Vec<ResolveCase>,
    sentence_facts: Vec<SentenceCase>,
    chat_node_sets: Vec<ChatNodeSetCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatNodeSetCase {
    name: String,
    session: String,
    nodes: Vec<String>,
    answer_on_next: bool,
    strategy: NodeStrategy,
}

#[derive(Deserialize)]
struct ConfigCase {
    name: String,
    config: Value,
}

#[derive(Deserialize)]
struct ModelIdCase {
    id: String,
    route: Option<RouteModel>,
}

#[derive(Deserialize)]
struct EffectiveCase {
    set: Vec<NodeRole>,
    role: NodeRole,
    expect: Option<NodeRole>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveCase {
    name: String,
    entry: NodeRoleEntry,
    facts: HashMap<String, EntryFact>,
    #[serde(default)]
    sticky: Option<String>,
    share: ShareState,
    expect: Decision,
    share_after: ShareState,
}

#[derive(Deserialize)]
struct SentenceCase {
    strategy: NodeStrategy,
    role: NodeRole,
    names: HashMap<String, String>,
    expect: Option<SentenceFacts>,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("nodes.fixture.json")).expect("nodes.fixture.json parses")
}

#[test]
fn every_fixture_config_round_trips_through_serde() {
    let cases = fixture().configs;
    assert!(cases.len() >= 3);
    for case in cases {
        let typed: NodesConfig = serde_json::from_value(case.config.clone())
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert_eq!(
            serde_json::to_value(&typed).unwrap(),
            case.config,
            "{}",
            case.name
        );
        assert!(
            validate(&typed).is_empty(),
            "{}: {:?}",
            case.name,
            validate(&typed)
        );
    }
}

#[test]
fn the_model_id_grammar_parses_and_formats_every_fixture_case() {
    for case in fixture().model_ids {
        let parsed = parse_route_model(&case.id);
        assert_eq!(parsed, case.route, "{}", case.id);
        if let Some(route) = parsed {
            assert_eq!(format_route_model(&route), case.id);
        }
    }
    assert_eq!(new_chats_model(&NodesForNewChats::Auto), "swarm");
    assert_eq!(
        new_chats_model(&NodesForNewChats::Node { id: "x".into() }),
        "node:x"
    );
}

#[test]
fn inheritance_follows_the_fixture() {
    for case in fixture().effective_role {
        let mut roles = NodeStrategyRoles::default();
        for role in &case.set {
            *roles.get_mut(*role) = Some(NodeRoleEntry {
                chain: vec![NodeChainEntry {
                    node: "n".into(),
                    weight: 1,
                }],
                when: NodeWhen::Failover,
                if_not_loaded: NodeIfNotLoaded::Load,
                if_serving_other: NodeIfServingOther::TakeOver,
            });
        }
        assert_eq!(
            effective_role(&roles, case.role),
            case.expect,
            "{:?} in {:?}",
            case.role,
            case.set
        );
    }
}

#[test]
fn resolve_matches_every_fixture_case() {
    let cases = fixture().resolve;
    // 3 when-rules × 6 situations of the 1st × chains of 1, 2 and 3, plus the extras.
    assert!(cases.len() >= 54, "{}", cases.len());
    for case in cases {
        let mut share = case.share.clone();
        let decision = resolve(&case.entry, &case.facts, case.sticky.as_deref(), &mut share);
        assert_eq!(decision, case.expect, "{}", case.name);
        assert_eq!(share, case.share_after, "{} (share state)", case.name);
    }
}

#[test]
fn an_exhausted_chain_names_every_entry_never_any_node() {
    let case = fixture()
        .resolve
        .into_iter()
        .find(|c| c.name.starts_with("failover · every entry passed over"))
        .unwrap();
    let Decision::Exhausted { tried } = &case.expect else {
        panic!("{:?}", case.expect)
    };
    assert_eq!(tried.len(), case.entry.chain.len());
}

#[test]
fn sentence_facts_match_the_fixture() {
    for case in fixture().sentence_facts {
        assert_eq!(
            sentence_facts(&case.strategy, case.role, &case.names),
            case.expect,
            "{} / {:?}",
            case.strategy.name,
            case.role
        );
    }
}

#[test]
fn chat_node_sets_match_the_fixture() {
    let cases = fixture().chat_node_sets;
    assert!(cases.len() >= 4, "{}", cases.len());
    for case in cases {
        assert_eq!(
            chat_set_roles(&case.nodes, case.answer_on_next),
            case.strategy.roles,
            "{}",
            case.name
        );
        assert_eq!(
            chat_set_of(&case.strategy),
            Some((case.nodes.clone(), case.answer_on_next)),
            "{}",
            case.name
        );
        assert_eq!(
            chat_set_id(&[], &case.session),
            case.strategy.id,
            "{}",
            case.name
        );
        let named = NodeStrategy {
            chat: None,
            ..case.strategy.clone()
        };
        assert_eq!(
            chat_set_of(&named),
            None,
            "{}: a named strategy is no chat's set",
            case.name
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Adoption.
// ---------------------------------------------------------------------------------------------

fn pool(devices: Value) -> PoolView {
    serde_json::from_value(json!({ "devices": devices })).unwrap()
}

fn mlx_device(id: &str, model: &str) -> Value {
    json!({"id": id, "model_id": model, "weight": 2, "enabled": true, "instances": 1, "engine": "mlx-sidecar"})
}

fn cloud_device(id: &str, model: &str, provider: &str) -> Value {
    json!({"id": id, "model_id": model, "weight": 1, "enabled": true, "instances": 1, "host": provider, "provider": provider})
}

fn lm_device(id: &str) -> Value {
    json!({"id": id, "model_id": "qwen-27b", "weight": 1, "enabled": true, "instances": 1})
}

#[test]
fn an_mlx_device_is_adopted_as_a_follows_node_that_reads_through() {
    let pool = pool(json!([mlx_device(
        "mihai-mlx",
        "mihai-qwen3.8-27b-atlassian-q8-mlx"
    )]));
    let a = adopt(Some(&pool), empty_config(), "Mihai Macbook");
    assert_eq!(a.adopted, vec!["mihai-mlx"]);
    let def = &a.config.defs[0];
    assert_eq!(def.name, "This Mac's engine");
    assert_eq!(def.placement, Some(NodePlacement::Follows));
    assert_eq!(def.pool_device.as_deref(), Some("mihai-mlx"));
    assert_eq!(def.model, None, "a pool node never copies its model");
    assert_eq!(def.origin, NodeOrigin::Pool);
    let resolved = resolve_def(def, &Ok(Some(pool)), true);
    assert_eq!(
        resolved.model.as_deref(),
        Some("mihai-qwen3.8-27b-atlassian-q8-mlx")
    );
    assert_eq!(resolved.model_from, NodeModelFrom::Pool);
    assert!(resolved.pending_adoption);
}

#[test]
fn two_mlx_devices_on_one_mac_get_distinct_names() {
    let pool = pool(json!([
        mlx_device("mihai-mlx", "mihai-qwen3.8-27b-atlassian-q8-mlx"),
        mlx_device("mihai-flash-mlx", "mihai-flash-qwen3.8-flash-next-4bit-mlx"),
        mlx_device("third-mlx", "mihai-flash-qwen3.8-flash-next-4bit-mlx"),
    ]));
    let a = adopt(Some(&pool), empty_config(), "Mihai Macbook");
    let names: Vec<&str> = a.config.defs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "This Mac's engine",
            "This Mac's engine · mihai-flash-qwen3.8-flash-next-4bit",
            "This Mac's engine · third-mlx",
        ]
    );
    assert!(validate(&a.config).is_empty(), "{:?}", validate(&a.config));
}

/// Q-303: a pool node adopted under the old rule ("<Mac name> engine") is named after what it
/// follows; a name the person typed, and a node that is not a follows pool node, keep theirs.
#[test]
fn a_pool_node_named_by_the_old_rule_takes_the_followed_name_and_a_typed_name_stays() {
    let pool = pool(json!([
        mlx_device("mihai-mlx", "mihai-qwen3.8-27b-atlassian-q8-mlx"),
        mlx_device("mihai-flash-mlx", "mihai-flash-qwen3.8-flash-next-4bit-mlx"),
        mlx_device("typed-mlx", "m"),
    ]));
    let first = adopt(Some(&pool), empty_config(), "Mihai Macbook");
    let mut stored = first.config.clone();
    stored.defs[0].name = "Mihai Macbook engine".into();
    stored.defs[1].name = "Mihai Macbook engine · mihai-flash-qwen3.8-flash-next-4bit".into();
    stored.defs[2].name = "Mihai Macbook engine room".into();
    let mut own = flash_here("own");
    own.name = "Mihai Macbook engine (2)".into();
    stored.defs.push(own);

    let a = adopt(Some(&pool), stored, "Mihai Macbook");
    let names: Vec<&str> = a.config.defs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "This Mac's engine",
            "This Mac's engine · mihai-flash-qwen3.8-flash-next-4bit",
            "Mihai Macbook engine room",
            "Mihai Macbook engine (2)",
        ]
    );
    assert!(a.adopted.is_empty(), "a rename is not an adoption");
    let again = adopt(Some(&pool), a.config.clone(), "Mihai Macbook");
    assert_eq!(again.config, a.config, "renaming is idempotent");
}

#[test]
fn a_cloud_device_is_adopted_reading_model_and_provider_through() {
    let pool = pool(json!([cloud_device(
        "openrouter-anthropic-claude-sonnet-4",
        "anthropic/claude-sonnet-4",
        "openrouter"
    )]));
    let a = adopt(Some(&pool), empty_config(), "Mihai Macbook");
    let def = &a.config.defs[0];
    assert_eq!(def.kind, NodeDefKind::Cloud);
    assert_eq!(def.name, "claude-sonnet-4 · openrouter");
    assert_eq!((def.model.as_ref(), def.provider.as_ref()), (None, None));
    let resolved = resolve_def(def, &Ok(Some(pool)), false);
    assert_eq!(resolved.model.as_deref(), Some("anthropic/claude-sonnet-4"));
    assert_eq!(resolved.provider.as_deref(), Some("openrouter"));
}

#[test]
fn lm_studio_devices_are_counted_never_adopted() {
    let pool = pool(json!([lm_device("gabee"), lm_device("workhorse")]));
    let a = adopt(Some(&pool), empty_config(), "Mihai Macbook");
    assert!(a.config.defs.is_empty());
    assert_eq!(a.lm_studio, 2);
}

#[test]
fn a_declined_device_is_never_re_adopted_and_adoption_is_idempotent() {
    let pool = pool(json!([
        mlx_device("mihai-mlx", "m"),
        cloud_device("c1", "anthropic/claude-sonnet-4", "openrouter"),
    ]));
    let mut declined = empty_config();
    declined.declined.push("c1".into());
    let a = adopt(Some(&pool), declined, "Mac");
    assert_eq!(a.adopted, vec!["mihai-mlx"]);
    let again = adopt(Some(&pool), a.config.clone(), "Mac");
    assert!(again.adopted.is_empty());
    assert_eq!(again.config, a.config);
}

#[test]
fn a_device_that_left_the_pool_is_kept_and_named() {
    let pool_with = pool(json!([mlx_device("mihai-mlx", "m")]));
    let a = adopt(Some(&pool_with), empty_config(), "Mac");
    let empty = pool(json!([]));
    let resolved = resolve_def(&a.config.defs[0], &Ok(Some(empty)), false);
    assert_eq!(resolved.model_from, NodeModelFrom::LeftPool);
    let unreadable = resolve_def(&a.config.defs[0], &Err("broken".into()), false);
    assert!(matches!(
        unreadable.model_from,
        NodeModelFrom::PoolUnreadable { .. }
    ));
}

#[test]
fn a_user_node_holding_a_device_id_does_not_block_its_adoption() {
    let mut config = empty_config();
    config.defs.push(flash_here("mihai-mlx"));
    let a = adopt(
        Some(&pool(json!([mlx_device("mihai-mlx", "m")]))),
        config,
        "Mac",
    );
    assert_eq!(a.adopted, vec!["mihai-mlx-pool"]);
    assert!(validate(&a.config).is_empty(), "{:?}", validate(&a.config));
}

// ---------------------------------------------------------------------------------------------
// Validation: one refusal per rule, with the negative controls that must pass.
// ---------------------------------------------------------------------------------------------

fn flash_here(id: &str) -> NodeDef {
    NodeDef {
        id: id.into(),
        name: format!("Flash · {id}"),
        kind: NodeDefKind::Mlx,
        model: Some("rapid-mlx/Qwen3.8-Flash-Next-4bit".into()),
        placement: Some(NodePlacement::Single {
            macs: vec![THIS_MAC.into()],
            link: None,
        }),
        goal: None,
        provider: None,
        keep_loaded: false,
        pool_device: None,
        origin: NodeOrigin::User,
    }
}

fn split_27b(id: &str) -> NodeDef {
    NodeDef {
        id: id.into(),
        name: "27B · both Macs".into(),
        kind: NodeDefKind::Mlx,
        model: Some("Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".into()),
        placement: Some(NodePlacement::Pipeline {
            macs: vec![THIS_MAC.into(), "link:studio".into()],
            link: Some("jaccl".into()),
        }),
        goal: None,
        provider: None,
        keep_loaded: false,
        pool_device: None,
        origin: NodeOrigin::RunIt,
    }
}

fn sonnet(id: &str) -> NodeDef {
    NodeDef {
        id: id.into(),
        name: "Claude Sonnet".into(),
        kind: NodeDefKind::Cloud,
        model: Some("anthropic/claude-sonnet-4".into()),
        placement: None,
        goal: None,
        provider: Some("openrouter".into()),
        keep_loaded: false,
        pool_device: None,
        origin: NodeOrigin::User,
    }
}

fn entry(nodes: &[(&str, u32)], when: NodeWhen) -> NodeRoleEntry {
    NodeRoleEntry {
        chain: nodes
            .iter()
            .map(|(n, w)| NodeChainEntry {
                node: n.to_string(),
                weight: *w,
            })
            .collect(),
        when,
        if_not_loaded: NodeIfNotLoaded::Load,
        if_serving_other: NodeIfServingOther::TakeOver,
    }
}

fn strategy(id: &str, chat: Option<NodeRoleEntry>, build: Option<NodeRoleEntry>) -> NodeStrategy {
    NodeStrategy {
        id: id.into(),
        name: format!("Strategy {id}"),
        note: None,
        roles: NodeStrategyRoles {
            chat,
            build,
            ..NodeStrategyRoles::default()
        },
        chat: None,
    }
}

fn chat_set(id: &str, session: &str, nodes: &[&str]) -> NodeStrategy {
    let nodes: Vec<String> = nodes.iter().map(|n| n.to_string()).collect();
    NodeStrategy {
        id: id.into(),
        name: format!("This chat's nodes ({id})"),
        note: None,
        roles: chat_set_roles(&nodes, false),
        chat: Some(session.into()),
    }
}

fn codes(config: &NodesConfig) -> Vec<NodesRefusalCode> {
    validate(config).into_iter().map(|r| r.code).collect()
}

fn base() -> NodesConfig {
    let mut c = empty_config();
    c.defs = vec![flash_here("flash"), split_27b("split"), sonnet("sonnet")];
    c
}

#[test]
fn the_base_config_is_valid() {
    assert!(validate(&base()).is_empty());
}

type Mutation = Box<dyn Fn(&mut NodesConfig)>;

#[test]
fn each_validation_rule_refuses_by_code() {
    use NodesRefusalCode as C;
    let cases: Vec<(&str, Mutation, C)> = vec![
        (
            "version",
            Box::new(|c| c.version = 2),
            C::UnsupportedVersion,
        ),
        (
            "bad id",
            Box::new(|c| c.defs[0].id = "a:b".into()),
            C::BadId,
        ),
        (
            "duplicate id",
            Box::new(|c| c.defs[1].id = "flash".into()),
            C::DuplicateId,
        ),
        (
            "duplicate name",
            Box::new(|c| c.defs[1].name = c.defs[0].name.clone()),
            C::DuplicateName,
        ),
        (
            "empty name",
            Box::new(|c| c.defs[0].name = " ".into()),
            C::EmptyName,
        ),
        (
            "missing model",
            Box::new(|c| c.defs[0].model = None),
            C::MissingModel,
        ),
        (
            "missing provider",
            Box::new(|c| c.defs[2].provider = None),
            C::MissingProvider,
        ),
        (
            "pool node owns a model",
            Box::new(|c| {
                c.defs[0].pool_device = Some("d".into());
                c.defs[0].placement = Some(NodePlacement::Follows);
            }),
            C::PoolNodeOwnsModel,
        ),
        (
            "mlx with no way",
            Box::new(|c| c.defs[0].placement = None),
            C::PlacementMismatch,
        ),
        (
            "follows without a pool device",
            Box::new(|c| c.defs[0].placement = Some(NodePlacement::Follows)),
            C::PlacementMismatch,
        ),
        (
            "cloud with a way",
            Box::new(|c| {
                c.defs[2].placement = Some(NodePlacement::Single {
                    macs: vec![THIS_MAC.into()],
                    link: None,
                })
            }),
            C::PlacementMismatch,
        ),
        (
            "single on two Macs",
            Box::new(|c| {
                c.defs[0].placement = Some(NodePlacement::Single {
                    macs: vec![THIS_MAC.into(), "link:x".into()],
                    link: None,
                })
            }),
            C::BadMacs,
        ),
        (
            "split on one Mac",
            Box::new(|c| {
                c.defs[1].placement = Some(NodePlacement::Tensor {
                    macs: vec![THIS_MAC.into()],
                    link: None,
                })
            }),
            C::BadMacs,
        ),
        (
            "same Mac twice",
            Box::new(|c| {
                c.defs[1].placement = Some(NodePlacement::Tensor {
                    macs: vec![THIS_MAC.into(), THIS_MAC.into()],
                    link: None,
                })
            }),
            C::BadMacs,
        ),
        (
            "unknown node in a chain",
            Box::new(|c| {
                c.strategies = vec![strategy(
                    "s",
                    Some(entry(&[("ghost", 1)], NodeWhen::Failover)),
                    None,
                )]
            }),
            C::UnknownNode,
        ),
        (
            "empty chain",
            Box::new(|c| {
                c.strategies = vec![strategy("s", Some(entry(&[], NodeWhen::Failover)), None)]
            }),
            C::EmptyChain,
        ),
        (
            "duplicate entry",
            Box::new(|c| {
                c.strategies = vec![strategy(
                    "s",
                    Some(entry(&[("flash", 1), ("flash", 1)], NodeWhen::Failover)),
                    None,
                )]
            }),
            C::DuplicateEntry,
        ),
        (
            "zero weight",
            Box::new(|c| {
                c.strategies = vec![strategy(
                    "s",
                    Some(entry(&[("flash", 0)], NodeWhen::Share)),
                    None,
                )]
            }),
            C::ZeroWeight,
        ),
        (
            "no role",
            Box::new(|c| c.strategies = vec![strategy("s", None, None)]),
            C::NoRoleSet,
        ),
        (
            "neither chat nor build",
            Box::new(|c| {
                let mut s = strategy("s", None, None);
                s.roles.planning = Some(entry(&[("flash", 1)], NodeWhen::Failover));
                c.strategies = vec![s];
            }),
            C::InheritanceCycle,
        ),
        (
            "share across two MLX ways",
            Box::new(|c| {
                c.strategies = vec![strategy(
                    "s",
                    Some(entry(&[("split", 2), ("flash", 1)], NodeWhen::Share)),
                    None,
                )]
            }),
            C::SharesTwoWays,
        ),
        (
            "overflow across two MLX ways",
            Box::new(|c| {
                c.strategies = vec![strategy(
                    "s",
                    None,
                    Some(entry(&[("flash", 1), ("split", 1)], NodeWhen::Overflow)),
                )]
            }),
            C::SharesTwoWays,
        ),
        (
            "new chats on a missing node",
            Box::new(|c| c.for_new_chats = NodesForNewChats::Node { id: "ghost".into() }),
            C::UnknownNode,
        ),
        (
            "builds on a missing strategy",
            Box::new(|c| c.for_builds = NodesForBuilds::Strategy { id: "ghost".into() }),
            C::UnknownStrategy,
        ),
        (
            "new chats on one chat's own nodes",
            Box::new(|c| {
                c.strategies = vec![chat_set("chat-1", "1", &["sonnet"])];
                c.for_new_chats = NodesForNewChats::Strategy {
                    id: "chat-1".into(),
                };
            }),
            C::ChatNodeSetNotShared,
        ),
        (
            "builds on one chat's own nodes",
            Box::new(|c| {
                c.strategies = vec![chat_set("chat-1", "1", &["sonnet"])];
                c.for_builds = NodesForBuilds::Strategy {
                    id: "chat-1".into(),
                };
            }),
            C::ChatNodeSetNotShared,
        ),
        (
            "two node sets for one chat",
            Box::new(|c| {
                c.strategies = vec![
                    chat_set("chat-1", "1", &["sonnet"]),
                    chat_set("chat-1-2", "1", &["flash"]),
                ];
            }),
            C::BadChatNodeSet,
        ),
        (
            "a node set of no chat",
            Box::new(|c| c.strategies = vec![chat_set("chat-", " ", &["sonnet"])]),
            C::BadChatNodeSet,
        ),
        (
            "a chat's set sharing two MLX ways",
            Box::new(|c| c.strategies = vec![chat_set("chat-1", "1", &["split", "flash"])]),
            C::SharesTwoWays,
        ),
    ];
    for (name, mutate, code) in cases {
        let mut config = base();
        mutate(&mut config);
        assert!(
            codes(&config).contains(&code),
            "{name}: {:?}",
            validate(&config)
        );
    }
}

#[test]
fn allowed_shapes_are_not_refused() {
    let mut config = base();
    config.strategies = vec![
        // failover across two MLX ways is the chain's own order: allowed.
        strategy(
            "a",
            Some(entry(&[("split", 1), ("flash", 1)], NodeWhen::Failover)),
            None,
        ),
        // share between one MLX way and cloud: allowed.
        strategy(
            "b",
            None,
            Some(entry(&[("split", 2), ("sonnet", 1)], NodeWhen::Share)),
        ),
    ];
    // An unknown Link Mac is kept, and `local` is this Mac.
    config.defs.push(NodeDef {
        placement: Some(NodePlacement::Single {
            macs: vec!["link:never-seen".into()],
            link: None,
        }),
        ..flash_here("remote")
    });
    config.defs.last_mut().unwrap().name = "remote".into();
    assert!(validate(&config).is_empty(), "{:?}", validate(&config));
}

// ---------------------------------------------------------------------------------------------
// The store: a read never writes; a write never writes `swarm`; forNewChats writes the defaults.
// ---------------------------------------------------------------------------------------------

struct TestConfig {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    config: Config,
}

fn test_config(yaml: &str) -> TestConfig {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, yaml).unwrap();
    let config = Config::new_with_file_secrets(&path, dir.path().join("secrets.yaml")).unwrap();
    TestConfig {
        _dir: dir,
        path,
        config,
    }
}

const POOL_YAML: &str = "\
GOOSE_PROVIDER: anthropic
swarm:
  endpoint: http://localhost:1234
  planner_model: workhorse-qwopus3.6-27b-coder-mtp
  future_field: kept
  devices:
  - id: mihai-mlx
    model_id: mihai-qwen3.8-27b-atlassian-q8-mlx
    weight: 2
    enabled: true
    instances: 1
    engine: mlx-sidecar
  - id: gabee
    model_id: qwen-27b
    weight: 1
    enabled: true
    instances: 1
";

fn facts(engine: &Result<goose_sidecar::engine::EngineSettings, String>) -> WriteFacts<'_> {
    WriteFacts {
        this_mac: Ok("Mihai Macbook".into()),
        engine,
    }
}

fn no_engine() -> Result<goose_sidecar::engine::EngineSettings, String> {
    Ok(goose_sidecar::engine::EngineSettings::default())
}

#[test]
fn a_read_never_writes_config() {
    let t = test_config(POOL_YAML);
    let before = std::fs::read(&t.path).unwrap();
    let read = read(&t.config, Ok("Mihai Macbook".into())).unwrap();
    assert_eq!(
        std::fs::read(&t.path).unwrap(),
        before,
        "a read wrote config"
    );
    assert!(!read.stored);
    assert_eq!(read.lm_studio_hidden, 1);
    assert_eq!(read.config.defs.len(), 1);
    assert!(read.nodes[0].pending_adoption);
    assert_eq!(read.config.for_new_chats, NodesForNewChats::Auto);
    assert_eq!(read.config.for_builds, NodesForBuilds::Pool);
}

#[test]
fn an_unreadable_nodes_key_is_an_error_never_an_empty_config() {
    let t = test_config("nodes: [1, 2]\n");
    let err = read(&t.config, Ok("Mac".into())).unwrap_err();
    assert!(err.to_string().contains("`nodes`"), "{err}");
}

#[test]
fn a_write_stores_nodes_never_writes_swarm_and_leaves_auto_defaults_alone() {
    let t = test_config(POOL_YAML);
    let swarm_before = t.config.get_param::<Value>("swarm").unwrap();
    let current = read(&t.config, Ok("Mihai Macbook".into())).unwrap();
    let engine = no_engine();
    let mut next = current.config.clone();
    next.defs.push(sonnet("sonnet"));
    let out = write(&t.config, next, facts(&engine)).unwrap();
    assert!(out.written, "{:?}", out.refusals);
    assert_eq!(t.config.get_param::<Value>("swarm").unwrap(), swarm_before);
    let stored = read_stored(&t.config).unwrap().unwrap();
    assert_eq!(
        stored.defs.len(),
        2,
        "the pending adoption is stored by the write"
    );
    // forNewChats stayed auto: the global defaults were not touched. (Any save runs the config's
    // own key migration, GOOSE_PROVIDER → active_provider, with the same value.)
    let text = std::fs::read_to_string(&t.path).unwrap();
    assert!(text.contains(": anthropic"), "{text}");
    assert!(crate::config::get_provider_entry(&t.config, "swarm").is_none());
}

#[test]
fn a_write_of_for_new_chats_writes_both_defaults() {
    let t = test_config(POOL_YAML);
    let engine = no_engine();
    let mut next = read(&t.config, Ok("Mac".into())).unwrap().config;
    next.defs.push(sonnet("sonnet"));
    next.for_new_chats = NodesForNewChats::Node {
        id: "sonnet".into(),
    };
    let out = write(&t.config, next, facts(&engine)).unwrap();
    assert!(out.written, "{:?}", out.refusals);
    assert_eq!(
        t.config.get_param::<String>("active_provider").unwrap(),
        "swarm"
    );
    assert_eq!(
        crate::config::get_provider_entry(&t.config, "swarm")
            .unwrap()
            .model,
        "node:sonnet"
    );
    // Back to auto: the defaults follow.
    let mut back = read(&t.config, Ok("Mac".into())).unwrap().config;
    back.for_new_chats = NodesForNewChats::Auto;
    assert!(write(&t.config, back, facts(&engine)).unwrap().written);
    assert_eq!(
        crate::config::get_provider_entry(&t.config, "swarm")
            .unwrap()
            .model,
        "swarm"
    );
}

#[test]
fn a_refused_write_writes_nothing() {
    let t = test_config(POOL_YAML);
    let before = std::fs::read(&t.path).unwrap();
    let engine = no_engine();
    let mut next = read(&t.config, Ok("Mac".into())).unwrap().config;
    next.for_new_chats = NodesForNewChats::Node { id: "ghost".into() };
    let out = write(&t.config, next, facts(&engine)).unwrap();
    assert!(!out.written);
    assert_eq!(out.refusals[0].code, NodesRefusalCode::UnknownNode);
    assert_eq!(std::fs::read(&t.path).unwrap(), before);
}

#[test]
fn dropping_a_node_in_a_write_is_refused_removal_goes_through_remove_node() {
    let t = test_config(POOL_YAML);
    let engine = no_engine();
    let mut next = read(&t.config, Ok("Mac".into())).unwrap().config;
    next.defs.push(sonnet("sonnet"));
    assert!(
        write(&t.config, next.clone(), facts(&engine))
            .unwrap()
            .written
    );
    next.defs.retain(|d| d.id != "sonnet");
    let out = write(&t.config, next, facts(&engine)).unwrap();
    assert_eq!(
        out.refusals[0].code,
        NodesRefusalCode::RemovedOutsideRemoveNode
    );
}

fn remove(id: &str) -> RemoveNode<'_> {
    RemoveNode {
        id,
        also_from_strategies: false,
        also_from_chat_node_sets: false,
        and_new_chats_auto: false,
        acknowledged_sessions: None,
        live_sessions: 0,
    }
}

#[test]
fn removing_a_node_new_chats_start_on_is_refused_unless_chats_go_to_auto() {
    let t = test_config(POOL_YAML);
    let engine = no_engine();
    let mut next = read(&t.config, Ok("Mac".into())).unwrap().config;
    next.defs.push(sonnet("sonnet"));
    next.for_new_chats = NodesForNewChats::Node {
        id: "sonnet".into(),
    };
    assert!(write(&t.config, next, facts(&engine)).unwrap().written);

    let refused = remove_node(&t.config, remove("sonnet"), facts(&engine)).unwrap();
    assert!(!refused.written);
    assert_eq!(
        refused.refusals[0].code,
        NodesRefusalCode::NodeIsForNewChats
    );

    let done = remove_node(
        &t.config,
        RemoveNode {
            and_new_chats_auto: true,
            ..remove("sonnet")
        },
        facts(&engine),
    )
    .unwrap();
    assert!(done.written, "{:?}", done.refusals);
    assert_eq!(done.read.config.for_new_chats, NodesForNewChats::Auto);
    assert_eq!(
        crate::config::get_provider_entry(&t.config, "swarm")
            .unwrap()
            .model,
        "swarm"
    );
}

#[test]
fn removing_a_node_a_strategy_uses_or_live_chats_are_set_to_needs_consent() {
    let t = test_config(POOL_YAML);
    let engine = no_engine();
    let mut next = read(&t.config, Ok("Mac".into())).unwrap().config;
    next.defs.push(sonnet("sonnet"));
    next.strategies.push(strategy(
        "s",
        Some(entry(
            &[("mihai-mlx", 1), ("sonnet", 1)],
            NodeWhen::Failover,
        )),
        None,
    ));
    assert!(write(&t.config, next, facts(&engine)).unwrap().written);

    let in_use = remove_node(
        &t.config,
        RemoveNode {
            live_sessions: 3,
            ..remove("sonnet")
        },
        facts(&engine),
    )
    .unwrap();
    let got: Vec<_> = in_use.refusals.iter().map(|r| r.code).collect();
    assert!(got.contains(&NodesRefusalCode::NodeInUse), "{got:?}");
    assert!(
        got.contains(&NodesRefusalCode::LiveSessionsNotAcknowledged),
        "{got:?}"
    );
    assert!(in_use
        .refusals
        .iter()
        .any(|r| r.message.contains("3 chats are")));
    let live: Vec<_> = in_use
        .refusals
        .iter()
        .map(|r| (r.code, r.live_sessions))
        .collect();
    assert!(
        live.contains(&(NodesRefusalCode::LiveSessionsNotAcknowledged, Some(3))),
        "{live:?}"
    );
    assert!(
        in_use
            .refusals
            .iter()
            .filter(|r| r.code != NodesRefusalCode::LiveSessionsNotAcknowledged)
            .all(|r| r.live_sessions.is_none()),
        "{live:?}"
    );

    let done = remove_node(
        &t.config,
        RemoveNode {
            also_from_strategies: true,
            acknowledged_sessions: Some(3),
            live_sessions: 3,
            ..remove("sonnet")
        },
        facts(&engine),
    )
    .unwrap();
    assert!(done.written, "{:?}", done.refusals);
    let chat = done.read.config.strategies[0].roles.chat.as_ref().unwrap();
    assert_eq!(chat.chain.len(), 1);
}

fn set_chat<'a>(session: &'a str, nodes: &[&str], answer_on_next: bool) -> SetChatNodes<'a> {
    SetChatNodes {
        session,
        nodes: nodes.iter().map(|n| n.to_string()).collect(),
        answer_on_next,
    }
}

#[test]
fn a_chats_node_set_is_one_strategy_owned_by_the_chat_and_replaced_in_place() {
    let t = test_config(POOL_YAML);
    let engine = no_engine();
    let mut next = read(&t.config, Ok("Mac".into())).unwrap().config;
    next.defs.push(sonnet("sonnet"));
    assert!(write(&t.config, next, facts(&engine)).unwrap().written);

    let first = set_chat_nodes(
        &t.config,
        set_chat("20260928_7", &["mihai-mlx"], false),
        facts(&engine),
    )
    .unwrap();
    assert!(first.written, "{:?}", first.refusals);
    let set = chat_set_for(&first.read.config, "20260928_7").unwrap();
    assert_eq!(set.id, "chat-20260928_7");
    assert_eq!(
        chat_set_route(&first.read.config, "20260928_7").as_deref(),
        Some("strategy:chat-20260928_7")
    );
    assert_eq!(
        first.read.config.for_new_chats,
        NodesForNewChats::Auto,
        "a chat's set never touches what new chats start on"
    );

    let second = set_chat_nodes(
        &t.config,
        set_chat("20260928_7", &["mihai-mlx", "sonnet"], true),
        facts(&engine),
    )
    .unwrap();
    assert!(second.written, "{:?}", second.refusals);
    let sets: Vec<_> = second
        .read
        .config
        .strategies
        .iter()
        .filter(|s| s.chat.is_some())
        .collect();
    assert_eq!(sets.len(), 1, "one chat, one set: replaced in place");
    assert_eq!(sets[0].id, "chat-20260928_7");
    assert_eq!(
        chat_set_of(sets[0]),
        Some((vec!["mihai-mlx".to_string(), "sonnet".to_string()], true))
    );

    let refused = set_chat_nodes(
        &t.config,
        set_chat("20260928_7", &["mihai-mlx", "ghost"], false),
        facts(&engine),
    )
    .unwrap();
    assert!(!refused.written);
    assert!(refused
        .refusals
        .iter()
        .any(|r| r.code == NodesRefusalCode::UnknownNode));
    assert_eq!(
        chat_set_of(chat_set_for(&refused.read.config, "20260928_7").unwrap()),
        Some((vec!["mihai-mlx".to_string(), "sonnet".to_string()], true)),
        "a refused set leaves the stored one as it was"
    );

    let other = set_chat_nodes(
        &t.config,
        set_chat("20260928_8", &["sonnet"], false),
        facts(&engine),
    )
    .unwrap();
    assert!(other.written, "{:?}", other.refusals);
    assert_eq!(
        other
            .read
            .config
            .strategies
            .iter()
            .filter(|s| s.chat.is_some())
            .count(),
        2
    );

    let cleared = set_chat_nodes(
        &t.config,
        set_chat("20260928_7", &[], false),
        facts(&engine),
    )
    .unwrap();
    assert!(cleared.written);
    assert!(chat_set_for(&cleared.read.config, "20260928_7").is_none());
    assert!(chat_set_for(&cleared.read.config, "20260928_8").is_some());
}

#[test]
fn a_deleted_chats_set_goes_and_a_chat_with_none_writes_nothing() {
    let t = test_config(POOL_YAML);
    let engine = no_engine();
    let before = std::fs::read(&t.path).unwrap();
    assert!(forget_chat(&t.config, "20260928_7", facts(&engine))
        .unwrap()
        .is_none());
    assert_eq!(
        std::fs::read(&t.path).unwrap(),
        before,
        "no nodes key: nothing is written"
    );

    let mut next = read(&t.config, Ok("Mac".into())).unwrap().config;
    next.defs.push(sonnet("sonnet"));
    assert!(write(&t.config, next, facts(&engine)).unwrap().written);
    assert!(
        set_chat_nodes(
            &t.config,
            set_chat("20260928_7", &["sonnet"], false),
            facts(&engine)
        )
        .unwrap()
        .written
    );
    let stored = std::fs::read(&t.path).unwrap();
    assert!(forget_chat(&t.config, "another-chat", facts(&engine))
        .unwrap()
        .is_none());
    assert_eq!(std::fs::read(&t.path).unwrap(), stored);

    let forgot = forget_chat(&t.config, "20260928_7", facts(&engine))
        .unwrap()
        .unwrap();
    assert!(forgot.written, "{:?}", forgot.refusals);
    assert!(read_stored(&t.config)
        .unwrap()
        .unwrap()
        .strategies
        .is_empty());
}

#[test]
fn removing_a_node_in_chats_sets_needs_its_own_box_and_moves_the_lead() {
    let t = test_config(POOL_YAML);
    let engine = no_engine();
    let mut next = read(&t.config, Ok("Mac".into())).unwrap().config;
    next.defs.push(sonnet("sonnet"));
    assert!(write(&t.config, next, facts(&engine)).unwrap().written);
    for (session, nodes, answer) in [
        ("a", vec!["sonnet", "mihai-mlx"], true),
        ("b", vec!["sonnet"], false),
    ] {
        let out =
            set_chat_nodes(&t.config, set_chat(session, &nodes, answer), facts(&engine)).unwrap();
        assert!(out.written, "{:?}", out.refusals);
    }

    let refused = remove_node(&t.config, remove("sonnet"), facts(&engine)).unwrap();
    assert!(!refused.written);
    let in_sets = refused
        .refusals
        .iter()
        .find(|r| r.code == NodesRefusalCode::NodeInChatNodeSets)
        .unwrap_or_else(|| panic!("{:?}", refused.refusals));
    assert!(
        in_sets.message.contains("2 chats' node sets"),
        "{}",
        in_sets.message
    );
    assert!(
        refused
            .refusals
            .iter()
            .all(|r| r.code != NodesRefusalCode::NodeInUse),
        "no named strategy uses it: {:?}",
        refused.refusals
    );

    let only_strategies = remove_node(
        &t.config,
        RemoveNode {
            also_from_strategies: true,
            ..remove("sonnet")
        },
        facts(&engine),
    )
    .unwrap();
    assert!(
        !only_strategies.written,
        "the strategies box never answers for the chats' sets"
    );

    let done = remove_node(
        &t.config,
        RemoveNode {
            also_from_chat_node_sets: true,
            ..remove("sonnet")
        },
        facts(&engine),
    )
    .unwrap();
    assert!(done.written, "{:?}", done.refusals);
    let a = chat_set_for(&done.read.config, "a").unwrap();
    assert_eq!(
        chat_set_of(a),
        Some((vec!["mihai-mlx".to_string()], false)),
        "the next node leads; one node left answers alone"
    );
    assert!(
        chat_set_for(&done.read.config, "b").is_none(),
        "a set left empty goes"
    );
}

#[test]
fn removing_a_pool_node_declines_its_device_and_leaves_swarm_untouched() {
    let t = test_config(POOL_YAML);
    let swarm_before = t.config.get_param::<Value>("swarm").unwrap();
    let engine = no_engine();
    let done = remove_node(&t.config, remove("mihai-mlx"), facts(&engine)).unwrap();
    assert!(done.written, "{:?}", done.refusals);
    assert_eq!(done.read.config.declined, vec!["mihai-mlx"]);
    assert!(done.read.config.defs.is_empty(), "declined, not re-adopted");
    assert_eq!(t.config.get_param::<Value>("swarm").unwrap(), swarm_before);
}

#[test]
fn removing_the_strategy_builds_use_is_refused_unless_builds_go_back_to_the_pool() {
    let t = test_config(POOL_YAML);
    let engine: Result<goose_sidecar::engine::EngineSettings, String> =
        Ok(serde_json::from_value(json!({"model_id": "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx", "served_model_name": "mihai-qwen3.8-27b-atlassian-q8-mlx"})).unwrap());
    let mut next = read(&t.config, Ok("Mac".into())).unwrap().config;
    next.strategies.push(strategy(
        "s",
        Some(entry(&[("mihai-mlx", 1)], NodeWhen::Failover)),
        None,
    ));
    next.for_builds = NodesForBuilds::Strategy { id: "s".into() };
    let stored = write(&t.config, next, facts(&engine)).unwrap();
    assert!(stored.written, "{:?}", stored.refusals);
    let refused = remove_strategy(
        &t.config,
        RemoveStrategy {
            id: "s",
            and_new_chats_auto: false,
            and_builds_pool: false,
        },
        facts(&engine),
    )
    .unwrap();
    assert_eq!(
        refused.refusals[0].code,
        NodesRefusalCode::StrategyIsForBuilds
    );
    let done = remove_strategy(
        &t.config,
        RemoveStrategy {
            id: "s",
            and_new_chats_auto: false,
            and_builds_pool: true,
        },
        facts(&engine),
    )
    .unwrap();
    assert!(done.written);
    assert_eq!(done.read.config.for_builds, NodesForBuilds::Pool);
}

#[test]
fn for_builds_on_an_ineligible_strategy_is_refused_with_its_reasons() {
    let t = test_config(POOL_YAML);
    let engine = no_engine();
    let mut next = read(&t.config, Ok("Mac".into())).unwrap().config;
    next.defs.push(split_27b("split"));
    next.strategies.push(strategy(
        "s",
        Some(entry(&[("split", 1)], NodeWhen::Failover)),
        None,
    ));
    next.for_builds = NodesForBuilds::Strategy { id: "s".into() };
    let out = write(&t.config, next, facts(&engine)).unwrap();
    assert!(!out.written);
    assert!(out
        .refusals
        .iter()
        .all(|r| r.code == NodesRefusalCode::BuildIneligible));
    assert!(
        out.refusals
            .iter()
            .any(|r| r.message.contains("is a split")),
        "{:?}",
        out.refusals
    );
}

// ---------------------------------------------------------------------------------------------
// Tier A: project() and its refusals.
// ---------------------------------------------------------------------------------------------

fn golden_block() -> Value {
    json!({
        "endpoint": "http://localhost:1234",
        "planner_model": "mihai-qwen3.8-27b-atlassian-q8-mlx",
        "worker_max_turns": 40,
        "speed_weights": {"local": 2, "gabee": 1},
        "future_field": {"kept": true},
        "devices": [
            {"id": "mihai-mlx", "model_id": "mihai-qwen3.8-27b-atlassian-q8-mlx", "weight": 2,
             "enabled": true, "instances": 1, "engine": "mlx-sidecar", "speed_weight": 2},
            {"id": "gabee", "model_id": "qwen-27b", "weight": 1, "enabled": true, "instances": 1},
            {"id": "or-sonnet", "model_id": "anthropic/claude-sonnet-4", "weight": 1, "enabled": true,
             "instances": 1, "host": "openrouter", "provider": "openrouter", "speed_weight": 1},
            {"id": "old-cloud", "model_id": "x", "weight": 1, "enabled": false, "instances": 1,
             "provider": "zai"}
        ]
    })
}

fn engine_27b() -> goose_sidecar::engine::EngineSettings {
    serde_json::from_value(json!({
        "model_id": "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx",
        "served_model_name": "mihai-qwen3.8-27b-atlassian-q8-mlx"
    }))
    .unwrap()
}

/// The nodes a read of the golden block adopts, plus a strategy whose projection IS the pool.
fn golden_nodes() -> NodesConfig {
    let pool: PoolView = serde_json::from_value(golden_block()).unwrap();
    let mut nodes = adopt(Some(&pool), empty_config(), "Mihai Macbook").config;
    let mut golden = strategy(
        "golden",
        None,
        Some(entry(
            &[("mihai-mlx", 2), ("or-sonnet", 1)],
            NodeWhen::Share,
        )),
    );
    golden.roles.planning = Some(entry(&[("mihai-mlx", 1)], NodeWhen::Failover));
    nodes.strategies.push(golden);
    nodes
}

fn inputs(nodes: &NodesConfig) -> BuildInputs<'_> {
    BuildInputs {
        swarm: Ok(Some(golden_block())),
        engine: Ok(engine_27b()),
        nodes,
    }
}

#[test]
fn projecting_the_golden_identity_strategy_is_byte_identical() {
    let nodes = golden_nodes();
    let projected = project(&inputs(&nodes), "golden").unwrap();
    assert_eq!(
        serde_json::to_string(&projected).unwrap(),
        serde_json::to_string(&golden_block()).unwrap()
    );
}

#[test]
fn build_weights_become_speed_weights_and_planning_becomes_the_planner() {
    let mut nodes = golden_nodes();
    nodes.defs.push(NodeDef {
        id: "local-27b".into(),
        name: "27B · this Mac".into(),
        model: Some("Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".into()),
        ..flash_here("local-27b")
    });
    let mut s = strategy(
        "s",
        None,
        Some(entry(
            &[("or-sonnet", 3), ("local-27b", 5)],
            NodeWhen::Share,
        )),
    );
    s.roles.planning = Some(entry(&[("local-27b", 1)], NodeWhen::Failover));
    nodes.strategies.push(s);
    let block = project(&inputs(&nodes), "s").unwrap();
    let devices = block["devices"].as_array().unwrap();
    let by_id = |id: &str| devices.iter().find(|d| d["id"] == id).unwrap().clone();
    assert_eq!(by_id("or-sonnet")["speed_weight"], 3);
    assert_eq!(by_id("or-sonnet")["enabled"], true);
    assert_eq!(by_id("local-27b")["speed_weight"], 5);
    assert_eq!(by_id("local-27b")["engine"], "mlx-sidecar");
    assert_eq!(
        by_id("local-27b")["model_id"],
        "mihai-qwen3.8-27b-atlassian-q8-mlx"
    );
    assert_eq!(
        by_id("mihai-mlx")["enabled"],
        false,
        "other sidecars are off"
    );
    assert_eq!(by_id("old-cloud")["enabled"], false);
    assert_eq!(
        by_id("gabee"),
        golden_block()["devices"][1],
        "LM Studio untouched"
    );
    assert_eq!(block["planner_model"], "mihai-qwen3.8-27b-atlassian-q8-mlx");
    assert_eq!(block["future_field"], json!({"kept": true}));
    assert_eq!(block["speed_weights"], golden_block()["speed_weights"]);
}

#[test]
fn failover_build_chains_prefer_the_first_entry() {
    let mut nodes = golden_nodes();
    let mut s = strategy(
        "f",
        None,
        Some(entry(
            &[("mihai-mlx", 1), ("or-sonnet", 1)],
            NodeWhen::Failover,
        )),
    );
    s.roles.planning = Some(entry(&[("mihai-mlx", 1)], NodeWhen::Failover));
    nodes.strategies.push(s);
    let block = project(&inputs(&nodes), "f").unwrap();
    assert_eq!(block["devices"][0]["speed_weight"], 2);
    assert_eq!(block["devices"][2]["speed_weight"], 1);
}

#[test]
fn every_tier_a_refusal_carries_its_reason() {
    let mut nodes = golden_nodes();
    nodes.defs.push(split_27b("split"));
    nodes.defs.push(NodeDef {
        name: "Flash · Studio".into(),
        placement: Some(NodePlacement::Single {
            macs: vec!["link:studio".into()],
            link: None,
        }),
        ..flash_here("remote")
    });
    nodes.defs.push(flash_here("flash"));
    nodes.defs.push(sonnet("sonnet"));
    let cases = [
        ("split", "split", "is a split"),
        ("remote", "remote", "runs on link:studio"),
        (
            "flash",
            "flash",
            "run Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx",
        ),
    ];
    for (id, node, words) in cases {
        let mut s = strategy(id, None, Some(entry(&[(node, 1)], NodeWhen::Failover)));
        s.roles.planning = Some(entry(&[("mihai-mlx", 1)], NodeWhen::Failover));
        nodes.strategies.push(s);
        let reasons = build_eligibility(&inputs(&nodes), id).unwrap_err();
        assert!(
            reasons.iter().any(|r| r.message.contains(words)),
            "{id}: {reasons:?}"
        );
    }
    let mut cloud_planner = strategy(
        "cp",
        None,
        Some(entry(&[("mihai-mlx", 1)], NodeWhen::Failover)),
    );
    cloud_planner.roles.planning = Some(entry(&[("sonnet", 1)], NodeWhen::Failover));
    nodes.strategies.push(cloud_planner);
    let reasons = build_eligibility(&inputs(&nodes), "cp").unwrap_err();
    assert!(matches!(
        reasons[0].reason,
        BuildRefusal::CloudPlanner { .. }
    ));
    assert!(reasons[0].message.contains("LM Studio"));
    // No model configured on this Mac's engine: named, never assumed.
    let mut no_model = inputs(&nodes);
    no_model.engine = Ok(goose_sidecar::engine::EngineSettings::default());
    let reasons = build_eligibility(&no_model, "flash").unwrap_err();
    assert!(
        reasons[0].message.contains("no model configured"),
        "{reasons:?}"
    );
}

/// Q-311: the editor asks about the strategy as it holds it, before Save — the draft replaces the
/// stored strategy of that id (or joins as a new one), and the answer is the draft's.
#[test]
fn an_unsaved_draft_is_checked_in_place_of_the_stored_strategy() {
    let mut nodes = golden_nodes();
    nodes.defs.push(split_27b("split"));
    let stored_ok = build_eligibility(&inputs(&nodes), "golden");
    assert!(stored_ok.is_ok());

    let mut draft = strategy(
        "ignored-id",
        None,
        Some(entry(&[("split", 1)], NodeWhen::Failover)),
    );
    draft.roles.planning = Some(entry(&[("mihai-mlx", 1)], NodeWhen::Failover));
    let mut edited = nodes.clone();
    with_draft_strategy(&mut edited, "golden", draft.clone());
    assert_eq!(edited.strategies.len(), nodes.strategies.len(), "replaced");
    let reasons = build_eligibility(&inputs(&edited), "golden").unwrap_err();
    assert!(
        reasons.iter().any(|r| r.message.contains("is a split")),
        "{reasons:?}"
    );

    let mut added = nodes.clone();
    with_draft_strategy(&mut added, "brand-new", draft);
    assert_eq!(
        added.strategies.len(),
        nodes.strategies.len() + 1,
        "appended"
    );
    assert!(build_eligibility(&inputs(&added), "brand-new").is_err());
}

#[test]
fn an_eligible_build_states_what_tier_a_cannot_express() {
    let nodes = golden_nodes();
    let planned = build_eligibility(&inputs(&nodes), "golden").unwrap();
    assert!(planned.notes.iter().any(|n| n.contains("LM Studio")));
    assert!(planned
        .notes
        .iter()
        .any(|n| n.contains("Testing, Frontend and Backend")));
}

// ---------------------------------------------------------------------------------------------
// The seam, residency, served.
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn with_no_loader_installed_a_load_is_refused_by_name() {
    assert!(!seam::loader_installed());
    let answer = seam::ensure_serving(seam::Demand {
        node: split_27b("split"),
        from: seam::DemandFrom::Ui,
        role: None,
        if_serving_other: NodeIfServingOther::TakeOver,
    })
    .await;
    assert_eq!(
        answer,
        NodeEnsureServing::Refused {
            code: NodeLoadRefusalCode::LoaderAbsent,
            reason: "loading nodes is not available in this goose process; start 27B · both Macs in Run it"
                .into(),
            facts: None,
        }
    );
    assert!(seam::in_progress().is_empty());
}

fn resolved(def: NodeDef) -> ResolvedNodeDef {
    resolve_def(&def, &Ok(None), false)
}

fn split_way() -> NodesServingWayDto {
    NodesServingWayDto {
        kind: NodesServingKind::Split,
        macs: Vec::new(),
        link: Some("jaccl".into()),
        model_id: "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".into(),
        served_model_id: "mihai-qwen3.8-27b-atlassian-q8-mlx".into(),
        mac_names: vec!["Mihai Macbook".into(), "Work's Mac Studio".into()],
        load_phase: None,
    }
}

#[test]
fn residency_is_one_way_at_a_time_across_macs() {
    let serving = ServingFacts::Way(split_way());
    assert_eq!(
        residency_of(&resolved(split_27b("split")), &serving, &[]),
        NodeResidency::Serving
    );
    let flash = residency_of(&resolved(flash_here("flash")), &serving, &[]);
    let NodeResidency::NotRunning {
        other_way: Some(words),
    } = flash
    else {
        panic!("{flash:?}")
    };
    assert!(words.contains("split across Mihai Macbook and Work's Mac Studio"));
    assert_eq!(
        residency_of(&resolved(sonnet("sonnet")), &serving, &[]),
        NodeResidency::AlwaysReady
    );
    // A split of the same model over another link is not this way.
    let mut ring = split_27b("ring");
    ring.placement = Some(NodePlacement::Pipeline {
        macs: vec![THIS_MAC.into(), "link:studio".into()],
        link: Some("ring".into()),
    });
    assert!(matches!(
        residency_of(&resolved(ring), &serving, &[]),
        NodeResidency::NotRunning { .. }
    ));
}

#[test]
fn a_remote_single_serves_only_its_peers_node() {
    let way = NodesServingWayDto {
        kind: NodesServingKind::RemoteSingle,
        macs: vec![residency::peer_key("studio")],
        link: None,
        model_id: "rapid-mlx/Qwen3.8-Flash-Next-4bit".into(),
        served_model_id: "rapid-mlx/Qwen3.8-Flash-Next-4bit".into(),
        mac_names: vec!["Work's Mac Studio".into()],
        load_phase: None,
    };
    let serving = ServingFacts::Way(way);
    let remote = NodeDef {
        placement: Some(NodePlacement::Single {
            macs: vec!["link:studio".into()],
            link: None,
        }),
        ..flash_here("remote")
    };
    assert_eq!(
        residency_of(&resolved(remote), &serving, &[]),
        NodeResidency::Serving
    );
    // The same model on this Mac is displaced, never "serving" (the Q-128 class).
    assert!(matches!(
        residency_of(&resolved(flash_here("flash")), &serving, &[]),
        NodeResidency::NotRunning { .. }
    ));
    // A node that follows this Mac's engine is refused by a route to a peer.
    let pool = pool(json!([mlx_device("mihai-mlx", "m")]));
    let follows = adopt(Some(&pool), empty_config(), "Mac")
        .config
        .defs
        .remove(0);
    let follows = resolve_def(&follows, &Ok(Some(pool)), false);
    assert!(matches!(
        residency_of(&follows, &serving, &[]),
        NodeResidency::NotRunning { .. }
    ));
}

/// Q-382: a node the loader loads names the sessions it loads for — whether the loader's own mark
/// or the serving way's phase reports the load.
#[test]
fn a_loading_node_names_the_sessions_it_loads_for() {
    let loader = [seam::LoaderActivity::Loading {
        node: "split".into(),
        phase: None,
        demanded_by: vec!["sub-1".into(), "sub-2".into()],
    }];
    let expected = |phase: Option<&str>| NodeResidency::Loading {
        phase: phase.map(str::to_string),
        demanded_by: vec!["sub-1".into(), "sub-2".into()],
    };
    assert_eq!(
        residency_of(
            &resolved(split_27b("split")),
            &ServingFacts::Nothing,
            &loader
        ),
        expected(None)
    );
    let mut loading = split_way();
    loading.load_phase = Some("loading".into());
    assert_eq!(
        residency_of(
            &resolved(split_27b("split")),
            &ServingFacts::Way(loading),
            &loader
        ),
        expected(Some("loading"))
    );
}

#[test]
fn a_loading_way_and_an_unknown_record_are_named() {
    let mut loading = split_way();
    loading.load_phase = Some("warming".into());
    assert_eq!(
        residency_of(
            &resolved(split_27b("split")),
            &ServingFacts::Way(loading),
            &[]
        ),
        NodeResidency::Loading {
            phase: Some("warming".into()),
            demanded_by: Vec::new(),
        }
    );
    let unknown = ServingFacts::Unknown("the route record is unreadable".into());
    assert_eq!(
        residency_of(&resolved(split_27b("split")), &unknown, &[]),
        NodeResidency::Unknown {
            reason: "the route record is unreadable".into()
        }
    );
    let replies = goose_sdk_types::custom_requests::NodeRepliesWaitDto {
        way: "the split across your Macs".into(),
        way_nodes: vec!["split".into()],
        count: 1,
        chats: vec!["Kickoff notes".into()],
    };
    let waiting = [seam::LoaderActivity::Waiting {
        node: "flash".into(),
        reason: "27B is answering 1".into(),
        replies: Some(replies.clone()),
        serving_other: None,
    }];
    assert_eq!(
        residency_of(
            &resolved(flash_here("flash")),
            &ServingFacts::Way(split_way()),
            &waiting
        ),
        NodeResidency::Waiting {
            reason: "27B is answering 1".into(),
            replies: Some(replies),
            serving_other: None,
        }
    );
}

/// Gap 3 (S3b): residency names the split by the way its owner published (`read_way`) — its Macs,
/// not only its model and link — so a split node over other Macs is not "serving".
#[test]
fn the_split_is_named_by_the_way_its_owner_published() {
    use goose_sdk_types::custom_requests::{MlxPlacementKeyDto, MlxPlacementKindDto};
    let engine = crate::providers::mlx_distributed_owner::PublishedEngine {
        pid: 4242,
        base_url: "http://127.0.0.1:8191".into(),
        served_model_id: "mihai-qwen3.8-27b-atlassian-q8-mlx".into(),
        model_id: "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".into(),
        backend: "jaccl".into(),
        node_names: vec!["Mihai Macbook".into(), "Work's Mac Studio".into()],
    };
    let published = MlxPlacementKeyDto {
        kind: MlxPlacementKindDto::Pipeline,
        nodes: vec![THIS_MAC.into(), "link:studio".into()],
        link: Some("jaccl".into()),
    };
    let serving = residency::split_serving(&engine, Ok(Some(published.clone())), None);
    let ServingFacts::Way(way) = &serving else {
        panic!("{serving:?}")
    };
    assert_eq!(way.macs, vec![THIS_MAC.to_string(), "link:studio".into()]);
    assert_eq!(
        residency_of(&resolved(split_27b("split")), &serving, &[]),
        NodeResidency::Serving
    );
    // The same model and link over another Mac is another way.
    let mut elsewhere = split_27b("elsewhere");
    elsewhere.placement = Some(NodePlacement::Pipeline {
        macs: vec![THIS_MAC.into(), "link:laptop".into()],
        link: Some("jaccl".into()),
    });
    assert!(matches!(
        residency_of(&resolved(elsewhere.clone()), &serving, &[]),
        NodeResidency::NotRunning { .. }
    ));
    // A record from a goose before the way names no Macs: matched on model and link, as before.
    let older = residency::split_serving(&engine, Ok(None), None);
    assert_eq!(older, ServingFacts::Way(split_way()));
    assert_eq!(
        residency_of(&resolved(elsewhere), &older, &[]),
        NodeResidency::Serving
    );
    // A way that cannot be read makes what serves unknown — never guessed.
    assert!(matches!(
        residency::split_serving(&engine, Err("EOF".into()), None),
        ServingFacts::Unknown(reason) if reason.contains("EOF")
    ));
    // Q-271: the owner publishes its record the moment the start is accepted — a split that has
    // not answered yet is LOADING in its phase, never serving, so the loader's line holds.
    let starting = residency::split_serving(&engine, Ok(Some(published)), Some("starting".into()));
    assert_eq!(
        residency_of(&resolved(split_27b("split")), &starting, &[]),
        NodeResidency::Loading {
            phase: Some("starting".into()),
            demanded_by: Vec::new(),
        }
    );
}

/// Q-271: the split's own supervisor decides its readiness — the furthest-behind rank's phase
/// while it starts, serving only once it is ready; a run that ended serves nothing.
#[cfg(unix)]
#[test]
fn a_starting_split_is_never_serving() {
    use goose_sidecar::distributed::supervisor::RunState;
    use residency::{split_readiness_of, SplitReadiness};
    let loading = |p: &str| SplitReadiness::Loading(p.to_string());
    assert_eq!(
        split_readiness_of(RunState::Starting, &[None, None]),
        loading("starting")
    );
    assert_eq!(
        split_readiness_of(RunState::Starting, &[Some("warming"), Some("loading")]),
        loading("loading")
    );
    assert_eq!(
        split_readiness_of(RunState::Starting, &[Some("warming"), Some("ready")]),
        loading("warming")
    );
    assert_eq!(
        split_readiness_of(RunState::Preflight, &[]),
        loading("starting")
    );
    assert_eq!(
        split_readiness_of(RunState::Recovering, &[]),
        loading("recovering")
    );
    assert_eq!(
        split_readiness_of(RunState::Ready, &[None]),
        SplitReadiness::Serving
    );
    assert_eq!(
        split_readiness_of(RunState::Serving, &[None]),
        SplitReadiness::Serving
    );
    for over in [RunState::Failed, RunState::Stopping, RunState::Stopped] {
        assert_eq!(split_readiness_of(over, &[]), SplitReadiness::Over);
    }
}

/// Q-434: a delegate's reply is many model calls, one served record each; only the call that
/// demanded the load measured it. The reply's later calls on that node carry the load, so the last
/// record — the one its card reads — still says "loaded for this delegate in …". Another node's
/// record carries nothing, and a new reply starts clean.
#[test]
fn a_replys_load_rides_its_later_records_on_the_same_node() {
    let call = |node: &str, loaded_ms: Option<u64>| NodeServedTurnDto {
        node: node.into(),
        role: Some(NodeRole::Build),
        rank: 1,
        reason: None,
        tried: Vec::new(),
        loaded_ms,
        at_ms: 1,
        asked_for_this_turn: false,
    };
    let delegate = "served-test-delegate-q434";
    assert_eq!(
        served::remember(delegate, call("split", Some(98_000))).loaded_ms,
        Some(98_000)
    );
    assert_eq!(
        served::remember(delegate, call("split", None)).loaded_ms,
        Some(98_000),
        "the reply's next call on the node it loaded"
    );
    assert_eq!(
        served::remember(delegate, call("sonnet", None)).loaded_ms,
        None,
        "a call on another node loaded nothing"
    );
    assert_eq!(
        served::remember(delegate, call("split", None)).loaded_ms,
        Some(98_000),
        "the reply still paid that load"
    );
    served::reply_began(delegate);
    assert_eq!(
        served::remember(delegate, call("split", None)).loaded_ms,
        None,
        "a new reply found the node loaded"
    );
    let other = "served-test-other-q434";
    assert_eq!(served::remember(other, call("split", None)).loaded_ms, None);
}

#[tokio::test]
async fn the_served_record_is_kept_in_memory_and_in_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = crate::session::SessionManager::new(dir.path().to_path_buf());
    let session = sessions
        .create_session(
            dir.path().to_path_buf(),
            "t".into(),
            crate::session::SessionType::User,
            crate::config::GooseMode::Auto,
        )
        .await
        .unwrap();
    assert_eq!(served::last(&sessions, &session.id).await.unwrap(), None);
    let turn = NodeServedTurnDto {
        node: "sonnet".into(),
        role: Some(NodeRole::Chat),
        rank: 2,
        reason: Some("27B · both Macs can't run".into()),
        tried: Vec::new(),
        loaded_ms: None,
        at_ms: 1,
        serving_other: None,
        asked_for_this_turn: false,
    };
    served::record(&sessions, &session.id, turn.clone())
        .await
        .unwrap();
    assert_eq!(
        served::last(&sessions, &session.id).await.unwrap(),
        Some(turn.clone())
    );
    // Another process (a reload) reads the persisted record.
    let fresh = crate::session::SessionManager::new(dir.path().to_path_buf());
    let persisted = fresh.get_session(&session.id, false).await.unwrap();
    assert_eq!(
        persisted
            .extension_data
            .get_extension_state("nodes", "served")
            .cloned(),
        Some(json!({ "last": serde_json::to_value(&turn).unwrap() }))
    );
}

#[cfg(unix)]
#[test]
fn load_groups_answer_for_the_nodes_own_way_even_before_any_load() {
    use goose_sidecar::placement::loads::{median_for, LoadOutcome, LoadPhasesMs, LoadRecord};
    use goose_sidecar::placement::store::{PlacementKey, PlacementKind};

    let node = resolved(split_27b("split"));
    let groups = acp::load_groups(&node, &[], |_, _| None);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].median_total_ms, None);
    assert_eq!(groups[0].count, 0);

    let key = PlacementKey {
        kind: PlacementKind::Pipeline,
        nodes: vec![THIS_MAC.into(), "link:studio".into()],
        link: Some("jaccl".into()),
    };
    let row = |ms: u64| LoadRecord {
        model: "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".into(),
        placement: key.clone(),
        macs: vec!["Mihai Macbook".into(), "Work's Mac Studio".into()],
        weights_bytes: 1,
        phases_ms: LoadPhasesMs::default(),
        total_ms: ms,
        file_cache_warm: true,
        outcome: LoadOutcome::Ready,
        recorded_at_ms: 1,
    };
    let rows = vec![row(40_000), row(48_000), row(50_000)];
    let groups = acp::load_groups(&node, &rows, |m, k| median_for(&rows, m, k));
    assert_eq!(groups[0].median_total_ms, Some(48_000));
    assert_eq!(groups[0].count, 3);
    // Another way of the same model is not this node's.
    let single = resolved(NodeDef {
        model: Some("Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".into()),
        ..flash_here("single")
    });
    let groups = acp::load_groups(&single, &rows, |m, k| median_for(&rows, m, k));
    assert_eq!(groups[0].count, 0);
}

/// Adoption against a COPY of a real config (never the live one): `NODES_ADOPT_CONFIG=<copy>
/// cargo test -p goose --lib nodes::tests::adopt_a_copy -- --ignored --nocapture`. Prints what
/// `nodes/read` answers and proves the read left the file byte-for-byte.
#[test]
#[ignore]
fn adopt_a_copy_of_a_real_config() {
    let path =
        std::env::var("NODES_ADOPT_CONFIG").expect("NODES_ADOPT_CONFIG=<a copy of config.yaml>");
    let secrets = tempfile::tempdir().unwrap();
    let before = std::fs::read(&path).unwrap();
    let config = Config::new_with_file_secrets(&path, secrets.path().join("secrets.yaml")).unwrap();
    let mac = std::env::var("NODES_ADOPT_MAC").unwrap_or_else(|_| "This Mac".into());
    let read = read(&config, Ok(mac)).unwrap();
    println!("{}", serde_json::to_string_pretty(&read).unwrap());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the read wrote the config"
    );
}
