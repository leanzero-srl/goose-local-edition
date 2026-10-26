use super::{conflict, unavailable, InputAnswer, OwnerRunRegistry};
use crate::needs_you::{self, NeedsYouItem, NeedsYouState, NeedsYouStatus, Resolution};
use crate::session::extension_data::ExtensionState;
use agent_client_protocol::schema::v1::{ContentBlock, PromptRequest, SessionId, TextContent};
use leanzero_link::controller::ControllerError;
use leanzero_link::controller_wire::{InputOption, PendingInput, PendingInputPayload};
use uuid::Uuid;

impl OwnerRunRegistry {
    async fn questions(&self, session_id: &str) -> Result<Vec<NeedsYouItem>, ControllerError> {
        let session = self
            .session_manager
            .get_session(session_id, false)
            .await
            .map_err(unavailable)?;
        match session
            .extension_data
            .get_extension_state(NeedsYouState::EXTENSION_NAME, NeedsYouState::VERSION)
        {
            Some(value) => Ok(NeedsYouState::from_value(value).map_err(unavailable)?.items),
            None => Ok(Vec::new()),
        }
    }

    pub(super) async fn pending_questions(
        &self,
        work_id: Uuid,
    ) -> Result<Vec<PendingInput>, ControllerError> {
        let catalog = self
            .session_manager
            .work_catalog()
            .await
            .map_err(unavailable)?;
        let watermark = catalog.watermark().await.map_err(unavailable)?;
        let work = catalog
            .lookup(&work_id.to_string(), &watermark.generation)
            .await
            .map_err(unavailable)?;
        if work.current_run_id.is_some() {
            return Ok(Vec::new());
        }
        self.questions(&work.local_locator)
            .await?
            .into_iter()
            .filter(|item| item.status == NeedsYouStatus::Open)
            .map(|item| question_input(&item))
            .collect()
    }

    pub(super) async fn question_input(
        &self,
        session_id: &str,
        input_id: Uuid,
    ) -> Result<Option<NeedsYouItem>, ControllerError> {
        let item_id = format!("ny_{}", input_id.simple());
        Ok(self
            .questions(session_id)
            .await?
            .into_iter()
            .find(|item| item.id == item_id))
    }

    pub(super) async fn resolve_question(
        &self,
        session_id: &str,
        work_id: Uuid,
        generation: Uuid,
        input_id: Uuid,
        answer: &InputAnswer,
    ) -> Result<Option<PromptRequest>, ControllerError> {
        let item = self
            .question_input(session_id, input_id)
            .await?
            .ok_or_else(|| conflict("input_not_found", "question no longer exists"))?;
        if let Some(origin) = &item.controller_origin {
            if origin.work_id != work_id || origin.store_generation != generation {
                return Err(conflict(
                    "input_interrupted",
                    "question belongs to a replaced work store",
                ));
            }
        }
        if item.status != NeedsYouStatus::Open {
            return Err(conflict(
                "input_already_answered",
                "question was already resolved by another viewer",
            ));
        }
        let resolution = question_resolution(&item, answer)?;
        let resolved = needs_you::resolve(&self.session_manager, session_id, &item.id, resolution)
            .await
            .map_err(|error| conflict("input_already_answered", error.to_string()))?;
        crate::session_loops::seam::needs_you_resolved(session_id, &item.id);
        let Some(answer) = resolved.answer else {
            return Ok(None);
        };
        let text = format!("Answer to your question \"{}\": {answer}", item.question);
        Ok(Some(
            PromptRequest::new(
                SessionId::new(session_id),
                vec![ContentBlock::Text(TextContent::new(text))],
            )
            .meta(
                serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
                    serde_json::json!({"goose":{"needsYouAnswers":[item.id]}}),
                )
                .map_err(unavailable)?,
            ),
        ))
    }
}

fn question_input(item: &NeedsYouItem) -> Result<PendingInput, ControllerError> {
    let input_id = item
        .id
        .strip_prefix("ny_")
        .ok_or_else(|| unavailable("question has no stable input identity"))?
        .parse()
        .map_err(unavailable)?;
    let mut options = item
        .options
        .iter()
        .enumerate()
        .map(|(index, label)| InputOption {
            option_id: format!("choice_{index}"),
            label: label.clone(),
            accepts_value: false,
        })
        .collect::<Vec<_>>();
    options.extend([
        InputOption {
            option_id: "answer".into(),
            label: "Send answer".into(),
            accepts_value: true,
        },
        InputOption {
            option_id: "dismiss".into(),
            label: "Dismiss question".into(),
            accepts_value: false,
        },
    ]);
    Ok(PendingInput {
        input_id,
        run_id: None,
        prompt: item.question.clone(),
        options,
        payload: PendingInputPayload::Question {
            item_id: item.id.clone(),
            why: item.why.clone(),
            recommended_answer: item.recommended_answer.clone(),
            origin_run_id: item
                .controller_origin
                .as_ref()
                .and_then(|origin| origin.run_id),
        },
    })
}

fn question_resolution(
    item: &NeedsYouItem,
    answer: &InputAnswer,
) -> Result<Resolution, ControllerError> {
    let input = question_input(item)?;
    let option = input
        .options
        .iter()
        .find(|option| option.option_id == answer.option_id)
        .ok_or_else(|| {
            conflict(
                "invalid_input_option",
                "this answer is not one of the current choices",
            )
        })?;
    if answer.value.is_some() && !option.accepts_value {
        return Err(conflict(
            "invalid_input_value",
            "this choice does not accept a value",
        ));
    }
    if answer.option_id == "dismiss" {
        return Ok(Resolution::Dismissed);
    }
    let text = if answer.option_id == "answer" {
        answer
            .value
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| conflict("invalid_input_value", "an answer must not be empty"))?
            .trim()
            .to_string()
    } else {
        option.label.clone()
    };
    Ok(Resolution::Answered(text))
}
