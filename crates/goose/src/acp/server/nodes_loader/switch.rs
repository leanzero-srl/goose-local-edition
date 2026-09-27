//! The stop set and stop-first order of a switch (design §6.4 steps 4 and 9): a port of Run it's
//! `servingWays` / `wayServing` (`ui/desktop/src/components/leanzero-swarm/PlacementCard.tsx`).
//! One MLX way serves this Mac's goose at a time, across all Macs, so EVERY way serving now stops
//! before another starts — whichever model it holds (Q-119). `switch.fixture.json` pins the two:
//! this suite runs it here, and `placementSwitch.fixture.test.ts` runs it against the exported
//! `servingWays`. Run it keeps its own choreography in v1 (§11.9); the fixture keeps them one rule.

use goose_sidecar::placement::store::{PlacementKey, PlacementKind};
use serde::{Deserialize, Serialize};

/// This Mac's single engine, as `mlxEngine/status` reports it (the fields the rule reads).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SingleFacts {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

/// The remote-single route, as `mlxEngine/remoteSingleStatus` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFacts {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

/// The split, as `mlxEngine/distributedStatus` reports it (`mode` is the backend's own "which
/// engine owns this Mac").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistributedFacts {
    pub state: String,
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

/// What the three engines report now. An absent engine is one that reported nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Serving {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single: Option<SingleFacts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<RemoteFacts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distributed: Option<DistributedFacts>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WayKind {
    /// This Mac's single engine.
    Local,
    /// A peer's single engine, reached through this Mac's relay.
    Peer,
    /// The distributed engine across Macs.
    Split,
}

/// A way, as Run it names it (`Way.kind` + `Way.peerNodeId`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WayRef {
    pub kind: WayKind,
    /// The peer's Link node id (peer ways only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer: Option<String>,
}

impl WayRef {
    pub fn local() -> Self {
        WayRef {
            kind: WayKind::Local,
            peer: None,
        }
    }

    pub fn peer(peer: &str) -> Self {
        WayRef {
            kind: WayKind::Peer,
            peer: Some(peer.to_string()),
        }
    }

    pub fn split() -> Self {
        WayRef {
            kind: WayKind::Split,
            peer: None,
        }
    }

    /// The way a placement key names: `local` alone is this Mac's single; one other Mac is a
    /// peer's single (`link:<node>` — the planner's key for a Link Mac); more than one is a split.
    pub fn of_key(key: &PlacementKey) -> Option<Self> {
        match (key.kind, key.nodes.as_slice()) {
            (PlacementKind::Single, [mac]) if mac == crate::nodes::THIS_MAC => Some(Self::local()),
            (PlacementKind::Single, [mac]) => Some(Self::peer(peer_of(mac))),
            (PlacementKind::Tensor | PlacementKind::Pipeline, [_, _, ..]) => Some(Self::split()),
            _ => None,
        }
    }

    /// Words for the way ("this Mac", "the Mac <peer>", "the split").
    pub fn words(&self) -> String {
        match self.kind {
            WayKind::Local => "this Mac's engine".to_string(),
            WayKind::Peer => format!(
                "the engine on {}",
                self.peer.as_deref().unwrap_or("a linked Mac")
            ),
            WayKind::Split => "the split across your Macs".to_string(),
        }
    }
}

/// A peer's node id from its placement key (`link:<node>`), or the key as given.
pub fn peer_of(mac: &str) -> &str {
    mac.strip_prefix("link:").unwrap_or(mac)
}

/// A way serving now, with the model it holds: what a switch stops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stop {
    pub way: WayRef,
    pub model_id: String,
}

impl Stop {
    /// Whether a reply whose last model call used `way` holds this stop's way. A split is one way
    /// whatever its Macs (the split's owner record carries no Mac ids): any split key holds it.
    pub fn held_by(&self, way: &PlacementKey) -> bool {
        match (&self.way.kind, WayRef::of_key(way)) {
            (_, None) => false,
            (WayKind::Split, Some(held)) => held.kind == WayKind::Split,
            (_, Some(held)) => held == self.way,
        }
    }
}

/// A switch: every serving way stops, in Run it's order, then the target starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchPlan {
    pub stops: Vec<Stop>,
    /// A switch to the split starts only once a stopped peer has answered its unmount: the split
    /// runs on that Mac too, and its preflight was refused by the route's own load on 3.0.44.
    pub settle_peer_before_start: bool,
    pub start: WayRef,
}

/// `wayServing`: the engine a way IS right now (its state and model), or `None`.
fn way_serving(way: &WayRef, serving: &Serving) -> Option<(String, String)> {
    match way.kind {
        WayKind::Local => {
            let single = serving.single.as_ref()?;
            let model = single.model_id.clone()?;
            matches!(single.state.as_str(), "mounting" | "running" | "failed")
                .then(|| (single.state.clone(), model))
        }
        WayKind::Peer => {
            let remote = serving.remote.as_ref()?;
            if way.peer.is_none() || remote.peer != way.peer {
                return None;
            }
            let model = remote.model_id.clone()?;
            if remote.state == "off" {
                return None;
            }
            let state = if remote.state == "ready" {
                "running".to_string()
            } else {
                remote.state.clone()
            };
            Some((state, model))
        }
        WayKind::Split => {
            let distributed = serving.distributed.as_ref()?;
            let model = distributed.model_id.clone()?;
            let owns_the_mac = distributed.mode == "distributed";
            (owns_the_mac || distributed.state == "failed")
                .then(|| (distributed.state.clone(), model))
        }
    }
}

/// `servingWays`: every way serving now (not failed), whichever model — this Mac's single, the
/// peer the route names, the split, in that order.
pub fn serving_ways(serving: &Serving) -> Vec<Stop> {
    let mut candidates = vec![WayRef::local()];
    if let Some(peer) = serving.remote.as_ref().and_then(|r| r.peer.as_deref()) {
        candidates.push(WayRef::peer(peer));
    }
    candidates.push(WayRef::split());
    candidates
        .into_iter()
        .filter_map(|way| {
            let (state, model_id) = way_serving(&way, serving)?;
            (state != "failed").then_some(Stop { way, model_id })
        })
        .collect()
}

pub fn switch_plan(serving: &Serving, target: &WayRef) -> SwitchPlan {
    let stops = serving_ways(serving);
    let settle_peer_before_start =
        target.kind == WayKind::Split && stops.iter().any(|s| s.way.kind == WayKind::Peer);
    SwitchPlan {
        stops,
        settle_peer_before_start,
        start: target.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Case {
        name: String,
        serving: Serving,
        target: WayRef,
        expect: Expect,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Expect {
        stops: Vec<Stop>,
        settle_peer_before_start: bool,
    }

    #[derive(Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }

    fn fixture() -> Fixture {
        serde_json::from_str(include_str!("switch.fixture.json")).unwrap()
    }

    /// The shared fixture: the TS suite runs the same cases against PlacementCard's exported
    /// `servingWays`, so Run it and the loader stop the same ways in the same order.
    #[test]
    fn the_stop_set_matches_run_its_for_every_fixture_case() {
        let fixture = fixture();
        assert!(fixture.cases.len() >= 12);
        for case in fixture.cases {
            let plan = switch_plan(&case.serving, &case.target);
            assert_eq!(plan.stops, case.expect.stops, "{}", case.name);
            assert_eq!(
                plan.settle_peer_before_start, case.expect.settle_peer_before_start,
                "{}",
                case.name
            );
            assert_eq!(plan.start, case.target, "{}", case.name);
        }
    }

    #[test]
    fn a_remote_single_on_the_studio_stops_for_a_single_here() {
        let serving = Serving {
            remote: Some(RemoteFacts {
                state: "ready".into(),
                peer: Some("wh".into()),
                model_id: Some("m/27b".into()),
            }),
            ..Default::default()
        };
        let plan = switch_plan(&serving, &WayRef::local());
        assert_eq!(
            plan.stops,
            vec![Stop {
                way: WayRef::peer("wh"),
                model_id: "m/27b".into()
            }]
        );
        assert!(!plan.settle_peer_before_start);
    }

    #[test]
    fn a_split_stops_for_a_remote_single() {
        let serving = Serving {
            distributed: Some(DistributedFacts {
                state: "serving".into(),
                mode: "distributed".into(),
                model_id: Some("m/27b".into()),
            }),
            ..Default::default()
        };
        let plan = switch_plan(&serving, &WayRef::peer("wh"));
        assert_eq!(plan.stops[0].way, WayRef::split());
    }

    #[test]
    fn a_reply_holds_the_stop_its_last_lease_used() {
        let local = Stop {
            way: WayRef::local(),
            model_id: "m".into(),
        };
        let peer = Stop {
            way: WayRef::peer("wh"),
            model_id: "m".into(),
        };
        let split = Stop {
            way: WayRef::split(),
            model_id: "m".into(),
        };
        let pipeline = PlacementKey {
            kind: PlacementKind::Pipeline,
            nodes: vec!["local".into(), "link:wh".into()],
            link: Some("jaccl".into()),
        };
        assert!(local.held_by(&PlacementKey::single("local")));
        assert!(!local.held_by(&PlacementKey::single("link:wh")));
        assert!(peer.held_by(&PlacementKey::single("link:wh")));
        assert!(!peer.held_by(&PlacementKey::single("link:other")));
        assert!(split.held_by(&pipeline));
        assert!(!split.held_by(&PlacementKey::single("local")));
    }
}
