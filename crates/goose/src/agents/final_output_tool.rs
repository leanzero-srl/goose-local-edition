use crate::agents::tool_execution::ToolCallResult;
use crate::recipe::Response;
use indoc::formatdoc;
use jsonschema::error::ValidationErrorKind;
use rmcp::model::{CallToolRequestParams, Content, ErrorCode, ErrorData, Tool, ToolAnnotations};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub const FINAL_OUTPUT_TOOL_NAME: &str = "recipe__final_output";
pub const FINAL_OUTPUT_CONTINUATION_MESSAGE: &str =
    "You MUST call the `final_output` tool NOW with the final output for the user.";
pub const FINAL_OUTPUT_VALIDATION_EVIDENCE_TYPE: &str = "goose.final_output.schema_validation";
pub const FINAL_OUTPUT_VALIDATION_EVIDENCE_VERSION: u8 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FinalOutputValidationEvidence {
    pub evidence_type: String,
    pub version: u8,
    pub error_fingerprint: String,
    pub argument_shape_fingerprint: String,
    pub value_sensitive: bool,
    pub root_kind: String,
    pub node_count: usize,
    pub error_count: usize,
}

impl FinalOutputValidationEvidence {
    pub fn from_error_data(data: &Value) -> Option<Self> {
        let evidence: Self = serde_json::from_value(data.clone()).ok()?;
        if evidence.evidence_type != FINAL_OUTPUT_VALIDATION_EVIDENCE_TYPE
            || evidence.version != FINAL_OUTPUT_VALIDATION_EVIDENCE_VERSION
            || !is_sha256_fingerprint(&evidence.error_fingerprint)
            || !is_sha256_fingerprint(&evidence.argument_shape_fingerprint)
            || !matches!(
                evidence.root_kind.as_str(),
                "null" | "boolean" | "integer" | "number" | "string" | "array" | "object"
            )
            || evidence.node_count == 0
            || evidence.error_count == 0
        {
            return None;
        }
        Some(evidence)
    }

    fn new(output: &Value, errors: &[jsonschema::ValidationError<'_>]) -> Self {
        let argument_shape = RedactedJsonShape::from_value(output);
        let mut normalized_errors: Vec<_> = errors
            .iter()
            .map(NormalizedValidationError::from_error)
            .collect();
        normalized_errors.sort_unstable();

        Self {
            evidence_type: FINAL_OUTPUT_VALIDATION_EVIDENCE_TYPE.to_string(),
            version: FINAL_OUTPUT_VALIDATION_EVIDENCE_VERSION,
            error_fingerprint: fingerprint_serializable(
                b"goose.final_output.validation_errors.v1\0",
                &normalized_errors,
            ),
            argument_shape_fingerprint: fingerprint_serializable(
                b"goose.final_output.argument_shape.v1\0",
                &argument_shape,
            ),
            value_sensitive: errors
                .iter()
                .any(|error| validation_error_is_value_sensitive(&error.kind)),
            root_kind: argument_shape.root_kind().to_string(),
            node_count: argument_shape.node_count(),
            error_count: errors.len(),
        }
    }
}

struct FinalOutputValidationFailure {
    message: String,
    evidence: Option<FinalOutputValidationEvidence>,
}

#[derive(Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct NormalizedValidationError {
    kind: &'static str,
    schema_path: String,
    instance_depth: usize,
    value_sensitive: bool,
}

impl NormalizedValidationError {
    fn from_error(error: &jsonschema::ValidationError<'_>) -> Self {
        Self {
            kind: validation_error_kind(&error.kind),
            schema_path: error.schema_path.to_string(),
            instance_depth: error.instance_path.as_str().matches('/').count(),
            value_sensitive: validation_error_is_value_sensitive(&error.kind),
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", content = "children", rename_all = "snake_case")]
enum RedactedJsonShape {
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array(Vec<Self>),
    Object(BTreeMap<String, Self>),
}

impl RedactedJsonShape {
    fn from_value(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(_) => Self::Boolean,
            Value::Number(number) if number.is_i64() || number.is_u64() => Self::Integer,
            Value::Number(_) => Self::Number,
            Value::String(_) => Self::String,
            Value::Array(values) => {
                Self::Array(values.iter().map(RedactedJsonShape::from_value).collect())
            }
            Value::Object(values) => Self::Object(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), RedactedJsonShape::from_value(value)))
                    .collect(),
            ),
        }
    }

    fn root_kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::String => "string",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
        }
    }

    fn node_count(&self) -> usize {
        match self {
            Self::Array(values) => {
                1 + values
                    .iter()
                    .map(RedactedJsonShape::node_count)
                    .sum::<usize>()
            }
            Self::Object(values) => {
                1 + values
                    .values()
                    .map(RedactedJsonShape::node_count)
                    .sum::<usize>()
            }
            _ => 1,
        }
    }
}

fn validation_error_is_value_sensitive(kind: &ValidationErrorKind) -> bool {
    !matches!(
        kind,
        ValidationErrorKind::AdditionalItems { .. }
            | ValidationErrorKind::AdditionalProperties { .. }
            | ValidationErrorKind::FalseSchema
            | ValidationErrorKind::MaxItems { .. }
            | ValidationErrorKind::MaxProperties { .. }
            | ValidationErrorKind::MinItems { .. }
            | ValidationErrorKind::MinProperties { .. }
            | ValidationErrorKind::PropertyNames { .. }
            | ValidationErrorKind::Required { .. }
            | ValidationErrorKind::Type { .. }
    )
}

fn validation_error_kind(kind: &ValidationErrorKind) -> &'static str {
    match kind {
        ValidationErrorKind::AdditionalItems { .. } => "additional_items",
        ValidationErrorKind::AdditionalProperties { .. } => "additional_properties",
        ValidationErrorKind::AnyOf => "any_of",
        ValidationErrorKind::BacktrackLimitExceeded { .. } => "backtrack_limit_exceeded",
        ValidationErrorKind::Constant { .. } => "constant",
        ValidationErrorKind::Contains => "contains",
        ValidationErrorKind::ContentEncoding { .. } => "content_encoding",
        ValidationErrorKind::ContentMediaType { .. } => "content_media_type",
        ValidationErrorKind::Custom { .. } => "custom",
        ValidationErrorKind::Enum { .. } => "enum",
        ValidationErrorKind::ExclusiveMaximum { .. } => "exclusive_maximum",
        ValidationErrorKind::ExclusiveMinimum { .. } => "exclusive_minimum",
        ValidationErrorKind::FalseSchema => "false_schema",
        ValidationErrorKind::Format { .. } => "format",
        ValidationErrorKind::FromUtf8 { .. } => "from_utf8",
        ValidationErrorKind::MaxItems { .. } => "max_items",
        ValidationErrorKind::Maximum { .. } => "maximum",
        ValidationErrorKind::MaxLength { .. } => "max_length",
        ValidationErrorKind::MaxProperties { .. } => "max_properties",
        ValidationErrorKind::MinItems { .. } => "min_items",
        ValidationErrorKind::Minimum { .. } => "minimum",
        ValidationErrorKind::MinLength { .. } => "min_length",
        ValidationErrorKind::MinProperties { .. } => "min_properties",
        ValidationErrorKind::MultipleOf { .. } => "multiple_of",
        ValidationErrorKind::Not { .. } => "not",
        ValidationErrorKind::OneOfMultipleValid => "one_of_multiple_valid",
        ValidationErrorKind::OneOfNotValid => "one_of_not_valid",
        ValidationErrorKind::Pattern { .. } => "pattern",
        ValidationErrorKind::PropertyNames { .. } => "property_names",
        ValidationErrorKind::Required { .. } => "required",
        ValidationErrorKind::Type { .. } => "type",
        ValidationErrorKind::UnevaluatedItems { .. } => "unevaluated_items",
        ValidationErrorKind::UnevaluatedProperties { .. } => "unevaluated_properties",
        ValidationErrorKind::UniqueItems => "unique_items",
        ValidationErrorKind::Referencing(_) => "referencing",
    }
}

fn fingerprint_serializable(value_domain: &[u8], value: &impl Serialize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value_domain);
    hasher.update(serde_json::to_vec(value).expect("validation evidence must be serializable"));
    let digest = hasher.finalize();
    let mut fingerprint = String::with_capacity("sha256:".len() + digest.len() * 2);
    fingerprint.push_str("sha256:");
    for byte in digest {
        write!(&mut fingerprint, "{byte:02x}").expect("writing to a String cannot fail");
    }
    fingerprint
}

fn is_sha256_fingerprint(value: &str) -> bool {
    value.len() == "sha256:".len() + 64
        && value
            .strip_prefix("sha256:")
            .is_some_and(|digest| digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

pub struct FinalOutputTool {
    pub response: Response,
    /// The final output collected for the user. It will be a single line string for easy script extraction from output.
    pub final_output: Option<String>,
}

impl FinalOutputTool {
    pub fn new(response: Response) -> Self {
        if response.json_schema.is_none() {
            panic!("Cannot create FinalOutputTool: json_schema is required");
        }
        let schema = response.json_schema.as_ref().unwrap();

        if let Some(obj) = schema.as_object() {
            if obj.is_empty() {
                panic!("Cannot create FinalOutputTool: empty json_schema is not allowed");
            }
        }

        jsonschema::meta::validate(schema).unwrap();
        Self {
            response,
            final_output: None,
        }
    }

    pub fn tool(&self) -> Tool {
        let instructions = formatdoc! {r#"
            The final_output tool collects the final output for the user and provides validation for structured JSON final output against a predefined schema.

            This final_output tool MUST be called with the final output for the user.
            
            Purpose:
            - Collects the final output for the user
            - Ensures that final outputs conform to the expected JSON structure
            - Provides clear validation feedback when outputs don't match the schema
            
            Usage:
            - Call the `final_output` tool with your JSON final output passed as the argument.
            
            The expected JSON schema format is:

            {}
            
            When validation fails, you'll receive:
            - Specific validation errors
            - The expected format
        "#, serde_json::to_string_pretty(self.response.json_schema.as_ref().unwrap()).unwrap()};

        Tool::new(
            FINAL_OUTPUT_TOOL_NAME.to_string(),
            instructions,
            self.response
                .json_schema
                .as_ref()
                .unwrap()
                .as_object()
                .unwrap()
                .clone(),
        )
        .annotate(
            ToolAnnotations::with_title("Final Output".to_string())
                .read_only(false)
                .destructive(false)
                .idempotent(true)
                .open_world(false),
        )
    }

    pub fn system_prompt(&self) -> String {
        formatdoc! {r#"
            # Final Output Instructions

            You MUST use the `final_output` tool to collect the final output for the user rather than providing the output directly in your response.
            The final output MUST be a valid JSON object that is provided to the `final_output` tool when called and it must match the following schema:

            {}

            ----
        "#, serde_json::to_string_pretty(self.response.json_schema.as_ref().unwrap()).unwrap()}
    }

    async fn validate_json_output(
        &self,
        output: &Value,
    ) -> Result<Value, FinalOutputValidationFailure> {
        let compiled_schema =
            match jsonschema::validator_for(self.response.json_schema.as_ref().unwrap()) {
                Ok(schema) => schema,
                Err(e) => {
                    return Err(FinalOutputValidationFailure {
                        message: format!("Internal error: Failed to compile schema: {}", e),
                        evidence: None,
                    });
                }
            };

        let errors: Vec<_> = compiled_schema.iter_errors(output).collect();
        let validation_errors: Vec<String> = errors
            .iter()
            .map(|error| format!("- {}: {}", error.instance_path, error))
            .collect();

        if validation_errors.is_empty() {
            Ok(output.clone())
        } else {
            Err(FinalOutputValidationFailure {
                message: format!(
                    "Validation failed:\n{}\n\nExpected format:\n{}\n\nPlease correct your output to match the expected JSON schema and try again.",
                    validation_errors.join("\n"),
                    serde_json::to_string_pretty(self.response.json_schema.as_ref().unwrap()).unwrap_or_else(|_| "Invalid schema".to_string())
                ),
                evidence: Some(FinalOutputValidationEvidence::new(output, &errors)),
            })
        }
    }

    pub async fn execute_tool_call(&mut self, tool_call: CallToolRequestParams) -> ToolCallResult {
        match tool_call.name.to_string().as_str() {
            FINAL_OUTPUT_TOOL_NAME => {
                let result = self.validate_json_output(&tool_call.arguments.into()).await;
                match result {
                    Ok(parsed_value) => {
                        self.final_output = Some(Self::parsed_final_output_string(parsed_value));
                        ToolCallResult::from(Ok(rmcp::model::CallToolResult::success(vec![
                            Content::text("Final output successfully collected.".to_string()),
                        ])))
                    }
                    Err(error) => {
                        let data = error.evidence.map(|evidence| {
                            serde_json::to_value(evidence)
                                .expect("validation evidence must be serializable")
                        });
                        ToolCallResult::from(Err(ErrorData {
                            code: ErrorCode::INVALID_PARAMS,
                            message: Cow::from(error.message),
                            data,
                        }))
                    }
                }
            }
            _ => ToolCallResult::from(Err(ErrorData {
                code: ErrorCode::INVALID_REQUEST,
                message: Cow::from(format!("Unknown tool: {}", tool_call.name)),
                data: None,
            })),
        }
    }

    // Formats the parsed JSON as a single line string so its easy to extract from the output
    fn parsed_final_output_string(parsed_json: Value) -> String {
        serde_json::to_string(&parsed_json).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::Response;
    use rmcp::model::CallToolRequestParams;
    use rmcp::object;
    use serde_json::json;

    fn create_complex_test_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "age": {"type": "number"}
                    },
                    "required": ["name", "age"]
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            },
            "required": ["user", "tags"]
        })
    }

    async fn execute_invalid(schema: Value, arguments: Value) -> ErrorData {
        let mut tool = FinalOutputTool::new(Response {
            json_schema: Some(schema),
        });
        let tool_call = CallToolRequestParams::new(FINAL_OUTPUT_TOOL_NAME)
            .with_arguments(arguments.as_object().unwrap().clone());
        let result = tool.execute_tool_call(tool_call).await;
        match result.result.await {
            Ok(_) => panic!("expected final output validation to fail"),
            Err(error) => error,
        }
    }

    fn evidence_from(error: &ErrorData) -> FinalOutputValidationEvidence {
        FinalOutputValidationEvidence::from_error_data(
            error
                .data
                .as_ref()
                .expect("schema validation failure must contain evidence"),
        )
        .expect("schema validation evidence must match the supported contract")
    }

    #[test]
    #[should_panic(expected = "Cannot create FinalOutputTool: json_schema is required")]
    fn test_new_with_missing_schema() {
        let response = Response { json_schema: None };
        FinalOutputTool::new(response);
    }

    #[test]
    #[should_panic(expected = "Cannot create FinalOutputTool: empty json_schema is not allowed")]
    fn test_new_with_empty_schema() {
        let response = Response {
            json_schema: Some(json!({})),
        };
        FinalOutputTool::new(response);
    }

    #[test]
    #[should_panic]
    fn test_new_with_invalid_schema() {
        let response = Response {
            json_schema: Some(json!({
                "type": "invalid_type",
                "properties": {
                    "message": {
                        "type": "unknown_type"
                    }
                }
            })),
        };
        FinalOutputTool::new(response);
    }

    #[tokio::test]
    async fn test_execute_tool_call_schema_validation_failure() {
        let response = Response {
            json_schema: Some(json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string"
                    },
                    "count": {
                        "type": "number"
                    }
                },
                "required": ["message", "count"]
            })),
        };

        let mut tool = FinalOutputTool::new(response);
        let tool_call =
            CallToolRequestParams::new(FINAL_OUTPUT_TOOL_NAME).with_arguments(object!({
                "message": "Hello"  // Missing required "count" field
            }));

        let result = tool.execute_tool_call(tool_call).await;
        let tool_result = result.result.await;
        assert!(tool_result.is_err());
        if let Err(error) = tool_result {
            assert!(error.to_string().contains("Validation failed"));
            let evidence = evidence_from(&error);
            assert_eq!(
                evidence.evidence_type,
                FINAL_OUTPUT_VALIDATION_EVIDENCE_TYPE
            );
            assert_eq!(evidence.version, FINAL_OUTPUT_VALIDATION_EVIDENCE_VERSION);
            assert!(!evidence.value_sensitive);
            assert_eq!(evidence.root_kind, "object");
            assert_eq!(evidence.node_count, 2);
            assert_eq!(evidence.error_count, 1);
        }
    }

    #[tokio::test]
    async fn validation_evidence_does_not_leak_argument_keys_or_leaf_values() {
        const SENTINEL_KEY: &str = "FINAL_OUTPUT_SENTINEL_PRIVATE_KEY_7fbe28";
        const SENTINEL_LEAF: &str = "FINAL_OUTPUT_SENTINEL_PRIVATE_LEAF_46c91a";
        const SENTINEL_NESTED_LEAF: &str = "FINAL_OUTPUT_SENTINEL_NESTED_LEAF_b31d04";

        let schema = json!({
            "type": "object",
            "properties": {
                "mode": {"type": "string", "enum": ["allowed"]},
                "count": {"type": "integer"}
            },
            "required": ["count"],
            "additionalProperties": false
        });
        let arguments = json!({
            "mode": SENTINEL_LEAF,
            (SENTINEL_KEY): [SENTINEL_NESTED_LEAF]
        });

        let error = execute_invalid(schema, arguments).await;
        let serialized_evidence = serde_json::to_string(
            error
                .data
                .as_ref()
                .expect("schema validation failure must contain evidence"),
        )
        .unwrap();
        assert!(!serialized_evidence.contains(SENTINEL_KEY));
        assert!(!serialized_evidence.contains(SENTINEL_LEAF));
        assert!(!serialized_evidence.contains(SENTINEL_NESTED_LEAF));

        let evidence = evidence_from(&error);
        assert!(evidence.value_sensitive);
        assert_eq!(evidence.root_kind, "object");
        assert_eq!(evidence.node_count, 4);
        assert!(evidence.error_count >= 1);
    }

    #[tokio::test]
    async fn structural_evidence_redacts_leaf_values_but_distinguishes_shapes() {
        let schema = json!({
            "type": "object",
            "properties": {
                "message": {"type": "number"}
            },
            "required": ["message"]
        });

        let first = execute_invalid(
            schema.clone(),
            json!({"message": "FINAL_OUTPUT_SENTINEL_FIRST_650b4a"}),
        )
        .await;
        let second = execute_invalid(
            schema.clone(),
            json!({"message": "FINAL_OUTPUT_SENTINEL_SECOND_3d8e29"}),
        )
        .await;
        let different_shape = execute_invalid(schema, json!({"message": false})).await;

        let first = evidence_from(&first);
        let second = evidence_from(&second);
        let different_shape = evidence_from(&different_shape);
        assert!(!first.value_sensitive);
        assert_eq!(
            first.argument_shape_fingerprint,
            second.argument_shape_fingerprint
        );
        assert_eq!(first.error_fingerprint, second.error_fingerprint);
        assert_ne!(
            first.argument_shape_fingerprint,
            different_shape.argument_shape_fingerprint
        );
        assert_eq!(first.error_fingerprint, different_shape.error_fingerprint);
    }

    #[test]
    fn validation_evidence_parser_rejects_other_versions_and_types() {
        let evidence = FinalOutputValidationEvidence {
            evidence_type: FINAL_OUTPUT_VALIDATION_EVIDENCE_TYPE.to_string(),
            version: FINAL_OUTPUT_VALIDATION_EVIDENCE_VERSION,
            error_fingerprint: format!("sha256:{}", "0".repeat(64)),
            argument_shape_fingerprint: format!("sha256:{}", "1".repeat(64)),
            value_sensitive: false,
            root_kind: "object".to_string(),
            node_count: 1,
            error_count: 1,
        };

        let mut wrong_version = serde_json::to_value(&evidence).unwrap();
        wrong_version["version"] = json!(FINAL_OUTPUT_VALIDATION_EVIDENCE_VERSION + 1);
        assert!(FinalOutputValidationEvidence::from_error_data(&wrong_version).is_none());

        let mut wrong_type = serde_json::to_value(evidence).unwrap();
        wrong_type["evidence_type"] = json!("another.tool.validation");
        assert!(FinalOutputValidationEvidence::from_error_data(&wrong_type).is_none());
    }

    #[tokio::test]
    async fn test_execute_tool_call_complex_valid_json() {
        let response = Response {
            json_schema: Some(create_complex_test_schema()),
        };

        let mut tool = FinalOutputTool::new(response);
        let tool_call =
            CallToolRequestParams::new(FINAL_OUTPUT_TOOL_NAME).with_arguments(object!({
                "user": {
                    "name": "John",
                    "age": 30
                },
                "tags": ["developer", "rust"]
            }));

        let result = tool.execute_tool_call(tool_call).await;
        let tool_result = result.result.await;
        assert!(tool_result.is_ok());
        assert!(tool.final_output.is_some());

        let final_output = tool.final_output.unwrap();
        assert!(serde_json::from_str::<Value>(&final_output).is_ok());
        assert!(!final_output.contains('\n'));
    }
}
