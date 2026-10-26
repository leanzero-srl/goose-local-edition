use super::*;
use crate::recipe::RecipeParameterInputType;
use crate::work_control::{InputAnswer, InputHandler, OwnerRunRegistry};
use leanzero_link::controller_wire::PendingInput;
use serde_json::{json, Value};
use std::sync::Mutex;

impl GooseAcpAgent {
    pub(super) async fn request_owned_recipe_params(
        &self,
        session_id: &str,
        recipe: &Recipe,
        recipe_dir: &Path,
        parameters: Vec<RecipeParameter>,
    ) -> Result<RecipeParamsResponse, agent_client_protocol::Error> {
        let schema = parameter_schema(&parameters).internal_err()?;
        let request = RequestRecipeParams {
            session_id: session_id.to_owned(),
            parameters: parameters
                .iter()
                .cloned()
                .map(RecipeParameterDto::from)
                .collect(),
        };
        let (sender, receiver) = oneshot::channel();
        let handler = Arc::new(RecipeInput {
            parameters,
            schema: schema.clone(),
            recipe: recipe.clone(),
            recipe_dir: recipe_dir.to_path_buf(),
            request,
            sender: Mutex::new(Some(sender)),
        });
        self.owner_runs
            .register_preflight_input(
                session_id,
                format!("Set up recipe: {}", recipe.title),
                schema,
                handler,
            )
            .await
            .internal_err()?;
        receiver.await.map_err(|_| {
            agent_client_protocol::Error::internal_error()
                .data("original recipe setup callback was interrupted")
        })
    }
}

struct RecipeInput {
    parameters: Vec<RecipeParameter>,
    schema: Value,
    recipe: Recipe,
    recipe_dir: PathBuf,
    request: RequestRecipeParams,
    sender: Mutex<Option<oneshot::Sender<RecipeParamsResponse>>>,
}

#[async_trait::async_trait]
impl InputHandler for RecipeInput {
    fn run_id(&self) -> Option<&str> {
        None
    }
    fn observe(
        &self,
        cx: &ConnectionTo<Client>,
        registry: Arc<OwnerRunRegistry>,
        input: PendingInput,
    ) {
        let parameters = self.parameters.clone();
        let _ = cx
            .send_request(RequestRecipeParamsMessage(self.request.clone()))
            .on_receiving_result(move |result| async move {
                if let Ok(response) = result {
                    match local_answer(&parameters, response.0) {
                        Ok(answer) => {
                            if let Err(error) =
                                registry.answer_preflight(input.input_id, answer).await
                            {
                                tracing::debug!(%error,"recipe input was not current or valid");
                            }
                        }
                        Err(error) => tracing::debug!(%error,"recipe input values were invalid"),
                    }
                }
                Ok(())
            });
    }
    fn validate(&self, answer: &InputAnswer) -> anyhow::Result<()> {
        if answer.option_id == "cancel" {
            return Ok(());
        }
        let value: Value = serde_json::from_str(
            answer
                .value
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("recipe parameters require a JSON object"))?,
        )?;
        jsonschema::validator_for(&self.schema)?
            .validate(&value)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let values = recipe_values(&value)?;
        for parameter in &self.parameters {
            let Some(value) = values.get(&parameter.key) else {
                continue;
            };
            match parameter.input_type {
                RecipeParameterInputType::Date => {
                    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")?;
                }
                RecipeParameterInputType::File => anyhow::ensure!(
                    Path::new(value).is_absolute(),
                    "{} must be an absolute path on the original Mac",
                    parameter.key
                ),
                _ => {}
            }
        }
        build_recipe_from_template(
            self.recipe.to_yaml()?,
            &self.recipe_dir,
            values.into_iter().collect(),
            None::<fn(&str, &str) -> Result<String, anyhow::Error>>,
        )?;
        Ok(())
    }
    async fn answer(&self, answer: InputAnswer) -> anyhow::Result<()> {
        let response = if answer.option_id == "cancel" {
            RecipeParamsResponse {
                action: RecipeParamsAction::Cancel,
                values: HashMap::new(),
            }
        } else {
            let value: Value = serde_json::from_str(
                answer
                    .value
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("recipe answer missing"))?,
            )?;
            RecipeParamsResponse {
                action: RecipeParamsAction::Submit,
                values: recipe_values(&value)?,
            }
        };
        let sender = self
            .sender
            .lock()
            .expect("recipe callback poisoned")
            .take()
            .ok_or_else(|| anyhow::anyhow!("recipe callback was already consumed"))?;
        sender
            .send(response)
            .map_err(|_| anyhow::anyhow!("original recipe callback receiver is gone"))
    }
}

fn recipe_values(value: &Value) -> anyhow::Result<HashMap<String, String>> {
    value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("recipe parameters must be an object"))?
        .iter()
        .map(|(key, value)| {
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Number(_) | Value::Bool(_) => value.to_string(),
                _ => anyhow::bail!("unsupported recipe value for {key}"),
            };
            Ok((key.clone(), text))
        })
        .collect()
}

fn typed_value(parameter: &RecipeParameter, text: &str) -> anyhow::Result<Value> {
    Ok(match parameter.input_type {
        RecipeParameterInputType::Number => Value::Number(text.parse()?),
        RecipeParameterInputType::Boolean => Value::Bool(text.parse()?),
        _ => Value::String(text.to_owned()),
    })
}

fn local_answer(
    parameters: &[RecipeParameter],
    response: RecipeParamsResponse,
) -> anyhow::Result<InputAnswer> {
    if matches!(response.action, RecipeParamsAction::Cancel) {
        return Ok(InputAnswer {
            option_id: "cancel".into(),
            value: None,
        });
    }
    let mut values = serde_json::Map::new();
    for (key, text) in response.values {
        let parameter = parameters
            .iter()
            .find(|parameter| parameter.key == key)
            .ok_or_else(|| anyhow::anyhow!("unknown recipe parameter {key}"))?;
        values.insert(key, typed_value(parameter, &text)?);
    }
    Ok(InputAnswer {
        option_id: "accept".into(),
        value: Some(Value::Object(values).to_string()),
    })
}

fn parameter_schema(parameters: &[RecipeParameter]) -> anyhow::Result<Value> {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for parameter in parameters {
        let mut property = json!({"title":parameter.key,"description":parameter.description,"type":match parameter.input_type {RecipeParameterInputType::Number=>"number",RecipeParameterInputType::Boolean=>"boolean",_=>"string"}});
        if let Some(default) = &parameter.default {
            property["default"] = typed_value(parameter, default)?;
        }
        if parameter.default.is_none() {
            required.push(parameter.key.clone());
        }
        if matches!(parameter.input_type, RecipeParameterInputType::Select) {
            let options = parameter
                .options
                .as_ref()
                .filter(|options| !options.is_empty())
                .ok_or_else(|| anyhow::anyhow!("{} has no recipe choices", parameter.key))?;
            property["enum"] = json!(options);
        }
        if matches!(parameter.input_type, RecipeParameterInputType::File) {
            property["description"] = json!(format!(
                "{} Absolute file path on the original Mac. Goose reads that file's contents.",
                parameter.description
            ));
        }
        if matches!(parameter.input_type, RecipeParameterInputType::Date) {
            property["description"] = json!(format!(
                "{} Date in YYYY-MM-DD form.",
                parameter.description
            ));
        }
        properties.insert(parameter.key.clone(), property);
    }
    Ok(
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    )
}
