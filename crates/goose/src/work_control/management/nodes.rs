use super::*;
use crate::{config::Config, nodes as core, session::SessionManager};
use leanzero_link::controller_management::{ManagedNode, ManagedNodeKind, NodeProviderChoice};
use serde::Deserialize;

fn refused(code: &str) -> ControllerError {
    error(
        StatusCode::CONFLICT,
        code,
        "Compute node request was refused; refresh its definition and usage",
    )
}

fn kind(value: core::NodeDefKind) -> ManagedNodeKind {
    match value {
        core::NodeDefKind::Mlx => ManagedNodeKind::Mlx,
        core::NodeDefKind::Cloud => ManagedNodeKind::Cloud,
        core::NodeDefKind::Endpoint => ManagedNodeKind::Endpoint,
    }
}

fn project(node: core::ResolvedNodeDef) -> Result<ManagedNode, ControllerError> {
    let def = node.def;
    let macs = match &def.placement {
        Some(
            core::NodePlacement::Single { macs, .. }
            | core::NodePlacement::Tensor { macs, .. }
            | core::NodePlacement::Pipeline { macs, .. },
        ) => macs.clone(),
        Some(core::NodePlacement::Follows) => vec![core::THIS_MAC.into()],
        None => vec![],
    };
    Ok(ManagedNode {
        definition_revision: core::definition_revision(&def).map_err(store_error)?,
        editable: def.origin == core::NodeOrigin::User && def.pool_device.is_none(),
        id: def.id,
        name: def.name,
        kind: kind(def.kind),
        model: node.model,
        provider: node.provider,
        macs,
    })
}

async fn provider_choices() -> Vec<NodeProviderChoice> {
    let mut choices = Vec::new();
    for (metadata, provider_type) in crate::providers::providers().await {
        // These internal routing engines are not provider-backed node definitions.
        if ["swarm", "omlx", "lmstudio", "local"].contains(&metadata.name.as_str())
            || !crate::providers::configured::check_provider_configured(&metadata, provider_type)
        {
            continue;
        }
        let mut models: Vec<_> = metadata.known_models.into_iter().map(|m| m.name).collect();
        if !metadata.default_model.is_empty() && !models.contains(&metadata.default_model) {
            models.push(metadata.default_model);
        }
        models.sort();
        models.dedup();
        choices.push(NodeProviderChoice {
            id: metadata.name,
            display_name: metadata.display_name,
            models,
        });
    }
    choices.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    choices
}

fn definition(
    action: ManagementAction,
    providers: &[NodeProviderChoice],
    local_models: &[String],
) -> Result<core::NodeDef, ControllerError> {
    let ManagementAction::NodeCreate {
        id,
        name,
        kind,
        model,
        provider,
    } = action
    else {
        return Err(refused("invalid_node_request"));
    };
    let (node_kind, placement) = match kind {
        ManagedNodeKind::Mlx => {
            if provider.is_some() || !local_models.contains(&model) {
                return Err(refused("node_model_unavailable"));
            }
            (
                core::NodeDefKind::Mlx,
                Some(core::NodePlacement::Single {
                    macs: vec![core::THIS_MAC.into()],
                    link: None,
                }),
            )
        }
        ManagedNodeKind::Cloud | ManagedNodeKind::Endpoint => {
            if !providers
                .iter()
                .any(|p| Some(&p.id) == provider.as_ref() && p.models.contains(&model))
            {
                return Err(refused("node_provider_model_unavailable"));
            }
            (
                if kind == ManagedNodeKind::Cloud {
                    core::NodeDefKind::Cloud
                } else {
                    core::NodeDefKind::Endpoint
                },
                None,
            )
        }
    };
    Ok(core::NodeDef {
        id,
        name,
        kind: node_kind,
        model: Some(model),
        provider,
        placement,
        goal: None,
        keep_loaded: false,
        pool_device: None,
        origin: core::NodeOrigin::User,
    })
}

fn applied(result: core::NodesWriteResponse) -> Result<Option<String>, ControllerError> {
    if result.written {
        return Ok(None);
    }
    // Core refusals can contain chat names/paths; expose stable codes only on the controller wire.
    let code = result
        .refusals
        .first()
        .map(|r| serde_json::to_value(r.code))
        .transpose()
        .map_err(store_error)?
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "node_not_written".into());
    Err(refused(&code))
}

impl GooseManagement {
    async fn local_node_models(&self) -> Result<Vec<String>, ControllerError> {
        #[derive(Deserialize)]
        struct Inventory {
            models: Vec<Model>,
        }
        #[derive(Deserialize)]
        struct Model {
            id: String,
            complete: bool,
        }
        let inventory: Inventory =
            serde_json::from_value(self.mlx(MlxOp::ModelsList, json!({})).await?)
                .map_err(store_error)?;
        Ok(inventory
            .models
            .into_iter()
            .filter(|m| m.complete)
            .map(|m| m.id)
            .collect())
    }
    pub(super) async fn node_inventory(&self) -> Result<ManagedNodes, ControllerError> {
        let read =
            core::read(Config::global(), core::acp::this_mac_name().await).map_err(store_error)?;
        let (local_models, local_models_error) = match self.local_node_models().await {
            Ok(models) => (models, None),
            Err(_) => (Vec::new(), Some("Local model inventory is unavailable on this Mac; cloud and endpoint definitions remain available".into())),
        };
        let mut supported_kinds = vec![ManagedNodeKind::Cloud, ManagedNodeKind::Endpoint];
        if local_models_error.is_none() {
            supported_kinds.push(ManagedNodeKind::Mlx);
        }
        Ok(ManagedNodes {
            nodes: read
                .nodes
                .into_iter()
                .map(project)
                .collect::<Result<_, _>>()?,
            providers: provider_choices().await,
            local_models,
            local_models_error,
            capabilities: if (self.enabled)() {
                vec!["node_create".into(), "node_remove".into()]
            } else {
                vec![]
            },
            supported_kinds,
        })
    }
    async fn authorize_node_commit(
        &self,
        authorization: &DispatchAuthorization,
    ) -> Result<(), ControllerError> {
        // Model inventory/session reads can await. Permission must still hold after that preparation.
        let device = authorization.recheck().await?;
        self.fence().await?;
        self.authenticate(&device, ControllerScope::ModelsManage)
    }
    pub(super) async fn execute_node(
        &self,
        action: ManagementAction,
        authorization: &DispatchAuthorization,
    ) -> Result<Option<String>, ControllerError> {
        let config = Config::global();
        let this_mac = core::acp::this_mac_name().await;
        let engine = core::acp::engine_settings(config);
        let facts = core::WriteFacts {
            this_mac,
            engine: &engine,
        };
        match action {
            ManagementAction::NodeRemove {
                id,
                expected_definition_revision,
            } => {
                let route = core::format_route_model(&core::RouteModel::Node { id: id.clone() });
                let sessions = SessionManager::instance()
                    .list_sessions()
                    .await
                    .map_err(store_error)?;
                let count = sessions
                    .iter()
                    .filter(|s| {
                        s.provider_name.as_deref() == Some(core::SWARM_PROVIDER)
                            && s.model_config
                                .as_ref()
                                .is_some_and(|m| m.model_name == route)
                    })
                    .count();
                let live = u32::try_from(count).map_err(store_error)?;
                self.authorize_node_commit(authorization).await?;
                applied(
                    core::remove_definition(
                        config,
                        &id,
                        &expected_definition_revision,
                        live,
                        facts,
                    )
                    .map_err(store_error)?,
                )
            }
            other => {
                let providers = provider_choices().await;
                let models = if matches!(
                    other,
                    ManagementAction::NodeCreate {
                        kind: ManagedNodeKind::Mlx,
                        ..
                    }
                ) {
                    self.local_node_models().await?
                } else {
                    vec![]
                };
                let def = definition(other, &providers, &models)?;
                self.authorize_node_commit(authorization).await?;
                applied(core::create_definition(config, def, facts).map_err(store_error)?)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn create(kind: ManagedNodeKind, provider: Option<&str>, model: &str) -> ManagementAction {
        ManagementAction::NodeCreate {
            id: "mobile-fixture".into(),
            name: "Mobile fixture".into(),
            kind,
            model: model.into(),
            provider: provider.map(str::to_owned),
        }
    }
    #[test]
    fn node_creation_uses_configured_choices_and_never_loads_or_sets_defaults() {
        let providers = vec![NodeProviderChoice {
            id: "fixture".into(),
            display_name: "Fixture".into(),
            models: vec!["model".into()],
        }];
        for kind in [ManagedNodeKind::Cloud, ManagedNodeKind::Endpoint] {
            let def = definition(create(kind, Some("fixture"), "model"), &providers, &[]).unwrap();
            assert_eq!(def.origin, core::NodeOrigin::User);
            assert!(!def.keep_loaded);
            assert!(def.pool_device.is_none() && def.placement.is_none());
            assert!(definition(create(kind, Some("absent"), "model"), &providers, &[]).is_err());
            assert!(definition(create(kind, Some("fixture"), "absent"), &providers, &[]).is_err());
        }
        let local = definition(
            create(ManagedNodeKind::Mlx, None, "installed"),
            &providers,
            &["installed".into()],
        )
        .unwrap();
        assert_eq!(
            local.placement,
            Some(core::NodePlacement::Single {
                macs: vec!["local".into()],
                link: None
            })
        );
        assert!(!local.keep_loaded);
        assert!(definition(
            create(ManagedNodeKind::Mlx, None, "partial"),
            &providers,
            &["installed".into()]
        )
        .is_err());
        assert!(definition(
            create(ManagedNodeKind::Mlx, Some("fixture"), "installed"),
            &providers,
            &["installed".into()]
        )
        .is_err());
    }
    #[test]
    fn node_payload_refuses_extra_configuration_and_requires_exact_removal_version() {
        let good = json!({"command_id":Uuid::new_v4(),"owner_node_id":Uuid::new_v4(),"expected_store_generation":Uuid::new_v4(),"action":"node_create","payload":{"id":"fixture","name":"Fixture","kind":"mlx","model":"installed"}});
        let cmd: ManagementCommand = serde_json::from_value(good.clone()).unwrap();
        assert_eq!(cmd.action.required_scope(), ControllerScope::ModelsManage);
        for key in [
            "pool_device",
            "keep_loaded",
            "placement",
            "for_new_chats",
            "origin",
        ] {
            let mut bad = good.clone();
            bad["payload"][key] = json!("forbidden");
            assert!(
                serde_json::from_value::<ManagementCommand>(bad).is_err(),
                "{key}"
            );
        }
        let mut remove = good;
        remove["action"] = json!("node_remove");
        remove["payload"] = json!({"id":"fixture"});
        assert!(serde_json::from_value::<ManagementCommand>(remove).is_err());
    }
}
