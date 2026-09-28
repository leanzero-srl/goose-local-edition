//! The Mac-wide half of "the user's turn always wins" (DESIGN-SESSION-LOOPS §5.3 v1b, §5.5; slice
//! L2c), read off the node loader's reply holds — the Mac-wide holder records every goose window
//! publishes (`goose_sidecar::holders`) and this process's own replies. The rule of WHICH replies
//! count lives once, in the loader (`Holds::persons_on`, the reader of the reply's kind); this
//! module only asks it and decides what a loop does with the answer.
//!
//! - **(a) The yield.** A running tick yields only to a PERSON's reply on the tick's OWN way — in
//!   this window or any other goose process on this Mac. A person's reply on another way, on a
//!   cloud or endpoint node, a reply waiting in its loader (it holds nothing), and another loop's
//!   tick never yield it. The tick's way is the one its reply's last MLX lease used, so a tick
//!   that has leased nothing yet shares no way with anyone.
//! - **(b) The check before an offer.** A due tick is not offered while a person's reply holds the
//!   way it would use or stop: a chat whose route names an MLX node (`node:<id>`) waits while a
//!   person's reply holds any way of this Mac's goose — ONE way serves it at a time, so that reply
//!   is on the node's own way (the tick would share the engine) or on the way the tick's load
//!   would stop — with the runner's `WayHeld` status; and a tick that yielded to a person's reply
//!   waits until that reply ends. A strategy's or Auto's chat is not held here: its node is the
//!   router's choice at lease time, and its tick yields at the lease that puts it on a person's
//!   way — (a). A cloud or endpoint node never touches the engine and is never held.
//!
//! This process's replies announce every change (the loader's `changed`); another process's reply
//! ending is announced by the kernel releasing its flock. A reply OPENING in another process
//! announces nothing, so a running tick that holds a way looks at the other processes' records on
//! the loader's own observation cadence (`LOOK_AGAIN`): what the look reads decides, never the
//! cadence, and nothing here carries a seconds value of its own.

use std::sync::Arc;

use async_trait::async_trait;
use futures::future::BoxFuture;
use goose_sdk_types::custom_requests::{
    LoopRecord, LoopStatusReason, LoopTickOutcome, NodeDefKind, ResolvedNodeDef,
};

use crate::nodes::seam::{self, NodeLoader, PersonHold, WayShare};
use crate::nodes::RouteModel;
use crate::session::SessionManager;

/// What the runner reads of this Mac's replies, and the node a chat's tick would load. Live =
/// the installed node loader; the runner's tests supply a fake.
#[async_trait]
pub trait MacWide: Send + Sync {
    /// The way `session`'s own reply holds, and the person's replies on it.
    fn persons_on_way_of(&self, session: &str) -> Result<WayShare, String>;
    /// The person's replies holding any way of this Mac's goose, in any goose process.
    fn persons_on_any_way(&self) -> Result<Vec<PersonHold>, String>;
    /// This process's replies' version, read BEFORE a look, for [`MacWide::changed`] and
    /// [`MacWide::person_ended`].
    fn version(&self) -> u64;
    /// Resolves once this process's replies changed after `since`, or — with `elsewhere` — at the
    /// next look at the other processes' records.
    fn changed(&self, since: u64, elsewhere: bool) -> BoxFuture<'static, ()>;
    /// Resolves once `person`'s reply may have ended.
    fn person_ended(&self, person: &PersonHold, since: u64) -> BoxFuture<'static, ()>;
    /// The MLX node a tick of `session` would load, by name: its route names exactly that node
    /// (`node:<id>` on the swarm provider). `None` for every other route.
    async fn mlx_target(&self, session_id: &str) -> Result<Option<String>, String>;
}

/// Why a due tick is not offered yet, and the person's reply it waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub reason: LoopStatusReason,
    pub person: PersonHold,
    /// This process's replies' version when the hold was read.
    pub since: u64,
}

/// §5.3's check before a tick is offered — and again when it starts, which closes the gap between
/// the offer and the window's submit: (1) a tick that yielded to a person's reply on a way waits
/// until THAT reply ends; (2) a chat whose route names an MLX node waits while a person's reply
/// holds any way of this Mac's goose (`WayHeld`). `Err` = whose replies hold the engine is unknown.
pub async fn offer_check(
    mac: &dyn MacWide,
    sessions: &SessionManager,
    session_id: &str,
    rec: &LoopRecord,
) -> Result<Option<Held>, String> {
    let since = mac.version();
    if let Some(LoopTickOutcome::Yielded {
        to_session,
        to_chat,
        way: Some(_),
    }) = rec.ticks.last().and_then(|t| t.outcome.as_ref())
    {
        if let Some(person) = mac
            .persons_on_any_way()?
            .into_iter()
            .find(|p| &p.session == to_session)
        {
            return Ok(Some(Held {
                reason: LoopStatusReason::UserTurn {
                    session_id: to_session.clone(),
                    chat: to_chat.clone(),
                },
                person,
                since,
            }));
        }
    }
    let target = match mac.mlx_target(session_id).await {
        Ok(target) => target,
        Err(error) => {
            // The tick's own routing reads the same config and ends its turn with the words.
            tracing::warn!(session_id, %error, "loop: the node this chat's tick would use could not be read; the tick is not held for it");
            None
        }
    };
    let Some(target) = target else {
        return Ok(None);
    };
    let Some(person) = mac.persons_on_any_way()?.into_iter().next() else {
        return Ok(None);
    };
    Ok(Some(Held {
        reason: LoopStatusReason::WayHeld {
            node: person.way.clone(),
            chat: chat_name(sessions, &person.session).await,
            target,
        },
        person,
        since,
    }))
}

/// (a): watch a running tick until a person's reply shares its way, and return that reply. Only a
/// look that finds one returns: a tick with no way yet waits on this process's replies changing
/// (its own first lease is such a change); a tick with a way also looks at the other processes'
/// records. A look that cannot read them is named once and yields nothing on a guess.
pub async fn person_on_tick_way(mac: Arc<dyn MacWide>, session: String) -> PersonHold {
    let mut unknown_named = false;
    loop {
        let since = mac.version();
        let elsewhere = match mac.persons_on_way_of(&session) {
            Ok(share) => {
                if let Some(person) = share.persons.into_iter().next() {
                    return person;
                }
                share.way.is_some()
            }
            Err(error) => {
                if !unknown_named {
                    tracing::warn!(session, %error, "loop: whether a person's reply shares this tick's way is unknown; the tick is not yielded on a guess");
                    unknown_named = true;
                }
                true
            }
        };
        mac.changed(since, elsewhere).await;
    }
}

/// A chat by the name the person sees; its id, loudly, when the name cannot be read (a reply of
/// another goose process may answer a chat this process's store has not seen yet).
pub async fn chat_name(sessions: &SessionManager, session_id: &str) -> String {
    match sessions.get_session(session_id, false).await {
        Ok(session) => session.name,
        Err(error) => {
            tracing::warn!(session_id, %error, "loop: a chat's name could not be read; it is named by its id");
            session_id.to_string()
        }
    }
}

/// The MLX node `model` names on the `swarm` provider, by name. A `node:` route names exactly one
/// node; a removed one is `Err` (the router refuses the turn by the same words). Every other route
/// — another provider, Auto, a strategy, a cloud or endpoint node — is `None`.
pub fn mlx_node_named(
    provider: &str,
    model: &str,
    nodes: &[ResolvedNodeDef],
) -> Result<Option<String>, String> {
    if provider != "swarm" {
        return Ok(None);
    }
    let Some(RouteModel::Node { id }) = crate::nodes::parse_route_model(model) else {
        return Ok(None);
    };
    let node = nodes
        .iter()
        .find(|n| n.def.id == id)
        .ok_or_else(|| format!("the node '{id}' was removed"))?;
    Ok((node.def.kind == NodeDefKind::Mlx).then(|| node.def.name.clone()))
}

/// The live view: the node loader installed in this process (looked up at each call — goosed
/// installs it after the runner), and the chat's route read from the session and the nodes config.
pub struct Holders {
    loader: Option<Arc<dyn NodeLoader>>,
    sessions: Arc<SessionManager>,
    #[cfg(test)]
    target: Option<String>,
}

impl Holders {
    pub fn installed(sessions: Arc<SessionManager>) -> Self {
        Self {
            loader: None,
            sessions,
            #[cfg(test)]
            target: None,
        }
    }

    /// Over a given loader, every chat's route naming the MLX node `target` (the loader's tests).
    #[cfg(test)]
    pub fn over(loader: Arc<dyn NodeLoader>, sessions: Arc<SessionManager>, target: &str) -> Self {
        Self {
            loader: Some(loader),
            sessions,
            target: Some(target.to_string()),
        }
    }

    fn loader(&self) -> Result<Arc<dyn NodeLoader>, String> {
        match &self.loader {
            Some(loader) => Ok(loader.clone()),
            None => seam::installed_loader(),
        }
    }

    async fn route_target(&self, session_id: &str) -> Result<Option<String>, String> {
        let session = self
            .sessions
            .get_session(session_id, false)
            .await
            .map_err(|e| format!("the chat could not be read: {e}"))?;
        let config = crate::config::Config::global();
        let provider = match session.provider_name {
            Some(provider) => provider,
            None => config
                .get_goose_provider()
                .map_err(|e| format!("no provider is set: {e}"))?,
        };
        let model = match session.model_config {
            Some(model) => model.model_name,
            None => config
                .get_goose_model()
                .map_err(|e| format!("no model is set: {e}"))?,
        };
        if provider != "swarm" {
            return Ok(None);
        }
        let read = crate::nodes::read(config, this_mac_name().await)
            .map_err(|e| format!("the nodes config could not be read: {e:#}"))?;
        mlx_node_named(&provider, &model, &read.nodes)
    }
}

/// This Mac's name, read once per process (the nodes read adopts this Mac's engine node with it,
/// as the router's read does).
async fn this_mac_name() -> Result<String, String> {
    static NAME: tokio::sync::OnceCell<Result<String, String>> = tokio::sync::OnceCell::const_new();
    NAME.get_or_init(crate::nodes::acp::this_mac_name)
        .await
        .clone()
}

#[async_trait]
impl MacWide for Holders {
    fn persons_on_way_of(&self, session: &str) -> Result<WayShare, String> {
        self.loader()?.persons_on_way_of(session)
    }

    fn persons_on_any_way(&self) -> Result<Vec<PersonHold>, String> {
        self.loader()?.persons_on_any_way()
    }

    fn version(&self) -> u64 {
        self.loader().map_or(0, |loader| loader.holds_version())
    }

    fn changed(&self, since: u64, elsewhere: bool) -> BoxFuture<'static, ()> {
        match self.loader() {
            Ok(loader) => Box::pin(async move { loader.holds_changed(since, elsewhere).await }),
            // No loader: nothing records a reply, so nothing will ever change.
            Err(_) => Box::pin(std::future::pending()),
        }
    }

    fn person_ended(&self, person: &PersonHold, since: u64) -> BoxFuture<'static, ()> {
        match self.loader() {
            Ok(loader) => {
                let person = person.clone();
                Box::pin(async move { loader.person_ended(&person, since).await })
            }
            Err(_) => Box::pin(std::future::pending()),
        }
    }

    async fn mlx_target(&self, session_id: &str) -> Result<Option<String>, String> {
        #[cfg(test)]
        if let Some(target) = &self.target {
            return Ok(Some(target.clone()));
        }
        self.route_target(session_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_sdk_types::custom_requests::{NodeDef, NodeModelFrom, NodeOrigin, NodePlacement};

    fn node(id: &str, name: &str, kind: NodeDefKind) -> ResolvedNodeDef {
        ResolvedNodeDef {
            def: NodeDef {
                id: id.into(),
                name: name.into(),
                kind,
                model: Some("m".into()),
                placement: (kind == NodeDefKind::Mlx).then_some(NodePlacement::Follows),
                goal: None,
                provider: None,
                keep_loaded: false,
                pool_device: None,
                origin: NodeOrigin::User,
            },
            model: Some("m".into()),
            provider: None,
            model_from: NodeModelFrom::Own,
            pending_adoption: false,
        }
    }

    /// (b)'s route: only a `node:` route to an MLX node names what a tick would load; a strategy,
    /// Auto, a cloud or endpoint node, and any other provider are not held for a person's reply.
    #[test]
    fn only_a_node_route_to_an_mlx_node_names_a_target() {
        let nodes = vec![
            node("q27", "Qwen 27B", NodeDefKind::Mlx),
            node("sonnet", "Sonnet", NodeDefKind::Cloud),
            node("studio", "LM box", NodeDefKind::Endpoint),
        ];
        let target = |provider: &str, model: &str| mlx_node_named(provider, model, &nodes);
        assert_eq!(target("swarm", "node:q27"), Ok(Some("Qwen 27B".into())));
        assert_eq!(target("swarm", "node:sonnet"), Ok(None));
        assert_eq!(target("swarm", "node:studio"), Ok(None));
        assert_eq!(target("swarm", "strategy:everyday"), Ok(None));
        assert_eq!(target("swarm", "swarm"), Ok(None));
        assert_eq!(target("anthropic", "node:q27"), Ok(None));
        assert_eq!(
            target("swarm", "node:gone"),
            Err("the node 'gone' was removed".into())
        );
    }
}
