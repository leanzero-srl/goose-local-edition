use crate::{
    session::SessionManager,
    work_control::{
        ControllerBackendConfig, CursorPolicy, GooseControllerBackend, OwnerRunRegistry,
    },
};
use async_trait::async_trait;
use leanzero_link::{
    controller::{ControllerBackend, ControllerError},
    controller_host::{ControllerHostFactory, HostResourceSampler, HostWatermark},
    controller_wire::{ControllerScope, HostDescriptor},
};
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;

pub(super) struct GooseHostFactory {
    pub sessions: Arc<SessionManager>,
    pub runs: Arc<OwnerRunRegistry>,
}

#[async_trait]
impl ControllerHostFactory for GooseHostFactory {
    fn directory(&self) -> PathBuf {
        self.sessions.controller_state_dir()
    }

    async fn watermark(&self) -> Result<HostWatermark, ControllerError> {
        self.runs
            .ensure_store()
            .await
            .map_err(|e| ControllerError::unavailable(e.to_string()))?;
        let catalog = self
            .sessions
            .work_catalog()
            .await
            .map_err(|e| ControllerError::unavailable(e.to_string()))?;
        let watermark = catalog
            .watermark()
            .await
            .map_err(|e| ControllerError::unavailable(e.to_string()))?;
        Ok(HostWatermark {
            store_generation: Uuid::parse_str(&watermark.generation)
                .map_err(|e| ControllerError::unavailable(e.to_string()))?,
            catalog_revision: watermark
                .revision
                .try_into()
                .map_err(|_| ControllerError::unavailable("catalog revision is negative"))?,
        })
    }

    async fn backend(
        &self,
        host: HostDescriptor,
        account_id: Uuid,
        resources: HostResourceSampler,
    ) -> Result<Arc<dyn ControllerBackend>, ControllerError> {
        let catalog = self
            .sessions
            .work_catalog()
            .await
            .map_err(|e| ControllerError::unavailable(e.to_string()))?;
        let management = crate::work_control::management::GooseManagement::new(
            &self.runs,
            host.owner_node_id,
            account_id,
            Arc::new(crate::acp::server::GoosedMlxControl::new()),
            Arc::new(super::remote_execution_allowed),
        )
        .await?;
        let backend = GooseControllerBackend::new(
            ControllerBackendConfig {
                host,
                account_id,
                cursor_key_path: self.directory().join("cursor-key.json"),
                policy: CursorPolicy::default(),
            },
            catalog,
            self.runs.clone(),
            Some(Arc::new(move || {
                let sample = resources();
                Box::pin(async move { Ok(sample.await) })
            })),
        )
        .await?
        .with_management(management)
        .with_adapters(Arc::new(crate::work_control::adapters::WorkAdapters::new(
            leanzero_link::jobs::JobRegistry::shared(super::jobs_config()),
            crate::config::paths::Paths::config_dir(),
            crate::work_control::adapters::DesktopBridge::from_env(),
        )))
        .await?
        .with_scope_policy(Arc::new(|scope| {
            !matches!(
                scope,
                ControllerScope::WorkSend
                    | ControllerScope::WorkSteer
                    | ControllerScope::WorkStop
                    | ControllerScope::InputAnswer
            ) || super::remote_execution_allowed()
        }));
        Ok(Arc::new(backend))
    }
}
