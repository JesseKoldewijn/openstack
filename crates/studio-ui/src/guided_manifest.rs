use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const SUPPORTED_SCHEMA_VERSION: &str = "1.2";
pub const GUIDED_MANIFEST_ROOT: &str = "manifests/guided";
pub const GUIDED_MANIFEST_SUFFIX: &str = ".guided.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolClass {
    Query,
    JsonTarget,
    RestXml,
    RestJson,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuidedManifest {
    pub schema_version: String,
    pub service: String,
    pub protocol: ProtocolClass,
    pub flows: Vec<GuidedFlow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuidedFlow {
    pub id: String,
    pub level: String,
    pub steps: Vec<GuidedStep>,
    #[serde(default)]
    pub cleanup: Vec<GuidedStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuidedStep {
    pub id: String,
    pub title: String,
    pub operation: NormalizedOperation,
    #[serde(default)]
    pub assertions: Vec<FlowAssertion>,
    #[serde(default)]
    pub captures: Vec<CaptureBinding>,
    pub error_guidance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedOperation {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub query: HashMap<String, String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowAssertion {
    pub kind: String,
    pub target: String,
    pub expected: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureBinding {
    pub name: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("schema document is invalid JSON: {0}")]
    InvalidSchemaDocument(serde_json::Error),
    #[error("manifest is invalid JSON: {0}")]
    InvalidManifestJson(serde_json::Error),
    #[error("manifest structural validation failed")]
    StructuralValidation { issues: Vec<ValidationIssue> },
    #[error("manifest semantic lint failed")]
    SemanticValidation { issues: Vec<ValidationIssue> },
    #[error(
        "unsupported manifest schema version '{manifest_version}' for runtime '{runtime_version}'"
    )]
    IncompatibleSchemaVersion {
        manifest_version: String,
        runtime_version: String,
    },
}

pub fn load_schema_document() -> Result<Value, ManifestError> {
    serde_json::from_str(include_str!(
        "../schemas/studio-guided-flow.manifest.v1.schema.json"
    ))
    .map_err(ManifestError::InvalidSchemaDocument)
}

pub fn parse_and_validate_manifest(source: &str) -> Result<GuidedManifest, ManifestError> {
    let value: Value = serde_json::from_str(source).map_err(ManifestError::InvalidManifestJson)?;

    let structural_issues = validate_manifest_structure(&value);
    if !structural_issues.is_empty() {
        return Err(ManifestError::StructuralValidation {
            issues: structural_issues,
        });
    }

    let manifest: GuidedManifest =
        serde_json::from_value(value).map_err(ManifestError::InvalidManifestJson)?;
    check_schema_compatibility(&manifest.schema_version, SUPPORTED_SCHEMA_VERSION)?;

    let semantic_issues = lint_manifest_semantics(&manifest);
    if !semantic_issues.is_empty() {
        return Err(ManifestError::SemanticValidation {
            issues: semantic_issues,
        });
    }

    Ok(manifest)
}

pub fn check_schema_compatibility(
    manifest_version: &str,
    runtime_version: &str,
) -> Result<(), ManifestError> {
    let Some((manifest_major, manifest_minor)) = parse_version(manifest_version) else {
        return Err(ManifestError::IncompatibleSchemaVersion {
            manifest_version: manifest_version.to_string(),
            runtime_version: runtime_version.to_string(),
        });
    };
    let Some((runtime_major, runtime_minor)) = parse_version(runtime_version) else {
        return Err(ManifestError::IncompatibleSchemaVersion {
            manifest_version: manifest_version.to_string(),
            runtime_version: runtime_version.to_string(),
        });
    };

    if manifest_major != runtime_major || manifest_minor > runtime_minor {
        return Err(ManifestError::IncompatibleSchemaVersion {
            manifest_version: manifest_version.to_string(),
            runtime_version: runtime_version.to_string(),
        });
    }

    Ok(())
}

pub fn expected_manifest_path(service: &str) -> String {
    format!("{GUIDED_MANIFEST_ROOT}/{service}{GUIDED_MANIFEST_SUFFIX}")
}

pub fn enforce_one_manifest_per_service(
    services: &[String],
    manifest_paths: &[String],
) -> Result<(), ManifestError> {
    let mut issues = Vec::new();
    let mut seen = HashSet::new();

    for path in manifest_paths {
        let Some(name) = service_name_from_path(path) else {
            issues.push(ValidationIssue {
                path: path.clone(),
                message: format!(
                    "manifest path must follow '{GUIDED_MANIFEST_ROOT}/<service>{GUIDED_MANIFEST_SUFFIX}'"
                ),
            });
            continue;
        };

        if !seen.insert(name.clone()) {
            issues.push(ValidationIssue {
                path: path.clone(),
                message: format!("duplicate manifest for service '{name}'"),
            });
        }
    }

    for service in services {
        if !seen.contains(service) {
            issues.push(ValidationIssue {
                path: expected_manifest_path(service),
                message: "missing manifest for supported service".to_string(),
            });
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(ManifestError::SemanticValidation { issues })
    }
}

fn validate_manifest_structure(value: &Value) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    let Some(obj) = value.as_object() else {
        issues.push(ValidationIssue {
            path: "$".to_string(),
            message: "manifest root must be an object".to_string(),
        });
        return issues;
    };

    require_string(obj, "schemaVersion", "$", &mut issues);
    require_string(obj, "service", "$", &mut issues);
    require_string(obj, "protocol", "$", &mut issues);

    match obj.get("flows") {
        Some(flows) if flows.is_array() => {
            if let Some(items) = flows.as_array() {
                if items.is_empty() {
                    issues.push(ValidationIssue {
                        path: "$.flows".to_string(),
                        message: "must contain at least one flow".to_string(),
                    });
                }

                for (flow_index, flow) in items.iter().enumerate() {
                    let flow_path = format!("$.flows[{flow_index}]");
                    let Some(flow_obj) = flow.as_object() else {
                        issues.push(ValidationIssue {
                            path: flow_path,
                            message: "flow must be an object".to_string(),
                        });
                        continue;
                    };

                    require_string(flow_obj, "id", &flow_path, &mut issues);
                    require_string(flow_obj, "level", &flow_path, &mut issues);

                    match flow_obj.get("steps") {
                        Some(steps) if steps.is_array() => {
                            if let Some(step_items) = steps.as_array() {
                                if step_items.is_empty() {
                                    issues.push(ValidationIssue {
                                        path: format!("{flow_path}.steps"),
                                        message: "must contain at least one step".to_string(),
                                    });
                                }

                                for (step_index, step) in step_items.iter().enumerate() {
                                    let step_path = format!("{flow_path}.steps[{step_index}]");
                                    let Some(step_obj) = step.as_object() else {
                                        issues.push(ValidationIssue {
                                            path: step_path,
                                            message: "step must be an object".to_string(),
                                        });
                                        continue;
                                    };

                                    require_string(step_obj, "id", &step_path, &mut issues);
                                    require_string(step_obj, "title", &step_path, &mut issues);

                                    match step_obj.get("operation") {
                                        Some(op) if op.is_object() => {
                                            if let Some(op_obj) = op.as_object() {
                                                let op_path = format!("{step_path}.operation");
                                                require_string(
                                                    op_obj,
                                                    "method",
                                                    &op_path,
                                                    &mut issues,
                                                );
                                                require_string(
                                                    op_obj,
                                                    "path",
                                                    &op_path,
                                                    &mut issues,
                                                );
                                            }
                                        }
                                        Some(_) => issues.push(ValidationIssue {
                                            path: format!("{step_path}.operation"),
                                            message: "must be an object".to_string(),
                                        }),
                                        None => issues.push(ValidationIssue {
                                            path: format!("{step_path}.operation"),
                                            message: "is required".to_string(),
                                        }),
                                    }
                                }
                            }
                        }
                        Some(_) => issues.push(ValidationIssue {
                            path: format!("{flow_path}.steps"),
                            message: "must be an array".to_string(),
                        }),
                        None => issues.push(ValidationIssue {
                            path: format!("{flow_path}.steps"),
                            message: "is required".to_string(),
                        }),
                    }
                }
            }
        }
        Some(_) => issues.push(ValidationIssue {
            path: "$.flows".to_string(),
            message: "must be an array".to_string(),
        }),
        None => issues.push(ValidationIssue {
            path: "$.flows".to_string(),
            message: "is required".to_string(),
        }),
    }

    issues
}

fn lint_manifest_semantics(manifest: &GuidedManifest) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    for (flow_index, flow) in manifest.flows.iter().enumerate() {
        let flow_path = format!("$.flows[{flow_index}]");
        let is_l1 = flow.level.eq_ignore_ascii_case("l1");

        let assertion_count: usize = flow.steps.iter().map(|s| s.assertions.len()).sum();
        if assertion_count == 0 {
            issues.push(ValidationIssue {
                path: flow_path.clone(),
                message: "flow must define at least one assertion".to_string(),
            });
        }

        if is_l1 && flow.cleanup.is_empty() {
            issues.push(ValidationIssue {
                path: format!("{flow_path}.cleanup"),
                message: "L1 flow must define cleanup steps".to_string(),
            });
        }

        for (step_index, step) in flow.steps.iter().enumerate() {
            let step_path = format!("{flow_path}.steps[{step_index}]");
            check_interpolation_value(
                &step.operation.path,
                &format!("{step_path}.operation.path"),
                &mut issues,
            );

            for (name, value) in &step.operation.headers {
                check_interpolation_value(
                    value,
                    &format!("{step_path}.operation.headers.{name}"),
                    &mut issues,
                );
            }

            for (name, value) in &step.operation.query {
                check_interpolation_value(
                    value,
                    &format!("{step_path}.operation.query.{name}"),
                    &mut issues,
                );
            }

            if let Some(body) = &step.operation.body {
                check_interpolation_value(
                    body,
                    &format!("{step_path}.operation.body"),
                    &mut issues,
                );
            }
        }

        for (cleanup_index, cleanup_step) in flow.cleanup.iter().enumerate() {
            let cleanup_path = format!("{flow_path}.cleanup[{cleanup_index}]");
            check_interpolation_value(
                &cleanup_step.operation.path,
                &format!("{cleanup_path}.operation.path"),
                &mut issues,
            );

            for (name, value) in &cleanup_step.operation.headers {
                check_interpolation_value(
                    value,
                    &format!("{cleanup_path}.operation.headers.{name}"),
                    &mut issues,
                );
            }

            for (name, value) in &cleanup_step.operation.query {
                check_interpolation_value(
                    value,
                    &format!("{cleanup_path}.operation.query.{name}"),
                    &mut issues,
                );
            }

            if let Some(body) = &cleanup_step.operation.body {
                check_interpolation_value(
                    body,
                    &format!("{cleanup_path}.operation.body"),
                    &mut issues,
                );
            }
        }
    }

    issues
}

fn check_interpolation_value(value: &str, path: &str, issues: &mut Vec<ValidationIssue>) {
    let mut cursor = 0usize;
    while let Some(start) = value[cursor..].find("{{") {
        let absolute_start = cursor + start;
        let token_start = absolute_start + 2;
        let Some(end_rel) = value[token_start..].find("}}") else {
            issues.push(ValidationIssue {
                path: path.to_string(),
                message: "unclosed interpolation expression".to_string(),
            });
            return;
        };

        let token_end = token_start + end_rel;
        let expr = value[token_start..token_end].trim();
        if let Err(message) = validate_interpolation_expression(expr) {
            issues.push(ValidationIssue {
                path: path.to_string(),
                message,
            });
        }

        cursor = token_end + 2;
    }
}

pub(crate) fn validate_interpolation_expression(expr: &str) -> Result<(), String> {
    if expr.is_empty() {
        return Err("empty interpolation expression".to_string());
    }

    if expr.contains(';') || expr.contains('`') || expr.contains("${") {
        return Err(format!("unsafe expression '{expr}'"));
    }

    if expr == "rand8()" || expr == "timestamp()" {
        return Ok(());
    }

    if expr.starts_with("inputs.") || expr.starts_with("context.") || expr.starts_with("captures.")
    {
        if expr.split('.').skip(1).all(is_valid_identifier_segment) {
            return Ok(());
        }
        return Err(format!("invalid identifier path in expression '{expr}'"));
    }

    Err(format!(
        "unsupported expression source '{expr}', expected inputs/context/captures/built-ins"
    ))
}

fn is_valid_identifier_segment(segment: &str) -> bool {
    if segment.is_empty() {
        return false;
    }

    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }

    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn parse_version(value: &str) -> Option<(u64, u64)> {
    let mut parts = value.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor))
}

fn require_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
    path: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    match object.get(key) {
        Some(v) if v.is_string() => {}
        Some(_) => issues.push(ValidationIssue {
            path: format!("{path}.{key}"),
            message: "must be a string".to_string(),
        }),
        None => issues.push(ValidationIssue {
            path: format!("{path}.{key}"),
            message: "is required".to_string(),
        }),
    }
}

fn service_name_from_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let normalized_path = Path::new(&normalized);
    let file_name = normalized_path.file_name()?.to_str()?;
    if !file_name.ends_with(GUIDED_MANIFEST_SUFFIX) {
        return None;
    }

    let service = file_name.strip_suffix(GUIDED_MANIFEST_SUFFIX)?;
    if service.is_empty() {
        return None;
    }

    let parent = normalized_path.parent()?;
    let parent_normalized = parent.to_string_lossy().replace('\\', "/");
    let guided_parent_suffix = format!("/{GUIDED_MANIFEST_ROOT}");
    if parent_normalized != GUIDED_MANIFEST_ROOT
        && !parent_normalized.ends_with(&guided_parent_suffix)
    {
        return None;
    }

    Some(service.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_document_loads() {
        let schema = load_schema_document().expect("schema should parse");
        assert_eq!(schema["$id"], "studio-guided-flow.manifest.v1");
    }

    #[test]
    fn parser_rejects_missing_required_fields_with_actionable_path() {
        let input = r#"{"schemaVersion":"1.0","service":"s3"}"#;
        let err = parse_and_validate_manifest(input).expect_err("missing fields must fail");

        match err {
            ManifestError::StructuralValidation { issues } => {
                assert!(issues.iter().any(|issue| issue.path == "$.protocol"));
                assert!(issues.iter().any(|issue| issue.path == "$.flows"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn semantic_lint_rejects_unsafe_expression_and_missing_l1_cleanup() {
        let input = r#"
        {
          "schemaVersion":"1.0",
          "service":"s3",
          "protocol":"rest_xml",
          "flows":[
            {
              "id":"basic",
              "level":"L1",
              "steps":[
                {
                  "id":"create",
                  "title":"Create",
                  "operation":{
                    "method":"PUT",
                    "path":"/{{evil()}}",
                    "headers":{},
                    "query":{}
                  },
                  "assertions":[]
                }
              ]
            }
          ]
        }
        "#;

        let err = parse_and_validate_manifest(input).expect_err("semantic lint must fail");
        match err {
            ManifestError::SemanticValidation { issues } => {
                assert!(issues
                    .iter()
                    .any(|issue| issue.message.contains("unsupported expression source")));
                assert!(issues
                    .iter()
                    .any(|issue| issue.message.contains("L1 flow must define cleanup steps")));
                assert!(issues
                    .iter()
                    .any(|issue| issue.message.contains("at least one assertion")));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn schema_compatibility_rules_follow_major_minor_contract() {
        assert!(check_schema_compatibility("1.0", "1.2").is_ok());
        assert!(check_schema_compatibility("1.2", "1.2").is_ok());
        assert!(check_schema_compatibility("1.3", "1.2").is_err());
        assert!(check_schema_compatibility("2.0", "1.2").is_err());
    }

    #[test]
    fn one_manifest_per_service_enforced_by_layout() {
        let services = vec!["s3".to_string(), "sqs".to_string()];
        let manifests = vec![
            "manifests/guided/s3.guided.json".to_string(),
            "manifests/guided/s3.guided.json".to_string(),
        ];

        let err = enforce_one_manifest_per_service(&services, &manifests).expect_err("must fail");
        match err {
            ManifestError::SemanticValidation { issues } => {
                assert!(issues
                    .iter()
                    .any(|issue| issue.message.contains("duplicate manifest")));
                assert!(issues
                    .iter()
                    .any(|issue| issue.message.contains("missing manifest")));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
