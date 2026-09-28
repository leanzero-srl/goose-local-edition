//! The loader's races, on a scripted engine (`Fake`): what a demand waits for, what wakes it, what
//! it stops and in what order, and what a cancel or a failure leaves behind. Each wait is bounded
//! by the test's own `settle` so a deadlock fails the test instead of hanging it.

use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use goose_sdk_types::custom_requests::{NodeDef, NodeModelFrom, NodeOrigin, NodePlacement};
use goose_sidecar::placement::loads::LoadOutcome;

use super::switch::{DistributedFacts, RemoteFacts, SingleFacts, WayKind};
use super::*;

/// How long a test lets the loader get where it is going before it calls the state a hang. A
/// test harness bound on the TEST, never on the product: the loader under test has no clock.
const SETTLE: Duration = Duration::from_secs(5);

struct NodeSpec {
    way: WayRef,
    model: String,
}

#[derive(Default)]
struct Fake {
    nodes: HashMap<String, NodeSpec>,
    serving: StdMutex<Option<String>>,
    log: StdMutex<Vec<String>>,
    kept: StdMutex<Option<String>>,
    refuse_prepare: StdMutex<Option<Refusal>>,
    fail_start: StdMutex<Option<String>>,
    /// A start that waits here until the test releases it.
    start_gate: StdMutex<Option<Arc<Notify>>>,
    prepares: StdMutex<usize>,
}

impl Fake {
    fn with(nodes: &[(&str, WayRef, &str)], serving: Option<&str>) -> Arc<Self> {
        Arc::new(Fake {
            nodes: nodes
                .iter()
                .map(|(id, way, model)| {
                    (
                        id.to_string(),
                        NodeSpec {
                            way: way.clone(),
                            model: model.to_string(),
                        },
                    )
                })
                .collect(),
            serving: StdMutex::new(serving.map(str::to_string)),
            ..Default::default()
        })
    }

    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn serving(&self) -> Option<String> {
        self.serving.lock().unwrap().clone()
    }
}

fn placement_of(way: &WayRef) -> NodePlacement {
    match way.kind {
        WayKind::Local => NodePlacement::Single {
            macs: vec!["local".into()],
            link: None,
        },
        WayKind::Peer => NodePlacement::Single {
            macs: vec![format!("link:{}", way.peer.clone().unwrap())],
            link: None,
        },
        WayKind::Split => NodePlacement::Pipeline {
            macs: vec!["local".into(), "link:wh".into()],
            link: Some("jaccl".into()),
        },
    }
}

fn def(id: &str, fake: &Fake) -> NodeDef {
    let spec = &fake.nodes[id];
    NodeDef {
        id: id.to_string(),
        name: format!("{id} node"),
        kind: NodeDefKind::Mlx,
        model: Some(spec.model.clone()),
        placement: Some(placement_of(&spec.way)),
        goal: None,
        provider: None,
        keep_loaded: false,
        pool_device: None,
        origin: NodeOrigin::User,
    }
}

#[async_trait]
impl Ways for Fake {
    async fn resolve(&self, node: &NodeDef) -> Result<ResolvedNodeDef, Refusal> {
        Ok(ResolvedNodeDef {
            def: node.clone(),
            model: node.model.clone(),
            provider: None,
            model_from: NodeModelFrom::Own,
            pending_adoption: false,
        })
    }

    async fn residency(&self, node: &ResolvedNodeDef) -> Result<Residency, Refusal> {
        Ok(if self.serving().as_deref() == Some(node.def.id.as_str()) {
            Residency::Serving
        } else {
            Residency::NotServing
        })
    }

    async fn serving(&self) -> Result<Serving, Refusal> {
        let Some(id) = self.serving() else {
            return Ok(Serving::default());
        };
        let spec = &self.nodes[&id];
        let model_id = Some(spec.model.clone());
        Ok(match spec.way.kind {
            WayKind::Local => Serving {
                single: Some(SingleFacts {
                    state: "running".into(),
                    model_id,
                }),
                ..Default::default()
            },
            WayKind::Peer => Serving {
                remote: Some(RemoteFacts {
                    state: "ready".into(),
                    peer: spec.way.peer.clone(),
                    model_id,
                }),
                ..Default::default()
            },
            WayKind::Split => Serving {
                distributed: Some(DistributedFacts {
                    state: "serving".into(),
                    mode: "distributed".into(),
                    model_id,
                }),
                ..Default::default()
            },
        })
    }

    async fn kept_loaded(&self, _stop: &Stop) -> Result<Option<String>, Refusal> {
        Ok(self.kept.lock().unwrap().clone())
    }

    async fn prepare(
        &self,
        node: &ResolvedNodeDef,
        _target: &WayRef,
        _plan: &SwitchPlan,
    ) -> Result<Prepared, Refusal> {
        *self.prepares.lock().unwrap() += 1;
        if let Some(refusal) = self.refuse_prepare.lock().unwrap().clone() {
            return Err(refusal);
        }
        let (_, key) = target_of(node)?;
        Ok(Prepared {
            model: node.model.clone().unwrap(),
            key,
            start: Start::MountHere,
        })
    }

    async fn stop(&self, stop: &Stop, _plan: &SwitchPlan) -> Result<(), String> {
        self.log
            .lock()
            .unwrap()
            .push(format!("stop {:?} {}", stop.way.kind, stop.model_id));
        *self.serving.lock().unwrap() = None;
        Ok(())
    }

    async fn start(&self, node: &ResolvedNodeDef, prepared: &Prepared) -> Result<(), String> {
        self.log
            .lock()
            .unwrap()
            .push(format!("start {}", node.def.id));
        let gate = self.start_gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.notified().await;
        }
        if let Some(words) = self.fail_start.lock().unwrap().clone() {
            let _ = rows::outcome_of(&prepared.model, &prepared.key, &Err(words.clone()));
            return Err(words);
        }
        *self.serving.lock().unwrap() = Some(node.def.id.clone());
        Ok(())
    }

    async fn unexplained_requests(&self) -> Result<Option<u32>, String> {
        Ok(None)
    }

    async fn chat_name(&self, session_id: &str) -> Result<String, String> {
        Ok(format!("Chat {session_id}"))
    }
}

fn demand(fake: &Fake, node: &str, session: Option<&str>) -> Demand {
    Demand {
        node: def(node, fake),
        from: session.map_or(DemandFrom::Ui, |s| DemandFrom::Turn(s.to_string())),
        role: None,
    }
}

fn lease(core: &Core, session: &str, fake: &Fake, node: &str) {
    let (_, key) = target_of(&ResolvedNodeDef {
        def: def(node, fake),
        model: None,
        provider: None,
        model_from: NodeModelFrom::Own,
        pending_adoption: false,
    })
    .unwrap();
    core.holds().note_lease(session, key);
}

async fn until(what: &str, cond: impl Fn() -> bool) {
    tokio::time::timeout(SETTLE, async {
        while !cond() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("never reached: {what}"));
}

async fn answer(task: tokio::task::JoinHandle<NodeEnsureServing>) -> NodeEnsureServing {
    tokio::time::timeout(SETTLE, task)
        .await
        .expect("the demand settled")
        .unwrap()
}

fn waiting(core: &Core, node: &str) -> Option<String> {
    core.in_progress().into_iter().find_map(|a| match a {
        LoaderActivity::Waiting { node: n, reason } if n == node => Some(reason),
        _ => None,
    })
}

fn flash_and_split() -> Arc<Fake> {
    Fake::with(
        &[
            (
                "flash",
                WayRef::local(),
                "rapid-mlx/Qwen3.8-Flash-Next-4bit",
            ),
            (
                "split",
                WayRef::split(),
                "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx",
            ),
            (
                "studio",
                WayRef::peer("wh"),
                "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx",
            ),
        ],
        Some("flash"),
    )
}

#[tokio::test]
async fn a_served_node_is_ready_and_nothing_stops() {
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let _reply = core.holds().open_reply("chat-1");
    let got = core
        .ensure_serving(demand(&fake, "flash", Some("chat-1")))
        .await;
    assert_eq!(got, NodeEnsureServing::Ready);
    assert!(fake.log().is_empty());
}

/// §13 item 3: the batching unit is a REPLY. Two chats alternate on two ways, each in a tool loop
/// (several model calls per reply): the swaps equal the reply alternations — never one per call —
/// and no reply is stopped mid-way.
#[tokio::test]
async fn two_chats_in_tool_loops_swap_once_per_reply_never_per_call() {
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);

    // Chat 1's reply is on Flash, several calls in.
    let reply_1 = core.holds().open_reply("chat-1");
    lease(&core, "chat-1", &fake, "flash");
    // Chat 2's reply wants the split: it waits for chat 1's reply.
    let reply_2 = core.holds().open_reply("chat-2");
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("chat-2"));
    let chat_2 = tokio::spawn(async move { c.ensure_serving(d).await });
    until("chat 2 waits", || waiting(&core, "split").is_some()).await;
    assert!(
        waiting(&core, "split")
            .unwrap()
            .contains("is answering 1 reply"),
        "{:?}",
        waiting(&core, "split")
    );
    // Chat 1's second and third calls of the same reply: served on Flash, no swap.
    for _ in 0..2 {
        let got = core
            .ensure_serving(demand(&fake, "flash", Some("chat-1")))
            .await;
        assert_eq!(got, NodeEnsureServing::Ready);
        lease(&core, "chat-1", &fake, "flash");
    }
    assert!(fake.log().is_empty(), "no swap mid-reply: {:?}", fake.log());

    // Chat 1's reply ends: chat 2's switch runs.
    drop(reply_1);
    assert_eq!(answer(chat_2).await, NodeEnsureServing::Ready);
    lease(&core, "chat-2", &fake, "split");
    assert_eq!(
        fake.log(),
        vec![
            "stop Local rapid-mlx/Qwen3.8-Flash-Next-4bit".to_string(),
            "start split".to_string()
        ]
    );

    // Chat 1's next reply wants Flash back: it waits for chat 2's reply, whose later calls are served.
    let reply_1b = core.holds().open_reply("chat-1");
    let c = Arc::clone(&core);
    let d = demand(&fake, "flash", Some("chat-1"));
    let chat_1 = tokio::spawn(async move { c.ensure_serving(d).await });
    until("chat 1 waits", || waiting(&core, "flash").is_some()).await;
    for _ in 0..3 {
        assert_eq!(
            core.ensure_serving(demand(&fake, "split", Some("chat-2")))
                .await,
            NodeEnsureServing::Ready
        );
    }
    assert_eq!(fake.log().len(), 2, "still one swap: {:?}", fake.log());
    drop(reply_2);
    assert_eq!(answer(chat_1).await, NodeEnsureServing::Ready);
    assert_eq!(fake.log().len(), 4, "two reply alternations, two swaps");
    assert_eq!(fake.serving().as_deref(), Some("flash"));
    drop(reply_1b);
}

/// A delegate's demand is its parent reply's own: the parent's hold yields (the parent is blocked
/// inside the tool call — waiting on it would deadlock), and the parent's next call swaps back.
#[tokio::test]
async fn a_delegates_demand_is_its_parents_own_and_does_not_deadlock() {
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let _parent = core.holds().open_reply("parent");
    lease(&core, "parent", &fake, "flash");
    core.holds().note_child("delegate", "parent");
    let got = tokio::time::timeout(
        SETTLE,
        core.ensure_serving(demand(&fake, "split", Some("delegate"))),
    )
    .await
    .expect("a delegate never waits on its own parent");
    assert_eq!(got, NodeEnsureServing::Ready);
    lease(&core, "delegate", &fake, "split");
    assert_eq!(
        core.holds().reply("parent").unwrap().way.unwrap().kind,
        goose_sidecar::placement::store::PlacementKind::Pipeline,
        "the delegate's lease is held by its parent's reply"
    );
    let back = tokio::time::timeout(
        SETTLE,
        core.ensure_serving(demand(&fake, "flash", Some("parent"))),
    )
    .await
    .expect("the parent's call back never waits on itself");
    assert_eq!(back, NodeEnsureServing::Ready);
    assert_eq!(
        fake.log().len(),
        4,
        "two swaps per delegate call: {:?}",
        fake.log()
    );
}

/// A reply that opens after a queued switch and wants the running way waits behind the switch —
/// new replies never starve it.
#[tokio::test]
async fn a_reply_opened_after_a_queued_demand_waits_behind_it() {
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let reply_1 = core.holds().open_reply("chat-1");
    lease(&core, "chat-1", &fake, "flash");
    let reply_2 = core.holds().open_reply("chat-2");
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("chat-2"));
    let chat_2 = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the switch is queued", || waiting(&core, "split").is_some()).await;

    let _reply_3 = core.holds().open_reply("chat-3");
    let c = Arc::clone(&core);
    let d = demand(&fake, "flash", Some("chat-3"));
    let chat_3 = tokio::spawn(async move { c.ensure_serving(d).await });
    until("chat 3 waits behind the switch", || {
        waiting(&core, "flash").is_some_and(|w| w.contains("waits behind it"))
    })
    .await;
    // Chat 1 (opened before the switch) is still served meanwhile.
    assert_eq!(
        core.ensure_serving(demand(&fake, "flash", Some("chat-1")))
            .await,
        NodeEnsureServing::Ready
    );
    drop(reply_1);
    assert_eq!(answer(chat_2).await, NodeEnsureServing::Ready);
    // Chat 2's reply holds the split it was switched to: chat 3 waits for that reply to end.
    until("chat 3 waits for chat 2's reply", || {
        waiting(&core, "flash").is_some_and(|w| w.contains("is answering 1 reply"))
    })
    .await;
    assert_eq!(fake.log().len(), 2);
    drop(reply_2);
    assert_eq!(answer(chat_3).await, NodeEnsureServing::Ready);
    assert_eq!(
        fake.log(),
        vec![
            "stop Local rapid-mlx/Qwen3.8-Flash-Next-4bit".to_string(),
            "start split".to_string(),
            "stop Split Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string(),
            "start flash".to_string(),
        ]
    );
}

/// §6.4 step 12: a turn cancelled while its demand waits leaves the queue before any stop.
#[tokio::test]
async fn a_cancelled_demand_leaves_the_queue_before_any_stop() {
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let reply_1 = core.holds().open_reply("chat-1");
    lease(&core, "chat-1", &fake, "flash");
    let _reply_2 = core.holds().open_reply("chat-2");
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("chat-2"));
    let chat_2 = tokio::spawn(async move { c.ensure_serving(d).await });
    until("chat 2 waits", || waiting(&core, "split").is_some()).await;
    chat_2.abort();
    let _ = chat_2.await;
    until("the queue is empty", || {
        core.queue.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(
        core.holds().reply("chat-2").unwrap().waiting,
        0,
        "the cancelled demand's reply holds its way again"
    );
    drop(reply_1);
    tokio::task::yield_now().await;
    assert!(fake.log().is_empty(), "nothing stopped: {:?}", fake.log());
    assert_eq!(fake.serving().as_deref(), Some("flash"));
}

/// §6.4 step 12: once the stops began, a cancel lets the swap run to its end — the target stays
/// loaded — and the load is recorded `cancelledAfterStop`.
#[tokio::test]
async fn a_cancel_after_the_stops_began_completes_the_swap_and_records_it() {
    let fake = Fake::with(
        &[
            (
                "flash",
                WayRef::local(),
                "rapid-mlx/Qwen3.8-Flash-Next-4bit",
            ),
            ("split", WayRef::split(), "loader-test/cancel-after-stop"),
        ],
        Some("flash"),
    );
    let gate = Arc::new(Notify::new());
    *fake.start_gate.lock().unwrap() = Some(Arc::clone(&gate));
    let core = Core::new(fake.clone(), None);
    let _reply = core.holds().open_reply("chat-2");
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("chat-2"));
    let chat_2 = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the swap started", || fake.log().len() == 2).await;
    chat_2.abort();
    let _ = chat_2.await;
    gate.notify_one();
    until("the target is loaded anyway", || {
        fake.serving().as_deref() == Some("split")
    })
    .await;
    let spec = &fake.nodes["split"];
    let (_, key) = target_of(&ResolvedNodeDef {
        def: def("split", &fake),
        model: None,
        provider: None,
        model_from: NodeModelFrom::Own,
        pending_adoption: false,
    })
    .unwrap();
    assert_eq!(
        rows::outcome_of(&spec.model, &key, &Ok(())),
        LoadOutcome::CancelledAfterStop,
        "the ready path records the swap as cancelled after its stops"
    );
}

/// §6.4 step 11: a failed load is not restored — the stop set stays stopped — and the turn gets
/// the load's own words.
#[tokio::test]
async fn a_failed_load_is_not_restored_and_the_turn_gets_its_words() {
    let fake = flash_and_split();
    *fake.fail_start.lock().unwrap() = Some("short 1.6 GB on Work's Mac Studio".into());
    let core = Core::new(fake.clone(), None);
    let _reply = core.holds().open_reply("chat-2");
    let got = core
        .ensure_serving(demand(&fake, "split", Some("chat-2")))
        .await;
    match got {
        NodeEnsureServing::Refused { code, reason } => {
            assert_eq!(code, NodeLoadRefusalCode::LoadFailed);
            assert!(
                reason.contains("short 1.6 GB on Work's Mac Studio"),
                "{reason}"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        fake.log(),
        vec![
            "stop Local rapid-mlx/Qwen3.8-Flash-Next-4bit".to_string(),
            "start split".to_string()
        ],
        "nothing started again"
    );
    assert_eq!(fake.serving(), None);
    assert!(matches!(
        core.in_progress().as_slice(),
        [LoaderActivity::RefusedLastTime { .. }]
    ));
}

#[tokio::test]
async fn a_kept_loaded_way_and_a_step_refuse_before_anything_stops() {
    let fake = flash_and_split();
    *fake.kept.lock().unwrap() = Some("flash node is kept loaded on this Mac's engine".into());
    let core = Core::new(fake.clone(), None);
    let got = core.ensure_serving(demand(&fake, "split", Some("s"))).await;
    assert!(
        matches!(&got, NodeEnsureServing::Refused { code: NodeLoadRefusalCode::KeptLoaded, reason } if reason.contains("kept loaded")),
        "{got:?}"
    );
    *fake.kept.lock().unwrap() = None;
    *fake.refuse_prepare.lock().unwrap() = Some(Refusal::new(
        NodeLoadRefusalCode::NeedsStep,
        "Qwen3.8-27B is not on Work’s Mac Studio yet — copy it there, then Run.",
    ));
    let got = core.ensure_serving(demand(&fake, "split", Some("s"))).await;
    assert!(
        matches!(&got, NodeEnsureServing::Refused { code: NodeLoadRefusalCode::NeedsStep, reason } if reason.contains("copy it there")),
        "{got:?}"
    );
    assert!(fake.log().is_empty());
}

/// Two demands that each wait on the other's reply cannot both hold: a reply waiting in the loader
/// holds nothing, so the older switch runs, then the other.
#[tokio::test]
async fn replies_waiting_on_each_other_do_not_deadlock() {
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let reply_1 = core.holds().open_reply("chat-1");
    lease(&core, "chat-1", &fake, "flash");
    let _reply_2 = core.holds().open_reply("chat-2");
    lease(&core, "chat-2", &fake, "flash");
    let (c1, c2) = (Arc::clone(&core), Arc::clone(&core));
    let (d1, d2) = (
        demand(&fake, "split", Some("chat-1")),
        demand(&fake, "studio", Some("chat-2")),
    );
    let one = tokio::spawn(async move { c1.ensure_serving(d1).await });
    until("chat 1 waits on chat 2's reply", || {
        waiting(&core, "split").is_some()
    })
    .await;
    let two = tokio::spawn(async move { c2.ensure_serving(d2).await });
    assert_eq!(answer(one).await, NodeEnsureServing::Ready);
    drop(reply_1);
    assert_eq!(answer(two).await, NodeEnsureServing::Ready);
    assert_eq!(fake.log().len(), 4, "{:?}", fake.log());
    assert_eq!(fake.serving().as_deref(), Some("studio"));
}

/// The prepare (the placement plan: a network probe) runs at the first look and at the look that
/// switches — not on every wake of a wait.
#[tokio::test]
async fn the_plan_is_read_at_the_first_look_and_the_switch_not_on_every_wake() {
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let reply_1 = core.holds().open_reply("chat-1");
    lease(&core, "chat-1", &fake, "flash");
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("chat-2"));
    let chat_2 = tokio::spawn(async move { c.ensure_serving(d).await });
    until("chat 2 waits", || waiting(&core, "split").is_some()).await;
    // Five wakes (another reply opening and ending), each a look while chat 1 still holds Flash.
    for _ in 0..5 {
        drop(core.holds().open_reply("chat-9"));
        tokio::task::yield_now().await;
    }
    assert!(fake.log().is_empty());
    drop(reply_1);
    assert_eq!(answer(chat_2).await, NodeEnsureServing::Ready);
    let prepares = *fake.prepares.lock().unwrap();
    assert!(prepares <= 3, "{prepares} plan reads");
}

/// A demand from the UI (no session) answers at once with the wait, and loads in the background.
#[tokio::test]
async fn a_ui_demand_answers_wait_at_once_and_loads_in_the_background() {
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let reply_1 = core.holds().open_reply("chat-1");
    lease(&core, "chat-1", &fake, "flash");
    let got = tokio::time::timeout(SETTLE, core.ensure_serving(demand(&fake, "split", None)))
        .await
        .unwrap();
    assert!(matches!(got, NodeEnsureServing::Wait { .. }), "{got:?}");
    drop(reply_1);
    until("the background load ran", || {
        fake.serving().as_deref() == Some("split")
    })
    .await;
}

/// A reply open in ANOTHER goose process (a stand-in holding the same records and flocks): the
/// demand waits on it, is woken by the kernel when that reply ends, and only then stops the way.
#[tokio::test]
async fn an_open_reply_in_another_process_holds_the_way_until_it_ends() {
    let dir = tempfile::tempdir().unwrap();
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), Some(dir.path().to_path_buf()));
    let script = r#"
import fcntl, json, os, sys
d = sys.argv[1]
pid = os.getpid()
stem = '%d-0' % pid
me = open(os.path.join(d, stem + '.lock'), 'a+'); fcntl.flock(me, fcntl.LOCK_EX)
reply = open(os.path.join(d, stem + '-r1.lock'), 'a+'); fcntl.flock(reply, fcntl.LOCK_EX)
record = {'pid': pid, 'startedAt': 0, 'since': 0, 'kind': 'goosed', 'replies': [
    {'reply': 1, 'session': 'other-window-chat', 'rootSession': 'other-window-chat',
     'way': {'kind': 'single', 'nodes': ['local']}}]}
open(os.path.join(d, stem + '.json.tmp'), 'w').write(json.dumps(record))
os.rename(os.path.join(d, stem + '.json.tmp'), os.path.join(d, stem + '.json'))
print('HELD', flush=True)
sys.stdin.readline()
record['replies'] = []
open(os.path.join(d, stem + '.json.tmp'), 'w').write(json.dumps(record))
os.rename(os.path.join(d, stem + '.json.tmp'), os.path.join(d, stem + '.json'))
os.remove(os.path.join(d, stem + '-r1.lock'))
fcntl.flock(reply, fcntl.LOCK_UN)
print('ENDED', flush=True)
sys.stdin.readline()
"#;
    let mut other = tokio::process::Command::new("/usr/bin/python3")
        .args(["-c", script, dir.path().to_str().unwrap()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let mut out = tokio::io::BufReader::new(other.stdout.take().unwrap()).lines();
    assert_eq!(out.next_line().await.unwrap().as_deref(), Some("HELD"));
    let mut stdin = other.stdin.take().unwrap();

    let _reply = core.holds().open_reply("chat-here");
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("chat-here"));
    let here = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the demand waits on the other window's reply", || {
        waiting(&core, "split").is_some()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        fake.log().is_empty(),
        "nothing stops under another window's reply"
    );

    stdin.write_all(b"\n").await.unwrap();
    assert_eq!(out.next_line().await.unwrap().as_deref(), Some("ENDED"));
    assert_eq!(answer(here).await, NodeEnsureServing::Ready);
    assert_eq!(fake.log().len(), 2, "{:?}", fake.log());
    stdin.write_all(b"\n").await.unwrap();
    other.wait().await.unwrap();
}

/// A swarm build registered as a holder: the loader refuses, by name, and stops nothing.
#[tokio::test]
async fn a_build_holding_the_engine_refuses_and_stops_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), Some(dir.path().to_path_buf()));
    let registration = goose_sidecar::holders::Registration::register(
        dir.path(),
        goose_sidecar::holders::HolderKind::SwarmRun {
            way: goose_sidecar::placement::store::PlacementKey::single("local"),
            model: "rapid-mlx/Qwen3.8-Flash-Next-4bit".into(),
            what: "swarm build run-7 on Qwen3.8-Flash".into(),
        },
    )
    .unwrap();
    // Registered by this test process: the loader reads other processes only, so the record is
    // re-labelled as a live stand-in's (a sleeping child).
    let record = registration.record();
    drop(registration);
    let mut child = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let (started_at, _) = goose_sidecar::machine::process_start(child.id()).unwrap();
    let foreign = goose_sidecar::holders::HolderRecord {
        pid: child.id(),
        started_at,
        ..record
    };
    std::fs::write(
        dir.path()
            .join(format!("{}-{}.json", foreign.pid, foreign.started_at)),
        serde_json::to_string(&foreign).unwrap(),
    )
    .unwrap();
    let got = core.ensure_serving(demand(&fake, "split", Some("s"))).await;
    match got {
        NodeEnsureServing::Refused { code, reason } => {
            assert_eq!(code, NodeLoadRefusalCode::HeldByBuild);
            assert!(reason.contains("swarm build run-7"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    assert!(fake.log().is_empty());
    child.kill().unwrap();
    child.wait().unwrap();
}

/// Another goose window's loader holds the Mac's swap claim: this demand waits for it, then runs.
#[tokio::test]
async fn a_swap_claimed_by_another_loader_is_waited_for() {
    let dir = tempfile::tempdir().unwrap();
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), Some(dir.path().to_path_buf()));
    let goose_sidecar::machine::LoadLockAttempt::Acquired(other) =
        goose_sidecar::holders::try_claim_swap(dir.path(), "the other window switches", "m")
            .unwrap()
    else {
        panic!()
    };
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("s"));
    let here = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the demand waits on the claim", || {
        waiting(&core, "split").is_some_and(|w| w.contains("the other window switches"))
    })
    .await;
    assert!(fake.log().is_empty());
    drop(other);
    assert_eq!(answer(here).await, NodeEnsureServing::Ready);
    assert_eq!(fake.log().len(), 2);
}

/// A demand told Ready streams on the way it was switched to before the router notes its lease: its
/// reply holds that way from the swap's end, so a switch queued behind it never stops the way
/// under the call it is about to make.
#[tokio::test]
async fn the_reply_a_swap_served_holds_the_new_way_before_the_next_switch_looks() {
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let gate = Arc::new(Notify::new());
    *fake.start_gate.lock().unwrap() = Some(Arc::clone(&gate));
    let reply_1 = core.holds().open_reply("chat-1");
    lease(&core, "chat-1", &fake, "flash");
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("chat-1"));
    let first = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the first switch is loading", || fake.log().len() == 2).await;
    let _reply_2 = core.holds().open_reply("chat-2");
    let c = Arc::clone(&core);
    let d = demand(&fake, "studio", Some("chat-2"));
    let second = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the second switch queues", || {
        waiting(&core, "studio").is_some()
    })
    .await;
    *fake.start_gate.lock().unwrap() = None;
    gate.notify_one();
    assert_eq!(answer(first).await, NodeEnsureServing::Ready);
    until("the second switch waits on chat 1's reply", || {
        waiting(&core, "studio").is_some_and(|w| w.contains("is answering 1 reply"))
    })
    .await;
    assert_eq!(
        fake.log().len(),
        2,
        "the split is not stopped under chat 1: {:?}",
        fake.log()
    );
    drop(reply_1);
    assert_eq!(answer(second).await, NodeEnsureServing::Ready);
    assert_eq!(fake.log().len(), 4);
}

/// A reply's pause is a COUNT: its delegate's demand ending never resumes the reply while the
/// parent's own demand still waits in the queue (a flag did, and deadlocked the demand between).
#[tokio::test]
async fn a_delegates_demand_ending_does_not_resume_a_parent_whose_own_demand_waits() {
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let gate = Arc::new(Notify::new());
    *fake.start_gate.lock().unwrap() = Some(Arc::clone(&gate));
    let _parent = core.holds().open_reply("parent");
    lease(&core, "parent", &fake, "flash");
    core.holds().note_child("task", "parent");
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("task"));
    let task = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the delegate's switch is loading", || fake.log().len() == 2).await;
    let reply_x = core.holds().open_reply("chat-x");
    let c = Arc::clone(&core);
    let d = demand(&fake, "studio", Some("chat-x"));
    let chat_x = tokio::spawn(async move { c.ensure_serving(d).await });
    until("chat x queues", || waiting(&core, "studio").is_some()).await;
    let c = Arc::clone(&core);
    let d = demand(&fake, "flash", Some("parent"));
    let parent = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the parent's own demand queues", || {
        waiting(&core, "flash").is_some()
    })
    .await;
    *fake.start_gate.lock().unwrap() = None;
    gate.notify_one();
    assert_eq!(answer(task).await, NodeEnsureServing::Ready);
    assert_eq!(
        answer(chat_x).await,
        NodeEnsureServing::Ready,
        "the parent waits in the queue: it holds nothing chat x must wait for"
    );
    drop(reply_x);
    assert_eq!(answer(parent).await, NodeEnsureServing::Ready);
    assert_eq!(fake.serving().as_deref(), Some("flash"));
}

/// While this loader waited for the swap claim, the other window's loader switched the Mac to
/// another way: the switch stops what serves NOW, never the stop set read before the claim.
#[tokio::test]
async fn the_switch_after_the_claim_stops_what_serves_then_not_what_served_before() {
    let dir = tempfile::tempdir().unwrap();
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), Some(dir.path().to_path_buf()));
    let goose_sidecar::machine::LoadLockAttempt::Acquired(other) =
        goose_sidecar::holders::try_claim_swap(dir.path(), "the other window switches", "m")
            .unwrap()
    else {
        panic!()
    };
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("s"));
    let here = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the demand waits on the claim", || {
        waiting(&core, "split").is_some_and(|w| w.contains("the other window switches"))
    })
    .await;
    *fake.serving.lock().unwrap() = Some("studio".to_string());
    drop(other);
    assert_eq!(answer(here).await, NodeEnsureServing::Ready);
    assert_eq!(
        fake.log(),
        vec![
            "stop Peer Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string(),
            "start split".to_string()
        ]
    );
}

#[tokio::test]
async fn a_node_that_follows_this_mac_is_never_loaded() {
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let mut d = demand(&fake, "split", Some("s"));
    d.node.placement = Some(NodePlacement::Follows);
    let got = core.ensure_serving(d).await;
    assert!(
        matches!(&got, NodeEnsureServing::Refused { code: NodeLoadRefusalCode::NeedsStep, reason } if reason.contains("Run it")),
        "{got:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// S3b (Q-196): the router-side gaps S5's review found.
// ---------------------------------------------------------------------------------------------

/// Gap 1: a BACKGROUND delegate runs beside its parent's turn, so it holds a reply of its own
/// (summon opens it through the seam): its switch waits for the parent's reply instead of stopping
/// the way the parent streams on; the parent waiting on it in `load` holds nothing, so the switch
/// runs; and the delegate keeps its way after the parent's reply ends.
#[tokio::test]
async fn a_background_delegate_holds_its_own_reply_and_never_stops_its_parents_way() {
    use goose_sidecar::placement::store::PlacementKind;
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let seam = Seam(Arc::clone(&core));
    let parent = core.holds().open_reply("parent");
    lease(&core, "parent", &fake, "flash");
    let task_reply = seam.open_reply("task");
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("task"));
    let task = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the task's switch waits for the parent's reply", || {
        waiting(&core, "split").is_some_and(|w| w.contains("is answering 1 reply"))
    })
    .await;
    assert!(
        fake.log().is_empty(),
        "the way the parent streams on is not stopped: {:?}",
        fake.log()
    );

    // The parent waits on the task in `load`: it holds nothing, and the task's switch runs.
    let in_load = seam.pause_reply("parent");
    assert_eq!(answer(task).await, NodeEnsureServing::Ready);
    lease(&core, "task", &fake, "split");
    assert_eq!(
        core.holds().reply("task").unwrap().way.unwrap().kind,
        PlacementKind::Pipeline,
        "the task's lease is held by its own reply"
    );
    assert_eq!(
        core.holds().reply("parent").unwrap().way.unwrap().kind,
        PlacementKind::Single,
        "and never lands on the parent's"
    );
    drop(in_load);

    // The parent's reply ends; the task's does not: a chat that wants Flash waits for the task.
    drop(parent);
    let _chat = core.holds().open_reply("chat");
    let c = Arc::clone(&core);
    let d = demand(&fake, "flash", Some("chat"));
    let chat = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the chat waits for the task's reply", || {
        waiting(&core, "flash").is_some_and(|w| w.contains("is answering 1 reply"))
    })
    .await;
    drop(task_reply);
    assert_eq!(answer(chat).await, NodeEnsureServing::Ready);
    assert_eq!(fake.serving().as_deref(), Some("flash"));
}

/// Gap 2: a reply opened after a queued switch and leasing the way that SERVES never reached the
/// loader; the router now asks before every MLX lease. The answer names the switch for a reply
/// opened after it, and nothing for a reply opened before it or a lease on the switch's own node;
/// the wait holds nothing and ends when the switch leaves the queue.
#[tokio::test]
async fn a_queued_switch_is_honoured_by_the_next_lease_on_the_running_way() {
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let reply_1 = core.holds().open_reply("chat-1");
    lease(&core, "chat-1", &fake, "flash");
    let _reply_2 = core.holds().open_reply("chat-2");
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("chat-2"));
    let chat_2 = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the switch is queued", || waiting(&core, "split").is_some()).await;

    let _reply_3 = core.holds().open_reply("chat-3");
    assert_eq!(
        core.queued_switch_ahead("chat-3", "flash").as_deref(),
        Some("split node"),
        "a reply opened after the switch waits behind it"
    );
    assert_eq!(
        core.queued_switch_ahead("chat-1", "flash"),
        None,
        "a reply opened before it keeps its way for every call"
    );
    assert_eq!(
        core.queued_switch_ahead("chat-3", "split"),
        None,
        "a lease on the switch's own node does not wait for it"
    );
    assert_eq!(
        core.queued_switch_ahead("no-reply", "flash"),
        None,
        "a call outside any reply holds nothing across calls"
    );

    let c = Arc::clone(&core);
    let chat_3 =
        tokio::spawn(async move { c.wait_behind_queued_switches("chat-3", "flash").await });
    until("chat 3 waits, holding nothing", || {
        core.holds().reply("chat-3").is_some_and(|r| r.waiting == 1)
    })
    .await;
    assert!(
        !chat_3.is_finished(),
        "steady replies no longer pass the switch"
    );
    drop(reply_1);
    assert_eq!(answer(chat_2).await, NodeEnsureServing::Ready);
    tokio::time::timeout(SETTLE, chat_3)
        .await
        .expect("the lease goes on once the switch has left the queue")
        .unwrap();
    assert_eq!(core.holds().reply("chat-3").unwrap().waiting, 0);
    assert_eq!(fake.serving().as_deref(), Some("split"));
}

/// Gap 3: a lease on the split, named by the way its owner published (`read_way`, noted through
/// the seam), holds the split: a switch away from it waits for that reply.
#[tokio::test]
async fn a_split_lease_counts_as_holding_the_split() {
    let fake = flash_and_split();
    *fake.serving.lock().unwrap() = Some("split".to_string());
    let core = Core::new(fake.clone(), None);
    let seam = Seam(Arc::clone(&core));
    let published = MlxPlacementKeyDto {
        kind: MlxPlacementKindDto::Tensor,
        nodes: vec!["local".to_string(), "link:wh".to_string()],
        link: Some("jaccl".to_string()),
    };
    let reply_1 = core.holds().open_reply("chat-1");
    seam.note_lease("chat-1", &published);
    let _reply_2 = core.holds().open_reply("chat-2");
    let c = Arc::clone(&core);
    let d = demand(&fake, "flash", Some("chat-2"));
    let chat_2 = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the switch waits for the reply on the split", || {
        waiting(&core, "flash")
            .is_some_and(|w| w.contains("the split across your Macs is answering 1 reply"))
    })
    .await;
    assert!(fake.log().is_empty(), "{:?}", fake.log());
    drop(reply_1);
    assert_eq!(answer(chat_2).await, NodeEnsureServing::Ready);
    assert_eq!(
        fake.log(),
        vec![
            "stop Split Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string(),
            "start flash".to_string()
        ]
    );
}

/// The S5 open risk: only a card's Start is answered `Wait`. A turn's demand is answered once it
/// is settled, however long it waits.
#[tokio::test]
async fn a_turns_demand_is_never_answered_wait() {
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let reply_1 = core.holds().open_reply("chat-1");
    lease(&core, "chat-1", &fake, "flash");
    let _reply_2 = core.holds().open_reply("chat-2");
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("chat-2"));
    let turn = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the turn's demand waits", || {
        waiting(&core, "split").is_some()
    })
    .await;
    assert!(!turn.is_finished(), "a turn is not answered while it waits");
    drop(reply_1);
    assert_eq!(answer(turn).await, NodeEnsureServing::Ready);
}

/// Session loops §5.5 / §13 item 9: a loop's tick opens its reply as kind `tick`, and its demand
/// for another way never stops the way a person's open reply is answering on — it waits, and
/// swaps once that reply ends.
#[tokio::test]
async fn a_ticks_demand_waits_for_the_persons_reply_on_the_way_it_would_stop() {
    use goose_sidecar::holders::ReplyKind;

    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let person = core.holds().open_reply_as("chat-1", ReplyKind::User);
    lease(&core, "chat-1", &fake, "flash");
    let tick = core.holds().open_reply_as("loop-chat", ReplyKind::Tick);
    assert_eq!(
        core.holds().reply("loop-chat").unwrap().kind,
        ReplyKind::Tick
    );
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("loop-chat"));
    let tick_demand = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the tick's demand waits", || {
        waiting(&core, "split").is_some()
    })
    .await;
    assert!(
        fake.log().is_empty(),
        "nothing stops under the person's reply: {:?}",
        fake.log()
    );
    drop(person);
    assert_eq!(answer(tick_demand).await, NodeEnsureServing::Ready);
    assert_eq!(fake.serving().as_deref(), Some("split"));
    drop(tick);
}

/// Session loops §5.5 (L2c): a tick's demand that waits on a PERSON's reply says whose, in the
/// design's words; one that waits on another loop's tick is an ordinary wait.
#[tokio::test]
async fn a_tick_waiting_on_a_persons_reply_names_the_chat_it_waits_for() {
    use goose_sidecar::holders::ReplyKind;

    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let other_loop = core.holds().open_reply_as("other-loop", ReplyKind::Tick);
    lease(&core, "other-loop", &fake, "flash");
    let _tick = core.holds().open_reply_as("loop-chat", ReplyKind::Tick);
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("loop-chat"));
    let tick_demand = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the tick waits on the other loop's tick", || {
        waiting(&core, "split").as_deref()
            == Some("this Mac's engine is answering 1 reply; loading split node when it finishes")
    })
    .await;

    let person = core.holds().open_reply_as("chat-1", ReplyKind::User);
    lease(&core, "chat-1", &fake, "flash");
    drop(other_loop);
    until("the tick names the person's chat", || {
        waiting(&core, "split").as_deref()
            == Some("this Mac's engine is answering you in Chat chat-1; the loop's tick loads split node when it finishes")
    })
    .await;
    assert!(fake.log().is_empty(), "{:?}", fake.log());
    drop(person);
    assert_eq!(answer(tick_demand).await, NodeEnsureServing::Ready);
}

/// Session loops §5.5 (L2c): a person's demand that arrives while a tick's demand waits goes in
/// front of it — the person never waits behind a loop tick's swap.
#[tokio::test]
async fn a_persons_demand_goes_before_a_ticks_queued_demand() {
    use goose_sidecar::holders::ReplyKind;

    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let person_1 = core.holds().open_reply_as("chat-1", ReplyKind::User);
    lease(&core, "chat-1", &fake, "flash");
    let tick = core.holds().open_reply_as("loop-chat", ReplyKind::Tick);
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("loop-chat"));
    let tick_demand = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the tick's demand waits", || {
        waiting(&core, "split").is_some()
    })
    .await;

    let person_2 = core.holds().open_reply_as("chat-2", ReplyKind::User);
    let c = Arc::clone(&core);
    let d = demand(&fake, "studio", Some("chat-2"));
    let person_demand = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the person's demand waits only on the open reply", || {
        waiting(&core, "studio").as_deref()
            == Some("this Mac's engine is answering 1 reply; loading studio node when it finishes")
    })
    .await;
    drop(person_1);
    assert_eq!(answer(person_demand).await, NodeEnsureServing::Ready);
    assert_eq!(
        fake.log(),
        vec![
            "stop Local rapid-mlx/Qwen3.8-Flash-Next-4bit".to_string(),
            "start studio".to_string()
        ],
        "the person's switch ran first"
    );
    // The person's reply now holds the way it was switched to: the tick waits for it.
    until("the tick waits for the person's new reply", || {
        waiting(&core, "split").as_deref()
            == Some("the engine on wh is answering you in Chat chat-2; the loop's tick loads split node when it finishes")
    })
    .await;
    drop(person_2);
    assert_eq!(answer(tick_demand).await, NodeEnsureServing::Ready);
    assert_eq!(fake.serving().as_deref(), Some("split"));
    drop(tick);
}

/// Session loops §5.5 (L2c): a person's reply that opens after a tick's switch was queued is
/// served on the running way at once — it does not wait behind the tick's switch.
#[tokio::test]
async fn a_persons_reply_never_waits_behind_a_ticks_queued_switch() {
    use goose_sidecar::holders::ReplyKind;

    let fake = flash_and_split();
    let core = Core::new(fake.clone(), None);
    let person_1 = core.holds().open_reply_as("chat-1", ReplyKind::User);
    lease(&core, "chat-1", &fake, "flash");
    let _tick = core.holds().open_reply_as("loop-chat", ReplyKind::Tick);
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("loop-chat"));
    let tick_demand = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the tick's switch is queued", || {
        waiting(&core, "split").is_some()
    })
    .await;

    let person_3 = core.holds().open_reply_as("chat-3", ReplyKind::User);
    assert_eq!(core.queued_switch_ahead("chat-3", "flash"), None);
    assert_eq!(
        tokio::time::timeout(
            SETTLE,
            core.ensure_serving(demand(&fake, "flash", Some("chat-3")))
        )
        .await
        .expect("the person's reply is served at once"),
        NodeEnsureServing::Ready
    );
    // A later TICK reply still honours the queued switch (batching per reply).
    let _later_tick = core.holds().open_reply_as("later-loop", ReplyKind::Tick);
    assert_eq!(
        core.queued_switch_ahead("later-loop", "flash").as_deref(),
        Some("split node")
    );
    drop(person_1);
    drop(person_3);
    assert_eq!(answer(tick_demand).await, NodeEnsureServing::Ready);
}

/// §6.4 step 12 holds for a tick too: once a tick's stops have begun, the swap runs to its end and
/// a person's demand that arrives meanwhile waits behind it.
#[tokio::test]
async fn a_ticks_swap_that_has_begun_runs_to_its_end_before_a_persons_demand() {
    use goose_sidecar::holders::ReplyKind;

    let fake = flash_and_split();
    let gate = Arc::new(Notify::new());
    *fake.start_gate.lock().unwrap() = Some(Arc::clone(&gate));
    let core = Core::new(fake.clone(), None);
    let tick = core.holds().open_reply_as("loop-chat", ReplyKind::Tick);
    let c = Arc::clone(&core);
    let d = demand(&fake, "split", Some("loop-chat"));
    let tick_demand = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the tick's swap started", || fake.log().len() == 2).await;

    let _person = core.holds().open_reply_as("chat-2", ReplyKind::User);
    let c = Arc::clone(&core);
    let d = demand(&fake, "studio", Some("chat-2"));
    let person_demand = tokio::spawn(async move { c.ensure_serving(d).await });
    until("the person waits behind the swap under way", || {
        waiting(&core, "studio").as_deref()
            == Some("waiting for the switch to split node first; then studio node")
    })
    .await;
    *fake.start_gate.lock().unwrap() = None;
    gate.notify_one();
    assert_eq!(answer(tick_demand).await, NodeEnsureServing::Ready);
    drop(tick);
    assert_eq!(answer(person_demand).await, NodeEnsureServing::Ready);
    assert_eq!(
        fake.log(),
        vec![
            "stop Local rapid-mlx/Qwen3.8-Flash-Next-4bit".to_string(),
            "start split".to_string(),
            "stop Split Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string(),
            "start studio".to_string(),
        ]
    );
}

/// Q-239, both ends of the holder kind across processes. Publishing: a tick's reply reaches this
/// Mac's holder record as `kind: tick` (what another goose window's loader reads). Reading: a
/// reply ANOTHER process published as a person's makes this process's tick wait naming it; one it
/// published as a tick is an ordinary wait.
#[tokio::test]
async fn the_holder_kind_crosses_processes_both_ways() {
    use goose_sidecar::holders::{self, HolderEntry, HolderKind, ReplyKind};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let dir = tempfile::tempdir().unwrap();
    let fake = flash_and_split();
    let core = Core::new(fake.clone(), Some(dir.path().to_path_buf()));
    let tick = core.holds().open_reply_as("loop-chat", ReplyKind::Tick);
    lease(&core, "loop-chat", &fake, "flash");
    let published: Vec<_> = holders::read_all(dir.path())
        .unwrap()
        .into_iter()
        .filter_map(|e| match e {
            HolderEntry::Live(record) => Some(record),
            _ => None,
        })
        .collect();
    let HolderKind::Goosed { replies } = &published[0].kind else {
        panic!("{published:?}")
    };
    assert_eq!(replies[0].session, "loop-chat");
    assert_eq!(replies[0].kind, ReplyKind::Tick);
    drop(tick);

    for (kind, expected) in [
        (
            "user",
            "this Mac's engine is answering you in Chat other-window-chat; the loop's tick loads split node when it finishes",
        ),
        (
            "tick",
            "this Mac's engine is answering 1 reply; loading split node when it finishes",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let fake = flash_and_split();
        let core = Core::new(fake.clone(), Some(dir.path().to_path_buf()));
        let script = r#"
import fcntl, json, os, sys
d, kind = sys.argv[1], sys.argv[2]
pid = os.getpid()
stem = '%d-0' % pid
me = open(os.path.join(d, stem + '.lock'), 'a+'); fcntl.flock(me, fcntl.LOCK_EX)
reply = open(os.path.join(d, stem + '-r1.lock'), 'a+'); fcntl.flock(reply, fcntl.LOCK_EX)
record = {'pid': pid, 'startedAt': 0, 'since': 0, 'kind': 'goosed', 'replies': [
    {'reply': 1, 'session': 'other-window-chat', 'rootSession': 'other-window-chat',
     'way': {'kind': 'single', 'nodes': ['local']}, 'kind': kind}]}
open(os.path.join(d, stem + '.json.tmp'), 'w').write(json.dumps(record))
os.rename(os.path.join(d, stem + '.json.tmp'), os.path.join(d, stem + '.json'))
print('HELD', flush=True)
sys.stdin.readline()
"#;
        let mut other = tokio::process::Command::new("/usr/bin/python3")
            .args(["-c", script, dir.path().to_str().unwrap(), kind])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut out = tokio::io::BufReader::new(other.stdout.take().unwrap()).lines();
        assert_eq!(out.next_line().await.unwrap().as_deref(), Some("HELD"));

        let _tick = core.holds().open_reply_as("loop-chat", ReplyKind::Tick);
        let c = Arc::clone(&core);
        let d = demand(&fake, "split", Some("loop-chat"));
        let waiter = tokio::spawn(async move { c.ensure_serving(d).await });
        until(&format!("the tick waits on the other window's {kind} reply"), || {
            waiting(&core, "split").as_deref() == Some(expected)
        })
        .await;
        assert!(fake.log().is_empty(), "{:?}", fake.log());
        waiter.abort();
        let mut stdin = other.stdin.take().unwrap();
        stdin.write_all(b"\n").await.unwrap();
        other.wait().await.unwrap();
    }
}
