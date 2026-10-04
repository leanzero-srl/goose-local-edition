//! Per-model custom fields over ACP: list the fields a model takes (from its provider's own
//! metadata, or goose's effort mapping) with their saved values, and save new values.

use goose_providers::model::ModelConfig;
use goose_sdk_types::custom_requests::{
    ModelFieldDto, ModelFieldKindDto, ModelFieldSourceDto, ModelFieldsListRequest,
    ModelFieldsListResponse, ModelFieldsSaveRequest, ModelFieldsSaveResponse,
};

use super::{GooseAcpAgent, ResultExt};
use crate::model_fields::{self, FieldKind, FieldSource, ModelField};

fn field_to_dto(field: ModelField) -> ModelFieldDto {
    ModelFieldDto {
        id: field.id,
        label: field.label,
        description: field.description,
        kind: match field.kind {
            FieldKind::Select { options } => ModelFieldKindDto::Select { options },
            FieldKind::Number { min, max, integer } => {
                ModelFieldKindDto::Number { min, max, integer }
            }
        },
        model_default: field.model_default,
    }
}

fn source_to_dto(source: FieldSource) -> ModelFieldSourceDto {
    match source {
        FieldSource::ProviderMetadata => ModelFieldSourceDto::ProviderMetadata,
        FieldSource::UnlistedModel => ModelFieldSourceDto::UnlistedModel,
        FieldSource::GooseEffort => ModelFieldSourceDto::GooseEffort,
        FieldSource::None => ModelFieldSourceDto::None,
    }
}

impl GooseAcpAgent {
    async fn model_fields_of(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<(Vec<ModelField>, FieldSource), agent_client_protocol::Error> {
        if model_id.trim().is_empty() {
            return Err(agent_client_protocol::Error::invalid_params().data("modelId is empty"));
        }
        let declared = if model_fields::reads_listing(provider_id) {
            let provider = self
                .create_provider(provider_id, Vec::new(), None)
                .await
                .internal_err_ctx("Failed to initialize provider")?;
            provider
                .declared_model_parameters(model_id)
                .await
                .internal_err_ctx("Failed to read the provider's model listing")?
        } else {
            None
        };
        let model = ModelConfig::new(model_id).with_canonical_limits(provider_id);
        Ok(model_fields::fields_for(
            provider_id,
            declared.as_ref(),
            &model,
        ))
    }

    pub(super) async fn on_list_model_fields(
        &self,
        req: ModelFieldsListRequest,
    ) -> Result<ModelFieldsListResponse, agent_client_protocol::Error> {
        let (fields, source) = self
            .model_fields_of(&req.provider_id, &req.model_id)
            .await?;
        let values = model_fields::saved_values(&req.provider_id, &req.model_id).internal_err()?;
        Ok(ModelFieldsListResponse {
            fields: fields.into_iter().map(field_to_dto).collect(),
            source: source_to_dto(source),
            values,
        })
    }

    pub(super) async fn on_save_model_fields(
        &self,
        req: ModelFieldsSaveRequest,
    ) -> Result<ModelFieldsSaveResponse, agent_client_protocol::Error> {
        let (fields, _) = self
            .model_fields_of(&req.provider_id, &req.model_id)
            .await?;
        let values = model_fields::validate(&fields, &req.values).invalid_params_err()?;
        model_fields::save_values(&req.provider_id, &req.model_id, values.clone())
            .internal_err()?;
        Ok(ModelFieldsSaveResponse { values })
    }
}
