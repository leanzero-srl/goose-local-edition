//! Native-only controller facade; the renderer selects immutable identities, not URLs.
use super::*;
use leanzero_link::{
    controller_client::{ClientError, DesktopController},
    controller_wire::*,
    peer_dial::MeshProxy,
};
use std::sync::LazyLock;
use tokio::sync::Mutex;
use uuid::Uuid;

static CONTROLLERS: LazyLock<Mutex<HashMap<PathBuf, Arc<DesktopController>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn visible_host(host: HostDescriptor) -> serde_json::Value {
    serde_json::json!({
        "owner_node_id":host.owner_node_id,"name":host.name,"device_role":host.device_role,
        "protocol_version":host.protocol_version,"availability":host.availability,
        "last_seen_at":host.last_seen_at,"capabilities":host.capabilities,
    })
}

fn visible_catalog(catalog: leanzero_link::controller_client::FleetCatalog) -> serde_json::Value {
    serde_json::json!({"updated_at":catalog.updated_at,"hosts":catalog.hosts.into_iter().map(|h| serde_json::json!({"host":visible_host(h.host),"entries":h.entries,"stale":h.stale,"error":h.error})).collect::<Vec<_>>()})
}

fn rpc_error(error: ClientError) -> agent_client_protocol::Error {
    agent_client_protocol::Error::internal_error().data(serde_json::json!({"error":error}))
}
fn decode<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> Result<T, agent_client_protocol::Error> {
    serde_json::from_value(value).map_err(|_| agent_client_protocol::Error::invalid_params())
}

async fn proxy() -> Option<MeshProxy> {
    let manager = existing_link_manager()?;
    manager.active_registry().await?.peer_proxy()
}

async fn refresh(service: &DesktopController) -> Result<(), ClientError> {
    let account_matches = identity::default_identity_path()
        .and_then(|path| identity::IdentityStore::new(path).load())
        .map_err(|_| ClientError {
            code: "identity_unavailable".into(),
            message: "The private Link account identity cannot be read".into(),
            retryable: true,
            status: 503,
        })?;
    let account = account_matches.ok_or_else(|| ClientError {
        code: "signed_out".into(),
        message: "Link account is not signed in".into(),
        retryable: false,
        status: 401,
    })?;
    if !service
        .matches_account(&account.email, &resolve_worker_base_url())
        .await
    {
        return Err(ClientError {
            code: "account_changed".into(),
            message: "Desktop controller belongs to another Link account".into(),
            retryable: false,
            status: 403,
        });
    }
    let manager = existing_link_manager().ok_or_else(|| ClientError {
        code: "host_unavailable".into(),
        message: "Connect this Mac to Link before enabling its controller".into(),
        retryable: true,
        status: 503,
    })?;
    service.enroll_from_host(&manager).await?;
    service.sync(proxy().await).await?;
    Ok(())
}

impl GooseAcpAgent {
    async fn desktop_controller(
        &self,
    ) -> Result<Arc<DesktopController>, agent_client_protocol::Error> {
        self.link_manager().await?;
        let path = identity::default_identity_path().internal_err()?;
        let identity = identity::IdentityStore::new(path)
            .load()
            .internal_err()?
            .ok_or_else(|| {
                rpc_error(ClientError {
                    code: "signed_out".into(),
                    message: "Sign in to Link on this Mac first".into(),
                    retryable: false,
                    status: 401,
                })
            })?;
        let directory = self
            .session_manager
            .controller_state_dir()
            .with_file_name("controller-desktop");
        let mut controllers = CONTROLLERS.lock().await;
        if let Some(service) = controllers.get(&directory) {
            if !service
                .matches_account(&identity.email, &resolve_worker_base_url())
                .await
            {
                return Err(agent_client_protocol::Error::invalid_params()
                    .data("Desktop controller belongs to another Link account"));
            }
            return Ok(service.clone());
        }
        let service = Arc::new(
            DesktopController::load(
                directory.clone(),
                resolve_worker_base_url(),
                identity.email,
                format!("{} desktop", gethostname::gethostname().to_string_lossy()),
            )
            .map_err(rpc_error)?,
        );
        controllers.insert(directory, service.clone());
        let pump = service.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) = refresh(&pump).await {
                    pump.record_error(&error).await;
                }
                // Metadata refresh cadence. Never bounds model work or accepted commands.
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });
        Ok(service)
    }

    pub(in crate::acp::server) async fn on_controller_call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<ControllerResponse, agent_client_protocol::Error> {
        if method == "status" {
            match identity::default_identity_path()
                .and_then(|path| identity::IdentityStore::new(path).load())
            {
                Ok(None) => {
                    return Ok(ControllerResponse(
                        serde_json::json!({"state":"signed_out","device":null,"error":null}),
                    ))
                }
                Err(_) => {
                    return Ok(ControllerResponse(
                        serde_json::json!({"state":"unavailable","device":null,"error":"The private Link account identity cannot be read"}),
                    ))
                }
                Ok(Some(_)) => {}
            }
        }
        let service = match self.desktop_controller().await {
            Ok(service) => service,
            Err(error) if method == "status" => {
                return Ok(ControllerResponse(
                    serde_json::json!({"state":"unavailable","device":null,"error":error.to_string()}),
                ))
            }
            Err(error) => return Err(error),
        };
        let encode = |value| Ok(ControllerResponse(value));
        match method {
            "status" => encode(serde_json::to_value(service.status().await).internal_err()?),
            "requestCode" => encode(
                serde_json::to_value(service.request_code().await.map_err(rpc_error)?)
                    .internal_err()?,
            ),
            "verify" => {
                let req: ControllerVerifyRequest = decode(params)?;
                service.verify(&req.code).await.map_err(rpc_error)?;
                encode(serde_json::to_value(service.status().await).internal_err()?)
            }
            "logout" => {
                service.logout().await.map_err(rpc_error)?;
                encode(serde_json::to_value(service.status().await).internal_err()?)
            }
            "sync" => {
                refresh(&service).await.map_err(rpc_error)?;
                encode(visible_catalog(service.catalog().await))
            }
            "catalog" => encode(visible_catalog(service.catalog().await)),
            "hosts" => encode(
                serde_json::json!({"hosts":service.catalog().await.hosts.into_iter().map(|h| visible_host(h.host)).collect::<Vec<_>>()}),
            ),
            "openWork" => {
                let req: ControllerOpenWorkRequest = decode(params)?;
                let work: WorkRef = decode(req.work)?;
                let generation: Uuid = decode(serde_json::json!(req.store_generation))?;
                encode(
                    serde_json::to_value(
                        service
                            .open_work(&work, generation, proxy().await)
                            .await
                            .map_err(rpc_error)?,
                    )
                    .internal_err()?,
                )
            }
            "history" => {
                let req: ControllerHistoryRequest = decode(params)?;
                let work: WorkRef = decode(req.work)?;
                let generation: Uuid = decode(serde_json::json!(req.store_generation))?;
                encode(
                    serde_json::to_value(
                        service
                            .history(&work, generation, req.page_token.as_deref(), proxy().await)
                            .await
                            .map_err(rpc_error)?,
                    )
                    .internal_err()?,
                )
            }
            "historyLatest" => {
                let req: ControllerHistoryLatestRequest = decode(params)?;
                let work: WorkRef = decode(req.work)?;
                let generation: Uuid = decode(serde_json::json!(req.store_generation))?;
                encode(
                    serde_json::to_value(
                        service
                            .history_latest(&work, generation, &req.cursor, proxy().await)
                            .await
                            .map_err(rpc_error)?,
                    )
                    .internal_err()?,
                )
            }
            "workChanges" => {
                let req: ControllerWorkChangesRequest = decode(params)?;
                let work: WorkRef = decode(req.work)?;
                let generation: Uuid = decode(serde_json::json!(req.store_generation))?;
                encode(
                    serde_json::to_value(
                        service
                            .work_changes(&work, generation, &req.cursor, proxy().await)
                            .await
                            .map_err(rpc_error)?,
                    )
                    .internal_err()?,
                )
            }
            "command" => {
                let req: ControllerCommandRequest = decode(params)?;
                let command: WorkCommand = decode(req.command)?;
                encode(
                    serde_json::to_value(
                        service
                            .command(command, proxy().await)
                            .await
                            .map_err(rpc_error)?,
                    )
                    .internal_err()?,
                )
            }
            "receipt" => {
                let req: ControllerReceiptRequest = decode(params)?;
                let owner: Uuid = decode(serde_json::json!(req.owner_node_id))?;
                let id: Uuid = decode(serde_json::json!(req.command_id))?;
                encode(
                    serde_json::to_value(
                        service
                            .receipt(owner, id, proxy().await)
                            .await
                            .map_err(rpc_error)?,
                    )
                    .internal_err()?,
                )
            }
            "pendingCommands" => {
                let req: ControllerPendingCommandsRequest = decode(params)?;
                let work: WorkRef = decode(req.work)?;
                encode(
                    serde_json::json!({"commands":service.pending_commands(&work).await.map_err(rpc_error)?}),
                )
            }
            _ => Err(agent_client_protocol::Error::method_not_found()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renderer_host_projection_excludes_destinations_and_missing_work_never_defaults() {
        let owner = Uuid::new_v4();
        let host = HostDescriptor {
            owner_node_id: owner,
            name: "Owner".into(),
            device_role: DeviceRole::Host,
            protocol_version: 1,
            availability: HostAvailability::Online,
            last_seen_at: None,
            endpoints: vec![MeshEndpoint {
                url: "https://100.64.0.1:41227".into(),
                tls_spki_sha256: "a".repeat(64),
            }],
            capabilities: vec![ControllerScope::CatalogRead],
        };
        let visible = visible_host(host);
        assert_eq!(visible["owner_node_id"], owner.to_string());
        assert!(visible.get("endpoints").is_none());
        assert!(!visible.to_string().contains("100.64.0.1"));
        assert!(decode::<WorkRef>(ControllerOpenWorkRequest::default().work).is_err());
        assert!(serde_json::from_value::<ControllerOpenWorkRequest>(
            serde_json::json!({"work":null,"store_generation":"","url":"https://other"})
        )
        .is_err());
    }
}
