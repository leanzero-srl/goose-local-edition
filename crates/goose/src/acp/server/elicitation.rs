use std::sync::Arc;

use agent_client_protocol::schema::v1::{
    CreateElicitationRequest, CreateElicitationResponse, ElicitationAction as AcpElicitationAction,
    ElicitationFormMode, ElicitationSchema, ElicitationSessionScope, Meta, SessionId,
    CLIENT_METHOD_NAMES,
};
use agent_client_protocol::{
    Client, ConnectionTo, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, UntypedMessage,
};
use tracing::warn;

use crate::action_required_manager::ElicitationOutcome;
use crate::session::SessionManager;

impl super::GooseAcpAgent {
    pub(super) async fn handle_form_elicitation(
        &self,
        _cx: &ConnectionTo<Client>,
        session_id: &SessionId,
        elicitation_id: &str,
        message: &str,
        requested_schema: &serde_json::Value,
        meta: Meta,
    ) -> Result<(), agent_client_protocol::Error> {
        use super::ResultExt;
        if requested_schema
            .get("url")
            .and_then(|url| url.as_str())
            .is_some()
        {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("owner URL elicitation is unsupported"));
        }
        let schema: ElicitationSchema =
            serde_json::from_value(requested_schema.clone()).invalid_params_err()?;
        let run_id = self
            .owner_runs
            .current_run(session_id.0.as_ref())
            .await
            .ok_or_else(|| {
                agent_client_protocol::Error::internal_error()
                    .data("elicitation has no active owner run")
            })?;
        let request = CreateElicitationRequest::new(
            ElicitationFormMode::new(
                ElicitationSessionScope::new(session_id.0.as_ref().to_string()),
                schema,
            ),
            message.to_string(),
        )
        .meta(meta);
        let handler = Arc::new(OwnerElicitation {
            run_id: run_id.to_string(),
            session_id: session_id.0.as_ref().to_string(),
            elicitation_id: elicitation_id.to_string(),
            session_manager: self.session_manager.clone(),
            schema: requested_schema.clone(),
            request,
        });
        let options = [
            ("accept", "Submit", true),
            ("decline", "Decline", false),
            ("cancel", "Cancel", false),
        ]
        .into_iter()
        .map(
            |(id, label, value)| leanzero_link::controller_wire::InputOption {
                option_id: id.into(),
                label: label.into(),
                accepts_value: value,
            },
        )
        .collect();
        self.owner_runs
            .register_input(
                session_id.0.as_ref(),
                elicitation_id,
                message.to_string(),
                options,
                leanzero_link::controller_wire::PendingInputPayload::Form {
                    schema: requested_schema.clone(),
                },
                handler,
            )
            .await
            .internal_err()?;
        Ok(())
    }
}

struct OwnerElicitation {
    run_id: String,
    session_id: String,
    elicitation_id: String,
    session_manager: Arc<SessionManager>,
    schema: serde_json::Value,
    request: CreateElicitationRequest,
}

#[async_trait::async_trait]
impl crate::work_control::InputHandler for OwnerElicitation {
    fn run_id(&self) -> Option<&str> {
        Some(&self.run_id)
    }
    fn observe(
        &self,
        cx: &ConnectionTo<Client>,
        registry: Arc<crate::work_control::OwnerRunRegistry>,
        input: leanzero_link::controller_wire::PendingInput,
    ) {
        let _ = cx
            .send_request(CreateElicitationRequestMessage(self.request.clone()))
            .on_receiving_result(move |result| async move {
                if let Ok(response) = result {
                    let run_id = input.run_id.ok_or_else(|| {
                        agent_client_protocol::Error::internal_error()
                            .data("form input has no owner run")
                    })?;
                    let answer = match elicitation_response_from_acp(response.0) {
                        ElicitationOutcome::Accept(value) => crate::work_control::InputAnswer {
                            option_id: "accept".into(),
                            value: Some(value.to_string()),
                        },
                        ElicitationOutcome::Decline => crate::work_control::InputAnswer {
                            option_id: "decline".into(),
                            value: None,
                        },
                        ElicitationOutcome::Cancel => crate::work_control::InputAnswer {
                            option_id: "cancel".into(),
                            value: None,
                        },
                    };
                    if let Err(error) = registry.answer_input(input.input_id, run_id, answer).await
                    {
                        tracing::debug!(%error, "elicitation answer was not current");
                    }
                }
                Ok(())
            });
    }
    fn validate(&self, answer: &crate::work_control::InputAnswer) -> anyhow::Result<()> {
        if answer.option_id == "accept" {
            let value: serde_json::Value = serde_json::from_str(
                answer
                    .value
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("form answer requires its JSON values"))?,
            )?;
            let validator = jsonschema::validator_for(&self.schema)?;
            if let Err(error) = validator.validate(&value) {
                anyhow::bail!("{error}");
            }
        }
        Ok(())
    }
    async fn answer(&self, answer: crate::work_control::InputAnswer) -> anyhow::Result<()> {
        let outcome = match answer.option_id.as_str() {
            "accept" => ElicitationOutcome::Accept(serde_json::from_str(
                answer
                    .value
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("form answer requires its JSON values"))?,
            )?),
            "decline" => ElicitationOutcome::Decline,
            "cancel" => ElicitationOutcome::Cancel,
            _ => anyhow::bail!("invalid form option"),
        };
        crate::elicitation::complete_elicitation_with_generated_message(
            &self.session_manager,
            &self.session_id,
            &self.elicitation_id,
            outcome,
        )
        .await
    }
}

#[derive(Debug, Clone)]
struct CreateElicitationRequestMessage(CreateElicitationRequest);

impl JsonRpcMessage for CreateElicitationRequestMessage {
    fn matches_method(method: &str) -> bool {
        method == CLIENT_METHOD_NAMES.elicitation_create
    }

    fn method(&self) -> &str {
        CLIENT_METHOD_NAMES.elicitation_create
    }

    fn to_untyped_message(&self) -> Result<UntypedMessage, agent_client_protocol::Error> {
        UntypedMessage::new(CLIENT_METHOD_NAMES.elicitation_create, &self.0)
    }

    fn parse_message(
        method: &str,
        params: &impl serde::Serialize,
    ) -> Result<Self, agent_client_protocol::Error> {
        if !Self::matches_method(method) {
            return Err(agent_client_protocol::Error::method_not_found());
        }

        Ok(Self(agent_client_protocol::util::json_cast_params(params)?))
    }
}

impl JsonRpcRequest for CreateElicitationRequestMessage {
    type Response = CreateElicitationResponseMessage;
}

#[derive(Debug, Clone)]
struct CreateElicitationResponseMessage(CreateElicitationResponse);

impl JsonRpcResponse for CreateElicitationResponseMessage {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, agent_client_protocol::Error> {
        serde_json::to_value(self.0).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(
        _method: &str,
        value: serde_json::Value,
    ) -> Result<Self, agent_client_protocol::Error> {
        Ok(Self(agent_client_protocol::util::json_cast(&value)?))
    }
}

pub(super) fn client_supports_form_elicitation(
    args: &agent_client_protocol::schema::v1::InitializeRequest,
) -> bool {
    args.client_capabilities
        .elicitation
        .as_ref()
        .and_then(|elicitation| elicitation.form.as_ref())
        .is_some()
}

fn elicitation_response_from_acp(response: CreateElicitationResponse) -> ElicitationOutcome {
    match response.action {
        AcpElicitationAction::Accept(action) => {
            let content = serde_json::to_value(action.content.unwrap_or_default())
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
            ElicitationOutcome::Accept(content)
        }
        AcpElicitationAction::Decline => ElicitationOutcome::Decline,
        AcpElicitationAction::Cancel => ElicitationOutcome::Cancel,
        action => {
            warn!(?action, "Unsupported ACP elicitation action");
            ElicitationOutcome::Cancel
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action_required_manager::ActionRequiredManager;
    use crate::conversation::message::{ActionRequiredData, MessageContent};
    use crate::work_control::InputAnswer;
    use leanzero_link::controller_wire::{InputOption, PendingInputPayload};
    use serde_json::json;

    #[tokio::test]
    async fn owner_form_preserves_schema_and_only_valid_values_reach_original_callback() {
        let fixture = crate::work_control::tests::Fixture::new().await;
        let ticket = fixture.reserve_run().await;
        let schema = json!({"type":"object", "properties":{"environment":{"type":"string", "enum":["staging","production"], "title":"Deployment target"}}, "required":["environment"], "additionalProperties":false});
        let mut requests = ActionRequiredManager::global()
            .register_action_required_stream(fixture.session.clone(), "deploy-form".into())
            .await;
        let session = fixture.session.clone();
        let original_schema = schema.clone();
        let waiting = tokio::spawn(async move {
            ActionRequiredManager::global()
                .request_and_wait(
                    session,
                    "deploy-form".into(),
                    "Choose deployment target".into(),
                    original_schema,
                    std::time::Duration::from_secs(5),
                )
                .await
        });
        let message = requests.recv().await.unwrap();
        let request_id = match &message.content[0] {
            MessageContent::ActionRequired(required) => match &required.data {
                ActionRequiredData::Elicitation { id, .. } => id.clone(),
                _ => panic!("expected form request"),
            },
            _ => panic!("expected action required"),
        };
        let handler = Arc::new(OwnerElicitation {
            run_id: ticket.run_id.to_string(),
            session_id: fixture.session.clone(),
            elicitation_id: request_id.clone(),
            session_manager: fixture.manager.clone(),
            schema: schema.clone(),
            request: CreateElicitationRequest::new(
                ElicitationFormMode::new(
                    ElicitationSessionScope::new(fixture.session.clone()),
                    serde_json::from_value(schema.clone()).unwrap(),
                ),
                "Choose deployment target",
            ),
        });
        let input = fixture
            .runs
            .register_input(
                &fixture.session,
                &request_id,
                "Choose deployment target".into(),
                vec![InputOption {
                    option_id: "accept".into(),
                    label: "Submit".into(),
                    accepts_value: true,
                }],
                PendingInputPayload::Form {
                    schema: schema.clone(),
                },
                handler,
            )
            .await
            .unwrap();
        let work_id = fixture
            .manager
            .work_catalog()
            .await
            .unwrap()
            .lookup_local_session(&fixture.session)
            .await
            .unwrap()
            .work_id
            .parse()
            .unwrap();
        let persisted = fixture.runs.pending_inputs(work_id).await.unwrap();
        assert_eq!(
            serde_json::to_value(&persisted[0]).unwrap()["payload"]["schema"],
            schema
        );
        for value in ["staging", "{}", "{\"environment\":\"unknown\"}"] {
            let error = fixture
                .runs
                .answer_input(
                    input.input_id,
                    ticket.run_id,
                    InputAnswer {
                        option_id: "accept".into(),
                        value: Some(value.into()),
                    },
                )
                .await
                .unwrap_err();
            assert_eq!(error.body.code, "invalid_input_value");
            assert_eq!(fixture.runs.pending_inputs(work_id).await.unwrap().len(), 1);
            assert!(
                !waiting.is_finished(),
                "invalid value must not reach the original callback"
            );
        }
        let values = json!({"environment":"staging"});
        fixture
            .runs
            .answer_input(
                input.input_id,
                ticket.run_id,
                InputAnswer {
                    option_id: "accept".into(),
                    value: Some(values.to_string()),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            waiting.await.unwrap().unwrap(),
            ElicitationOutcome::Accept(values)
        );
        assert!(fixture
            .runs
            .pending_inputs(work_id)
            .await
            .unwrap()
            .is_empty());
        fixture
            .runs
            .finish(&fixture.session, &ticket.run_id.to_string())
            .await;
        ActionRequiredManager::global()
            .unregister_action_required_stream(&fixture.session, "deploy-form")
            .await;
    }
}
