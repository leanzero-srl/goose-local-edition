use super::*;
use crate::providers::openrouter::openrouter_api_client;
use crate::providers::openrouter_hosts;

impl GooseAcpAgent {
    fn openrouter_client(
        &self,
    ) -> Result<crate::providers::api_client::ApiClient, agent_client_protocol::Error> {
        let config = self.config()?;
        let tls = crate::config::tls::provider_tls_config_from_config(config).internal_err()?;
        openrouter_api_client(config, tls).internal_err_ctx("OpenRouter is not set up")
    }

    pub(super) async fn on_openrouter_pin_read(
        &self,
        _req: OpenRouterPinReadRequest,
    ) -> Result<OpenRouterPinResponse, agent_client_protocol::Error> {
        let pin = openrouter_hosts::read_pin(self.config()?).internal_err()?;
        Ok(OpenRouterPinResponse { pin })
    }

    pub(super) async fn on_openrouter_pin_set(
        &self,
        req: OpenRouterPinSetRequest,
    ) -> Result<OpenRouterPinResponse, agent_client_protocol::Error> {
        let pin =
            openrouter_hosts::write_pin(self.config()?, req.tag.as_deref()).invalid_params_err()?;
        Ok(OpenRouterPinResponse { pin })
    }

    pub(super) async fn on_openrouter_hosts_list(
        &self,
        req: OpenRouterHostsListRequest,
    ) -> Result<OpenRouterHostsListResponse, agent_client_protocol::Error> {
        openrouter_hosts::endpoints_path(&req.model).invalid_params_err()?;
        openrouter_hosts::list_hosts(&self.openrouter_client()?, req.model.trim())
            .await
            .internal_err()
    }

    pub(super) async fn on_openrouter_host_probe(
        &self,
        req: OpenRouterHostProbeRequest,
    ) -> Result<OpenRouterHostProbeResponse, agent_client_protocol::Error> {
        openrouter_hosts::endpoints_path(&req.model).invalid_params_err()?;
        Ok(
            openrouter_hosts::probe_host(&self.openrouter_client()?, req.model.trim(), &req.tag)
                .await,
        )
    }
}
