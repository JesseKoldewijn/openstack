use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::parity::{CaptureJson, ProtocolFamily, ScenarioStep};

const RESPONSE_HEADER_ALLOWLIST: &[&str] = &["content-type", "etag", "x-amz-bucket-region"];
const INLINE_BODY_LIMIT_BYTES: u64 = 8 * 1024 * 1024;

static RE_XML_DECL: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"<\?xml[^>]*\?>"#).expect("valid regex"));
static RE_XMLNS: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"\s+xmlns=\"[^\"]+\""#).expect("valid regex"));
static RE_RESPONSE_METADATA: Lazy<regex::Regex> = Lazy::new(|| {
    regex::Regex::new(r#"<ResponseMetadata>.*?</ResponseMetadata>"#).expect("valid regex")
});
static RE_REQUEST_ID: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"<RequestId>[^<]+</RequestId>"#).expect("valid regex"));
static RE_UUID_IN_TAG: Lazy<regex::Regex> = Lazy::new(|| {
    regex::Regex::new(r#">[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}<"#)
        .expect("valid regex")
});
static RE_RUN_ID: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"[A-Za-z0-9_-]+core-[0-9]{14}"#).expect("valid regex"));
static RE_SQS_HOST_URL: Lazy<regex::Regex> = Lazy::new(|| {
    regex::Regex::new(r#"http://[a-z0-9\.-]+:[0-9]+/000000000000/"#).expect("valid regex")
});
static RE_MESSAGE_ID: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"<MessageId>[^<]+</MessageId>"#).expect("valid regex"));
static RE_RECEIPT_HANDLE: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"<ReceiptHandle>.*?</ReceiptHandle>"#).expect("valid regex"));
static RE_MD5_OF_BODY: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"<MD5OfBody>.*?</MD5OfBody>"#).expect("valid regex"));
static RE_MD5_OF_MESSAGE_BODY: Lazy<regex::Regex> = Lazy::new(|| {
    regex::Regex::new(r#"<MD5OfMessageBody>.*?</MD5OfMessageBody>"#).expect("valid regex")
});
static RE_ACCESS_KEY_ID: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"<AccessKeyId>.*?</AccessKeyId>"#).expect("valid regex"));
static RE_SECRET_ACCESS_KEY: Lazy<regex::Regex> = Lazy::new(|| {
    regex::Regex::new(r#"<SecretAccessKey>.*?</SecretAccessKey>"#).expect("valid regex")
});
static RE_SESSION_TOKEN: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"<SessionToken>.*?</SessionToken>"#).expect("valid regex"));
static RE_EXPIRATION: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"<Expiration>.*?</Expiration>"#).expect("valid regex"));
static RE_ASSUMED_ROLE_ID: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"<AssumedRoleId>.*?</AssumedRoleId>"#).expect("valid regex"));
static RE_PACKED_POLICY_SIZE: Lazy<regex::Regex> = Lazy::new(|| {
    regex::Regex::new(r#"<PackedPolicySize>.*?</PackedPolicySize>"#).expect("valid regex")
});
static RE_SEQUENCE_NUMBER: Lazy<regex::Regex> = Lazy::new(|| {
    regex::Regex::new(r#"<SequenceNumber>.*?</SequenceNumber>"#).expect("valid regex")
});
static RE_TYPE_TAG: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"<Type>.*?</Type>"#).expect("valid regex"));
static RE_ATTRIBUTE_TAG: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"<Attribute>.*?</Attribute>"#).expect("valid regex"));
static RE_MISSING_QUEUE_MESSAGE: Lazy<regex::Regex> = Lazy::new(|| {
    regex::Regex::new(
        r#"Queue does not exist|The specified queue does not exist(?: for this wsdl version\.)?"#,
    )
    .expect("valid regex")
});
static RE_MISSING_DDB_TABLE_MESSAGE: Lazy<regex::Regex> = Lazy::new(|| {
    regex::Regex::new(r#"Cannot do operations on a non-existent table: [A-Za-z0-9_-]+"#)
        .expect("valid regex")
});
static RE_XML_SPACE_BETWEEN_TAGS: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#">\s+<"#).expect("valid regex"));

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NativeExecutionStatus {
    Executed,
    UnsupportedOperation,
    TranslationError,
    TransportError,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NativeHttpRequestTrace {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepTrace {
    pub step_id: String,
    pub command: Vec<String>,
    pub request: NativeHttpRequestTrace,
    pub status_code: Option<u16>,
    pub success: bool,
    pub response_headers: BTreeMap<String, String>,
    pub normalized_response_headers: BTreeMap<String, String>,
    pub body: String,
    pub normalized_body: String,
    pub error: String,
    pub execution_status: NativeExecutionStatus,
    pub follow_up_reason: Option<String>,
    pub translator: Option<String>,
    pub elapsed_ms: f64,
}

#[derive(Debug, Clone)]
struct NativeHttpPlan {
    method: Method,
    path: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    body_preview: String,
    translator: String,
}

pub async fn execute_step(
    endpoint: &str,
    step: &ScenarioStep,
    context: &mut HashMap<String, String>,
    timeout: Duration,
    retries: u8,
) -> StepTrace {
    seed_context_from_command(endpoint, &step.command, context);
    let command = render_command(&step.command, context);
    let mut last_trace = None;

    for attempt in 0..=retries {
        let trace = execute_step_once(endpoint, step, &command, context, timeout).await;
        let retryable = trace.execution_status == NativeExecutionStatus::Executed
            && !trace.success
            && step.expect_success
            && attempt < retries;
        if !retryable {
            return trace;
        }
        last_trace = Some(trace);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    last_trace.unwrap_or_else(|| unsupported_trace(step, command, "missing trace"))
}

async fn execute_step_once(
    endpoint: &str,
    step: &ScenarioStep,
    command: &[String],
    context: &mut HashMap<String, String>,
    timeout: Duration,
) -> StepTrace {
    let plan = match translate_command(endpoint, step, command) {
        Ok(plan) => plan,
        Err(TranslationOutcome::Unsupported(reason)) => {
            return unsupported_trace(step, command.to_vec(), &reason);
        }
        Err(TranslationOutcome::Invalid(reason)) => {
            return invalid_trace(step, command.to_vec(), &reason);
        }
    };

    let url = format!("{}{}", endpoint.trim_end_matches('/'), plan.path);
    let started = Instant::now();
    let client = match reqwest::Client::builder().timeout(timeout).build() {
        Ok(client) => client,
        Err(err) => {
            return transport_error_trace(step, command.to_vec(), &plan, &err.to_string(), 0.0);
        }
    };

    let mut request = client.request(plan.method.clone(), &url);
    for (key, value) in &plan.headers {
        request = request.header(key, value);
    }
    if let Some(body) = &plan.body {
        request = request.body(body.clone());
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            return transport_error_trace(
                step,
                command.to_vec(),
                &plan,
                &err.to_string(),
                started.elapsed().as_secs_f64() * 1000.0,
            );
        }
    };

    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let status_code = response.status().as_u16();
    let response_headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
        })
        .collect::<BTreeMap<_, _>>();
    let normalized_response_headers = normalize_headers(&response_headers);

    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(err) => {
            return transport_error_trace(
                step,
                command.to_vec(),
                &plan,
                &err.to_string(),
                elapsed_ms,
            );
        }
    };
    let body = preview_bytes(&bytes);
    let normalized_body = normalize_payload(&body, &step.protocol);

    if let Some(capture) = &step.capture_json {
        capture_output_value(&body, context, capture);
    }

    StepTrace {
        step_id: step.id.clone(),
        command: command.to_vec(),
        request: NativeHttpRequestTrace {
            method: plan.method.as_str().to_string(),
            path: plan.path,
            headers: plan.headers.into_iter().collect(),
            body: plan.body_preview,
        },
        status_code: Some(status_code),
        success: status_code < 400,
        response_headers,
        normalized_response_headers,
        body,
        normalized_body,
        error: String::new(),
        execution_status: NativeExecutionStatus::Executed,
        follow_up_reason: None,
        translator: Some(plan.translator.to_string()),
        elapsed_ms,
    }
}

fn unsupported_trace(step: &ScenarioStep, command: Vec<String>, reason: &str) -> StepTrace {
    StepTrace {
        step_id: step.id.clone(),
        command,
        request: NativeHttpRequestTrace::default(),
        status_code: None,
        success: false,
        response_headers: BTreeMap::new(),
        normalized_response_headers: BTreeMap::new(),
        body: String::new(),
        normalized_body: String::new(),
        error: reason.to_string(),
        execution_status: NativeExecutionStatus::UnsupportedOperation,
        follow_up_reason: Some(reason.to_string()),
        translator: None,
        elapsed_ms: 0.0,
    }
}

fn invalid_trace(step: &ScenarioStep, command: Vec<String>, reason: &str) -> StepTrace {
    StepTrace {
        step_id: step.id.clone(),
        command,
        request: NativeHttpRequestTrace::default(),
        status_code: None,
        success: false,
        response_headers: BTreeMap::new(),
        normalized_response_headers: BTreeMap::new(),
        body: String::new(),
        normalized_body: String::new(),
        error: reason.to_string(),
        execution_status: NativeExecutionStatus::TranslationError,
        follow_up_reason: Some(reason.to_string()),
        translator: None,
        elapsed_ms: 0.0,
    }
}

fn transport_error_trace(
    step: &ScenarioStep,
    command: Vec<String>,
    plan: &NativeHttpPlan,
    reason: &str,
    elapsed_ms: f64,
) -> StepTrace {
    StepTrace {
        step_id: step.id.clone(),
        command,
        request: NativeHttpRequestTrace {
            method: plan.method.as_str().to_string(),
            path: plan.path.clone(),
            headers: plan.headers.clone().into_iter().collect(),
            body: plan.body_preview.clone(),
        },
        status_code: None,
        success: false,
        response_headers: BTreeMap::new(),
        normalized_response_headers: BTreeMap::new(),
        body: String::new(),
        normalized_body: String::new(),
        error: reason.to_string(),
        execution_status: NativeExecutionStatus::TransportError,
        follow_up_reason: Some(reason.to_string()),
        translator: Some(plan.translator.to_string()),
        elapsed_ms,
    }
}

enum TranslationOutcome {
    Unsupported(String),
    Invalid(String),
}

fn translate_command(
    endpoint: &str,
    _step: &ScenarioStep,
    command: &[String],
) -> Result<NativeHttpPlan, TranslationOutcome> {
    let Some(service) = command.first().map(String::as_str) else {
        return Err(TranslationOutcome::Invalid("empty command".to_string()));
    };
    let Some(operation) = command.get(1).map(String::as_str) else {
        return Err(TranslationOutcome::Invalid(format!(
            "missing operation for service '{service}'"
        )));
    };

    match (service, operation) {
        ("s3api", "create-bucket") => s3_create_bucket(command),
        ("s3api", "delete-bucket") => s3_delete_bucket(command),
        ("s3api", "list-buckets") => {
            Ok(simple_get("/", "s3:list-buckets", &signed_service(service)))
        }
        ("s3api", "put-object") => s3_put_object(command),
        ("s3api", "get-object") => s3_get_object(command),
        ("s3api", "head-object") => s3_head_object(command),
        ("s3api", "delete-object") => s3_delete_object(command),
        ("s3api", "get-bucket-location") => s3_get_bucket_location(command),
        ("sqs", op) => translate_sqs(op, command, endpoint),
        ("sns", op) => translate_sns(op, command),
        ("sts", op) => translate_sts(op, command),
        ("iam", op) => translate_iam(op, command),
        ("ses", op) => translate_ses(op, command),
        ("cloudformation", op) => translate_cloudformation(op, command),
        ("ec2", op) => translate_ec2(op, command),
        ("redshift", op) => translate_redshift(op, command),
        ("route53", op) => translate_route53(op, command),
        ("dynamodb", op) => translate_dynamodb(op, command),
        ("kinesis", op) => translate_kinesis(op, command),
        ("firehose", op) => translate_firehose(op, command),
        ("kms", op) => translate_kms(op, command),
        ("secretsmanager", op) => translate_secretsmanager(op, command),
        ("ssm", op) => translate_ssm(op, command),
        ("cloudwatch", op) => translate_cloudwatch(op, command),
        ("events", op) => translate_events(op, command),
        ("acm", op) => translate_acm(op, command),
        ("ecr", op) => translate_ecr(op, command),
        ("stepfunctions", op) => translate_stepfunctions(op, command),
        ("apigateway", op) => translate_apigateway(op, command),
        ("lambda", op) => translate_lambda(op, command),
        ("opensearch", op) => translate_opensearch(op, command),
        _ => Err(TranslationOutcome::Unsupported(format!(
            "native HTTP translator not implemented for '{}' '{}'",
            service, operation
        ))),
    }
}

fn simple_get(path: &str, translator: &'static str, service: &str) -> NativeHttpPlan {
    signed_request(
        Method::GET,
        path.to_string(),
        Vec::new(),
        None,
        String::new(),
        translator.to_string(),
        service,
    )
}

fn signed_request(
    method: Method,
    path: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    body_preview: String,
    translator: String,
    service: &str,
) -> NativeHttpPlan {
    let signed_service = signed_service(service);
    let mut all_headers = vec![
        (
            "authorization".to_string(),
            fake_auth(&signed_service, "us-east-1"),
        ),
        ("x-amz-date".to_string(), "20260306T000000Z".to_string()),
    ];
    all_headers.extend(headers);
    NativeHttpPlan {
        method,
        path,
        headers: all_headers,
        body,
        body_preview,
        translator,
    }
}

fn s3_create_bucket(command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let bucket = required_flag(command, "--bucket")?;
    Ok(signed_request(
        Method::PUT,
        format!("/{bucket}"),
        Vec::new(),
        None,
        String::new(),
        "s3:create-bucket".to_string(),
        "s3",
    ))
}

fn s3_delete_bucket(command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let bucket = required_flag(command, "--bucket")?;
    Ok(signed_request(
        Method::DELETE,
        format!("/{bucket}"),
        Vec::new(),
        None,
        String::new(),
        "s3:delete-bucket".to_string(),
        "s3",
    ))
}

fn s3_put_object(command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let bucket = required_flag(command, "--bucket")?;
    let key = required_flag(command, "--key")?;
    let body_arg = required_flag(command, "--body")?;
    let (body, preview) = load_body_argument(&body_arg)?;
    Ok(signed_request(
        Method::PUT,
        format!("/{bucket}/{key}"),
        Vec::new(),
        Some(body),
        preview,
        "s3:put-object".to_string(),
        "s3",
    ))
}

fn s3_get_object(command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let bucket = required_flag(command, "--bucket")?;
    let key = required_flag(command, "--key")?;
    Ok(signed_request(
        Method::GET,
        format!("/{bucket}/{key}"),
        Vec::new(),
        None,
        String::new(),
        "s3:get-object".to_string(),
        "s3",
    ))
}

fn s3_head_object(command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let bucket = required_flag(command, "--bucket")?;
    let key = required_flag(command, "--key")?;
    Ok(signed_request(
        Method::HEAD,
        format!("/{bucket}/{key}"),
        Vec::new(),
        None,
        String::new(),
        "s3:head-object".to_string(),
        "s3",
    ))
}

fn s3_delete_object(command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let bucket = required_flag(command, "--bucket")?;
    let key = required_flag(command, "--key")?;
    Ok(signed_request(
        Method::DELETE,
        format!("/{bucket}/{key}"),
        Vec::new(),
        None,
        String::new(),
        "s3:delete-object".to_string(),
        "s3",
    ))
}

fn s3_get_bucket_location(command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let bucket =
        extract_flag_value(command, "--bucket").unwrap_or_else(|| "missing-bucket".to_string());
    Ok(signed_request(
        Method::GET,
        format!("/{bucket}?location"),
        Vec::new(),
        None,
        String::new(),
        "s3:get-bucket-location".to_string(),
        "s3",
    ))
}

fn translate_sqs(
    op: &str,
    command: &[String],
    endpoint: &str,
) -> Result<NativeHttpPlan, TranslationOutcome> {
    let (path, queue_url) = queue_path_and_url(command, endpoint);
    let mut params = Vec::new();
    match op {
        "create-queue" => {
            params.push(("Action", "CreateQueue".to_string()));
            params.push(("QueueName", required_flag(command, "--queue-name")?));
            params.push(("Version", "2012-11-05".to_string()));
        }
        "get-queue-url" => {
            params.push(("Action", "GetQueueUrl".to_string()));
            params.push((
                "QueueName",
                extract_flag_value(command, "--queue-name")
                    .unwrap_or_else(|| "missing-queue".to_string()),
            ));
            params.push(("Version", "2012-11-05".to_string()));
        }
        "send-message" => {
            params.push(("Action", "SendMessage".to_string()));
            params.push((
                "QueueUrl",
                queue_url.unwrap_or_else(|| "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/missing-queue".to_string()),
            ));
            params.push(("MessageBody", required_flag(command, "--message-body")?));
            params.push(("Version", "2012-11-05".to_string()));
        }
        "receive-message" => {
            params.push(("Action", "ReceiveMessage".to_string()));
            params.push((
                "QueueUrl",
                queue_url.unwrap_or_else(|| "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/missing-queue".to_string()),
            ));
            if let Some(value) = extract_flag_value(command, "--max-number-of-messages") {
                params.push(("MaxNumberOfMessages", value));
            }
            params.push(("Version", "2012-11-05".to_string()));
        }
        "delete-queue" => {
            params.push(("Action", "DeleteQueue".to_string()));
            params.push((
                "QueueUrl",
                queue_url.unwrap_or_else(|| "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/missing-queue".to_string()),
            ));
            params.push(("Version", "2012-11-05".to_string()));
        }
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'sqs' '{op}'"
            )));
        }
    }

    let body = encode_query_params(&params);
    Ok(signed_request(
        Method::POST,
        path,
        vec![(
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        )],
        Some(body.clone().into_bytes()),
        body,
        format!("sqs:{op}"),
        "sqs",
    ))
}

fn translate_sns(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let mut params = vec![("Version", "2010-03-31".to_string())];
    match op {
        "create-topic" => {
            params.push(("Action", "CreateTopic".to_string()));
            params.push(("Name", required_flag(command, "--name")?));
        }
        "delete-topic" => {
            params.push(("Action", "DeleteTopic".to_string()));
            params.push(("TopicArn", required_flag(command, "--topic-arn")?));
        }
        "list-topics" => {
            params.push(("Action", "ListTopics".to_string()));
        }
        "publish" => {
            params.push(("Action", "Publish".to_string()));
            params.push(("TopicArn", required_flag(command, "--topic-arn")?));
            params.push(("Message", required_flag(command, "--message")?));
        }
        "get-topic-attributes" => {
            params.push(("Action", "GetTopicAttributes".to_string()));
            params.push((
                "TopicArn",
                extract_flag_value(command, "--topic-arn").unwrap_or_else(|| {
                    "arn:aws:sns:us-east-1:000000000000:missing-topic".to_string()
                }),
            ));
        }
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'sns' '{op}'"
            )));
        }
    }
    query_plan("/", "sns", op, params)
}

fn translate_sts(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let mut params = vec![
        ("Version", "2011-06-15".to_string()),
        ("Action", camel_case(op).to_string()),
    ];
    if op == "assume-role" {
        params.push((
            "RoleArn",
            extract_flag_value(command, "--role-arn")
                .unwrap_or_else(|| "arn:aws:iam::000000000000:role/missing-role".to_string()),
        ));
        params.push((
            "RoleSessionName",
            extract_flag_value(command, "--role-session-name")
                .unwrap_or_else(|| "native-http".to_string()),
        ));
    }
    query_plan("/", "sts", op, params)
}

fn translate_iam(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let mut params = vec![
        ("Version", "2010-05-08".to_string()),
        ("Action", camel_case(op).to_string()),
    ];
    if matches!(op, "create-user" | "delete-user" | "get-user") {
        params.push((
            "UserName",
            extract_flag_value(command, "--user-name")
                .unwrap_or_else(|| "missing-user".to_string()),
        ));
    }
    query_plan("/", "iam", op, params)
}

fn translate_ses(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let mut params = vec![
        ("Version", "2010-12-01".to_string()),
        ("Action", camel_case(op).to_string()),
    ];
    match op {
        "verify-email-identity" => {
            params.push(("EmailAddress", required_flag(command, "--email-address")?))
        }
        "delete-identity" => params.push((
            "Identity",
            extract_flag_value(command, "--identity")
                .unwrap_or_else(|| "missing@example.com".to_string()),
        )),
        "list-identities" => {
            if let Some(value) = extract_flag_value(command, "--max-items") {
                params.push(("MaxItems", value));
            }
        }
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'ses' '{op}'"
            )));
        }
    }
    query_plan("/", "ses", op, params)
}

fn translate_cloudformation(
    op: &str,
    command: &[String],
) -> Result<NativeHttpPlan, TranslationOutcome> {
    let mut params = vec![
        ("Version", "2010-05-15".to_string()),
        ("Action", camel_case(op).to_string()),
    ];
    match op {
        "create-stack" => {
            params.push((
                "StackName",
                extract_flag_value(command, "--stack-name")
                    .unwrap_or_else(|| "missing-stack".to_string()),
            ));
            let template_body = if let Some(value) = extract_flag_value(command, "--template-body")
            {
                read_text_argument(&value)?
            } else {
                "{}".to_string()
            };
            params.push(("TemplateBody", template_body));
        }
        "describe-stacks" => {}
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'cloudformation' '{op}'"
            )));
        }
    }
    query_plan("/", "cloudformation", op, params)
}

fn translate_ec2(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let mut params = vec![
        ("Version", "2016-11-15".to_string()),
        ("Action", camel_case(op).to_string()),
    ];
    match op {
        "terminate-instances" => params.push((
            "InstanceId.1",
            extract_flag_value(command, "--instance-ids")
                .unwrap_or_else(|| "i-1234567890abcdef0".to_string()),
        )),
        "create-tags" => {
            params.push((
                "ResourceId.1",
                extract_flag_value(command, "--resources")
                    .unwrap_or_else(|| "i-1234567890abcdef0".to_string()),
            ));
            if let Some(tags) = extract_flag_value(command, "--tags") {
                let parsed = parse_comma_kv(&tags);
                if let Some(key) = parsed.get("Key") {
                    params.push(("Tag.1.Key", key.clone()));
                }
                if let Some(value) = parsed.get("Value") {
                    params.push(("Tag.1.Value", value.clone()));
                }
            }
        }
        "describe-instances" => {}
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'ec2' '{op}'"
            )));
        }
    }
    query_plan("/", "ec2", op, params)
}

fn translate_redshift(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let mut params = vec![
        ("Version", "2012-12-01".to_string()),
        ("Action", camel_case(op).to_string()),
    ];
    match op {
        "delete-cluster" => {
            params.push((
                "ClusterIdentifier",
                extract_flag_value(command, "--cluster-identifier")
                    .unwrap_or_else(|| "missing-cluster".to_string()),
            ));
            params.push(("SkipFinalClusterSnapshot", "true".to_string()));
        }
        "create-cluster-snapshot" => {
            params.push((
                "ClusterIdentifier",
                required_flag(command, "--cluster-identifier")?,
            ));
            params.push((
                "SnapshotIdentifier",
                required_flag(command, "--snapshot-identifier")?,
            ));
        }
        "describe-clusters" => {}
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'redshift' '{op}'"
            )));
        }
    }
    query_plan("/", "redshift", op, params)
}

fn translate_route53(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    match op {
        "list-hosted-zones" => Ok(signed_request(
            Method::GET,
            "/2013-04-01/hostedzone".to_string(),
            Vec::new(),
            None,
            String::new(),
            "route53:list-hosted-zones".to_string(),
            "route53",
        )),
        "list-resource-record-sets" => Ok(signed_request(
            Method::GET,
            "/2013-04-01/hostedzone/Z1D633PJN98FT9/rrset".to_string(),
            Vec::new(),
            None,
            String::new(),
            "route53:list-resource-record-sets".to_string(),
            "route53",
        )),
        "create-health-check" => {
            let caller_reference = required_flag(command, "--caller-reference")?;
            let config = required_flag(command, "--health-check-config")?;
            let parsed = parse_comma_kv(&config);
            let xml = format!(
                "<CreateHealthCheckRequest xmlns=\"https://route53.amazonaws.com/doc/2013-04-01/\"><CallerReference>{caller_reference}</CallerReference><HealthCheckConfig><IPAddress>{}</IPAddress><Port>{}</Port><Type>{}</Type><ResourcePath>{}</ResourcePath><FullyQualifiedDomainName>{}</FullyQualifiedDomainName><RequestInterval>{}</RequestInterval><FailureThreshold>{}</FailureThreshold></HealthCheckConfig></CreateHealthCheckRequest>",
                parsed
                    .get("IPAddress")
                    .cloned()
                    .unwrap_or_else(|| "127.0.0.1".to_string()),
                parsed
                    .get("Port")
                    .cloned()
                    .unwrap_or_else(|| "80".to_string()),
                parsed
                    .get("Type")
                    .cloned()
                    .unwrap_or_else(|| "HTTP".to_string()),
                parsed
                    .get("ResourcePath")
                    .cloned()
                    .unwrap_or_else(|| "/".to_string()),
                parsed
                    .get("FullyQualifiedDomainName")
                    .cloned()
                    .unwrap_or_else(|| "example.com".to_string()),
                parsed
                    .get("RequestInterval")
                    .cloned()
                    .unwrap_or_else(|| "30".to_string()),
                parsed
                    .get("FailureThreshold")
                    .cloned()
                    .unwrap_or_else(|| "3".to_string()),
            );
            Ok(signed_request(
                Method::POST,
                "/2013-04-01/healthcheck".to_string(),
                vec![("content-type".to_string(), "application/xml".to_string())],
                Some(xml.clone().into_bytes()),
                xml,
                "route53:create-health-check".to_string(),
                "route53",
            ))
        }
        _ => Err(TranslationOutcome::Unsupported(format!(
            "native HTTP translator not implemented for 'route53' '{op}'"
        ))),
    }
}

fn translate_dynamodb(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let body = match op {
        "create-table" => {
            let table_name = required_flag(command, "--table-name")?;
            let attribute_definitions = extract_flag_value(command, "--attribute-definitions")
                .map(|value| vec![comma_kv_json(&value)])
                .unwrap_or_default();
            let key_schema = extract_flag_value(command, "--key-schema")
                .map(|value| vec![comma_kv_json(&value)])
                .unwrap_or_default();
            json!({
                "TableName": table_name,
                "AttributeDefinitions": attribute_definitions,
                "KeySchema": key_schema,
                "BillingMode": extract_flag_value(command, "--billing-mode").unwrap_or_else(|| "PAY_PER_REQUEST".to_string()),
            })
        }
        "put-item" => json!({
            "TableName": required_flag(command, "--table-name")?,
            "Item": parse_json_flag(command, "--item")?,
        }),
        "get-item" => json!({
            "TableName": extract_flag_value(command, "--table-name").unwrap_or_else(|| "missing-table".to_string()),
            "Key": parse_json_flag(command, "--key").unwrap_or_else(|_| json!({"pk": {"S": "missing"}})),
        }),
        "describe-table" => json!({
            "TableName": extract_flag_value(command, "--table-name").unwrap_or_else(|| "missing-table".to_string()),
        }),
        "list-tables" => json!({}),
        "delete-table" => json!({
            "TableName": required_flag(command, "--table-name")?,
        }),
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'dynamodb' '{op}'"
            )));
        }
    };
    json_target_plan(
        "dynamodb",
        op,
        "DynamoDB_20120810",
        body,
        "application/x-amz-json-1.0",
    )
}

fn translate_kinesis(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let body = match op {
        "list-streams" => json!({}),
        "put-record" => json!({
            "StreamName": extract_flag_value(command, "--stream-name").unwrap_or_else(|| "missing-stream".to_string()),
            "PartitionKey": extract_flag_value(command, "--partition-key").unwrap_or_else(|| "pk".to_string()),
            "Data": extract_flag_value(command, "--data").unwrap_or_else(|| "YmVuY2g=".to_string()),
        }),
        "create-stream" => json!({
            "StreamName": required_flag(command, "--stream-name")?,
            "ShardCount": extract_flag_value(command, "--shard-count")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(1),
        }),
        "describe-stream" => json!({
            "StreamName": extract_flag_value(command, "--stream-name").unwrap_or_else(|| "missing-stream".to_string()),
        }),
        "delete-stream" => json!({
            "StreamName": required_flag(command, "--stream-name")?,
        }),
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'kinesis' '{op}'"
            )));
        }
    };
    json_target_plan(
        "kinesis",
        op,
        "Kinesis_20131202",
        body,
        "application/x-amz-json-1.1",
    )
}

fn translate_firehose(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let body = match op {
        "list-delivery-streams" => json!({}),
        "create-delivery-stream" => json!({
            "DeliveryStreamName": required_flag(command, "--delivery-stream-name")?,
        }),
        "put-record" => json!({
            "DeliveryStreamName": extract_flag_value(command, "--delivery-stream-name").unwrap_or_else(|| "missing-stream".to_string()),
            "Record": {"Data": extract_flag_value(command, "--record").unwrap_or_else(|| "Data=YmVuY2g=".to_string()).trim_start_matches("Data=")},
        }),
        "describe-delivery-stream" => json!({
            "DeliveryStreamName": extract_flag_value(command, "--delivery-stream-name").unwrap_or_else(|| "missing-stream".to_string()),
        }),
        "delete-delivery-stream" => json!({
            "DeliveryStreamName": required_flag(command, "--delivery-stream-name")?,
        }),
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'firehose' '{op}'"
            )));
        }
    };
    json_target_plan(
        "firehose",
        op,
        "Firehose_20150804",
        body,
        "application/x-amz-json-1.1",
    )
}

fn translate_kms(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let body = match op {
        "list-keys" => {
            json!({"Limit": extract_flag_value(command, "--limit").and_then(|v| v.parse::<u64>().ok()).unwrap_or(100)})
        }
        "describe-key" => json!({
            "KeyId": extract_flag_value(command, "--key-id").unwrap_or_else(|| "1234abcd-12ab-34cd-56ef-1234567890ab".to_string()),
        }),
        "create-alias" => json!({
            "AliasName": required_flag(command, "--alias-name")?,
            "TargetKeyId": required_flag(command, "--target-key-id")?,
        }),
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'kms' '{op}'"
            )));
        }
    };
    json_target_plan(
        "kms",
        op,
        "TrentService",
        body,
        "application/x-amz-json-1.1",
    )
}

fn translate_secretsmanager(
    op: &str,
    command: &[String],
) -> Result<NativeHttpPlan, TranslationOutcome> {
    let body = match op {
        "list-secrets" => json!({}),
        "create-secret" => json!({
            "Name": required_flag(command, "--name")?,
            "SecretString": extract_flag_value(command, "--secret-string").unwrap_or_else(|| "seed".to_string()),
        }),
        "get-secret-value" => json!({
            "SecretId": extract_flag_value(command, "--secret-id").unwrap_or_else(|| "missing-secret".to_string()),
        }),
        "put-secret-value" => json!({
            "SecretId": required_flag(command, "--secret-id")?,
            "SecretString": required_flag(command, "--secret-string")?,
        }),
        "delete-secret" => json!({
            "SecretId": required_flag(command, "--secret-id")?,
            "ForceDeleteWithoutRecovery": command.iter().any(|part| part == "--force-delete-without-recovery"),
        }),
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'secretsmanager' '{op}'"
            )));
        }
    };
    json_target_plan(
        "secretsmanager",
        op,
        "secretsmanager",
        body,
        "application/x-amz-json-1.1",
    )
}

fn translate_ssm(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let body = match op {
        "describe-parameters" => json!({
            "MaxResults": extract_flag_value(command, "--max-results").and_then(|v| v.parse::<u64>().ok()).unwrap_or(50),
        }),
        "get-parameter" => json!({
            "Name": extract_flag_value(command, "--name").unwrap_or_else(|| "missing-parameter".to_string()),
        }),
        "put-parameter" => json!({
            "Name": required_flag(command, "--name")?,
            "Value": required_flag(command, "--value")?,
            "Type": extract_flag_value(command, "--type").unwrap_or_else(|| "String".to_string()),
            "Overwrite": command.iter().any(|part| part == "--overwrite"),
        }),
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'ssm' '{op}'"
            )));
        }
    };
    json_target_plan("ssm", op, "AmazonSSM", body, "application/x-amz-json-1.1")
}

fn translate_cloudwatch(
    op: &str,
    command: &[String],
) -> Result<NativeHttpPlan, TranslationOutcome> {
    let mut params = vec![
        ("Version", "2010-08-01".to_string()),
        ("Action", camel_case(op).to_string()),
    ];
    match op {
        "list-metrics" => {
            if let Some(namespace) = extract_flag_value(command, "--namespace") {
                params.push(("Namespace", namespace));
            }
            if let Some(metric_name) = extract_flag_value(command, "--metric-name") {
                params.push(("MetricName", metric_name));
            }
        }
        "put-metric-data" => {
            params.push((
                "Namespace",
                extract_flag_value(command, "--namespace")
                    .unwrap_or_else(|| "Benchmark".to_string()),
            ));
            params.push((
                "MetricData.member.1.MetricName",
                extract_flag_value(command, "--metric-name")
                    .unwrap_or_else(|| "Latency".to_string()),
            ));
            params.push((
                "MetricData.member.1.Value",
                extract_flag_value(command, "--value").unwrap_or_else(|| "1".to_string()),
            ));
        }
        "get-metric-statistics" => {
            params.push(("Namespace", "AWS/Logs".to_string()));
            params.push(("MetricName", "IncomingLogEvents".to_string()));
            params.push(("StartTime", "2024-01-01T00:00:00Z".to_string()));
            params.push(("EndTime", "2024-01-01T01:00:00Z".to_string()));
            params.push(("Period", "60".to_string()));
            params.push(("Statistics.member.1", "Sum".to_string()));
        }
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'cloudwatch' '{op}'"
            )));
        }
    }
    query_plan("/", "cloudwatch", op, params)
}

fn translate_events(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let body = match op {
        "list-rules" => json!({
            "Limit": extract_flag_value(command, "--limit").and_then(|v| v.parse::<u64>().ok()).unwrap_or(50),
        }),
        "put-rule" => {
            let mut body = json!({
                "Name": required_flag(command, "--name")?,
                "ScheduleExpression": required_flag(command, "--schedule-expression")?,
            });
            if let Some(bus) = extract_flag_value(command, "--event-bus-name") {
                body["EventBusName"] = serde_json::Value::String(bus);
            }
            body
        }
        "put-events" => json!({
            "Entries": [{"Source": "benchmark", "DetailType": "native-http", "Detail": "{}"}],
        }),
        "create-event-bus" => json!({
            "Name": required_flag(command, "--name")?,
        }),
        "delete-event-bus" => json!({
            "Name": required_flag(command, "--name")?,
        }),
        "list-event-buses" => json!({}),
        "describe-rule" => {
            let mut body = json!({
                "Name": required_flag(command, "--name")?,
            });
            if let Some(bus) = extract_flag_value(command, "--event-bus-name") {
                body["EventBusName"] = serde_json::Value::String(bus);
            }
            body
        }
        "put-targets" => {
            let mut body = json!({
                "Rule": required_flag(command, "--rule")?,
                "Targets": parse_json_flag(command, "--targets")?,
            });
            if let Some(bus) = extract_flag_value(command, "--event-bus-name") {
                body["EventBusName"] = serde_json::Value::String(bus);
            }
            body
        }
        "list-targets-by-rule" => {
            let mut body = json!({
                "Rule": required_flag(command, "--rule")?,
            });
            if let Some(bus) = extract_flag_value(command, "--event-bus-name") {
                body["EventBusName"] = serde_json::Value::String(bus);
            }
            body
        }
        "disable-rule" => {
            let mut body = json!({
                "Name": required_flag(command, "--name")?,
            });
            if let Some(bus) = extract_flag_value(command, "--event-bus-name") {
                body["EventBusName"] = serde_json::Value::String(bus);
            }
            body
        }
        "enable-rule" => {
            let mut body = json!({
                "Name": required_flag(command, "--name")?,
            });
            if let Some(bus) = extract_flag_value(command, "--event-bus-name") {
                body["EventBusName"] = serde_json::Value::String(bus);
            }
            body
        }
        "remove-targets" => {
            let ids: Vec<serde_json::Value> = command
                .iter()
                .skip_while(|s| s.as_str() != "--ids")
                .skip(1)
                .take_while(|s| !s.starts_with('-'))
                .map(|s| serde_json::Value::String(s.clone()))
                .collect();
            if ids.is_empty() {
                return Err(TranslationOutcome::Invalid(
                    "remove-targets requires at least one ID via --ids".to_string(),
                ));
            }
            let mut body = json!({
                "Rule": required_flag(command, "--rule")?,
                "Ids": ids,
            });
            if let Some(bus) = extract_flag_value(command, "--event-bus-name") {
                body["EventBusName"] = serde_json::Value::String(bus);
            }
            body
        }
        "delete-rule" => {
            let mut body = json!({
                "Name": required_flag(command, "--name")?,
            });
            if let Some(bus) = extract_flag_value(command, "--event-bus-name") {
                body["EventBusName"] = serde_json::Value::String(bus);
            }
            body
        }
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'events' '{op}'"
            )));
        }
    };
    json_target_plan(
        "events",
        op,
        "AWSEvents",
        body,
        "application/x-amz-json-1.1",
    )
}

fn translate_acm(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let body = match op {
        "list-certificates" => json!({
            "MaxItems": extract_flag_value(command, "--max-items").unwrap_or_else(|| "20".to_string()),
        }),
        "request-certificate" => json!({
            "DomainName": required_flag(command, "--domain-name")?,
            "ValidationMethod": extract_flag_value(command, "--validation-method").unwrap_or_else(|| "DNS".to_string()),
        }),
        "describe-certificate" => json!({
            "CertificateArn": extract_flag_value(command, "--certificate-arn").unwrap_or_else(|| "arn:aws:acm:us-east-1:000000000000:certificate/missing".to_string()),
        }),
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'acm' '{op}'"
            )));
        }
    };
    json_target_plan(
        "acm",
        op,
        "CertificateManager",
        body,
        "application/x-amz-json-1.1",
    )
}

fn translate_ecr(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    let body = match op {
        "describe-repositories" => json!({
            "maxResults": extract_flag_value(command, "--max-results").and_then(|v| v.parse::<u64>().ok()).unwrap_or(50),
        }),
        "create-repository" => json!({
            "repositoryName": required_flag(command, "--repository-name")?,
        }),
        "delete-repository" => json!({
            "repositoryName": required_flag(command, "--repository-name")?,
        }),
        "list-images" => json!({
            "repositoryName": required_flag(command, "--repository-name")?,
        }),
        "put-image" => {
            let repo = required_flag(command, "--repository-name")?;
            let manifest = required_flag(command, "--image-manifest")?;
            let mut body = json!({
                "repositoryName": repo,
                "imageManifest": manifest,
            });
            if let Some(tag) = extract_flag_value(command, "--image-tag") {
                body["imageTag"] = serde_json::Value::String(tag);
            }
            body
        }
        "batch-get-image" => {
            let repo = required_flag(command, "--repository-name")?;
            // --image-ids accepts "imageTag=<tag>" or "imageDigest=<digest>" key=value pairs
            let image_ids: Vec<serde_json::Value> = command
                .iter()
                .skip_while(|s| s.as_str() != "--image-ids")
                .skip(1)
                .take_while(|s| !s.starts_with('-'))
                .map(|s| {
                    if let Some(tag) = s.strip_prefix("imageTag=") {
                        json!({"imageTag": tag})
                    } else if let Some(digest) = s.strip_prefix("imageDigest=") {
                        json!({"imageDigest": digest})
                    } else {
                        json!({"imageTag": s})
                    }
                })
                .collect();
            json!({
                "repositoryName": repo,
                "imageIds": image_ids,
            })
        }
        "describe-images" => json!({
            "repositoryName": extract_flag_value(command, "--repository-name").unwrap_or_else(|| "missing-repository".to_string()),
        }),
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'ecr' '{op}'"
            )));
        }
    };
    json_target_plan(
        "ecr",
        op,
        "AmazonEC2ContainerRegistry_V20150921",
        body,
        "application/x-amz-json-1.1",
    )
}

fn translate_stepfunctions(
    op: &str,
    command: &[String],
) -> Result<NativeHttpPlan, TranslationOutcome> {
    let body = match op {
        "list-state-machines" => json!({}),
        "describe-state-machine" => json!({
            "stateMachineArn": extract_flag_value(command, "--state-machine-arn").unwrap_or_else(|| "arn:aws:states:us-east-1:000000000000:stateMachine:missing".to_string()),
        }),
        "start-execution" => json!({
            "stateMachineArn": required_flag(command, "--state-machine-arn")?,
            "name": required_flag(command, "--name")?,
            "input": extract_flag_value(command, "--input").unwrap_or_else(|| "{}".to_string()),
        }),
        _ => {
            return Err(TranslationOutcome::Unsupported(format!(
                "native HTTP translator not implemented for 'stepfunctions' '{op}'"
            )));
        }
    };
    json_target_plan(
        "stepfunctions",
        op,
        "AWSStepFunctions",
        body,
        "application/x-amz-json-1.0",
    )
}

fn translate_apigateway(
    op: &str,
    command: &[String],
) -> Result<NativeHttpPlan, TranslationOutcome> {
    match op {
        "create-rest-api" => {
            let body = json!({"name": required_flag(command, "--name")?}).to_string();
            Ok(signed_request(
                Method::POST,
                "/restapis".to_string(),
                vec![("content-type".to_string(), "application/json".to_string())],
                Some(body.clone().into_bytes()),
                body,
                "apigateway:create-rest-api".to_string(),
                "apigateway",
            ))
        }
        "get-rest-apis" => {
            let limit = extract_flag_value(command, "--limit").unwrap_or_else(|| "50".to_string());
            Ok(signed_request(
                Method::GET,
                format!("/restapis?limit={limit}"),
                Vec::new(),
                None,
                String::new(),
                "apigateway:get-rest-apis".to_string(),
                "apigateway",
            ))
        }
        "get-rest-api" => {
            let rest_api_id = extract_flag_value(command, "--rest-api-id")
                .unwrap_or_else(|| "missing-api".to_string());
            Ok(signed_request(
                Method::GET,
                format!("/restapis/{rest_api_id}"),
                Vec::new(),
                None,
                String::new(),
                "apigateway:get-rest-api".to_string(),
                "apigateway",
            ))
        }
        _ => Err(TranslationOutcome::Unsupported(format!(
            "native HTTP translator not implemented for 'apigateway' '{op}'"
        ))),
    }
}

fn translate_lambda(op: &str, command: &[String]) -> Result<NativeHttpPlan, TranslationOutcome> {
    match op {
        "list-functions" => Ok(signed_request(
            Method::GET,
            "/2015-03-31/functions/".to_string(),
            Vec::new(),
            None,
            String::new(),
            "lambda:list-functions".to_string(),
            "lambda",
        )),
        "get-function" => {
            let function_name = extract_flag_value(command, "--function-name")
                .unwrap_or_else(|| "missing-function".to_string());
            Ok(signed_request(
                Method::GET,
                format!("/2015-03-31/functions/{function_name}"),
                Vec::new(),
                None,
                String::new(),
                "lambda:get-function".to_string(),
                "lambda",
            ))
        }
        "create-function" => {
            let function_name = required_flag(command, "--function-name")?;
            let runtime = required_flag(command, "--runtime")?;
            let handler = required_flag(command, "--handler")?;
            let role = required_flag(command, "--role")?;
            let zip_file = required_flag(command, "--zip-file")?;
            let body = json!({
                "FunctionName": function_name,
                "Runtime": runtime,
                "Handler": handler,
                "Role": role,
                "Code": { "ZipFile": zip_file },
            })
            .to_string();
            Ok(signed_request(
                Method::POST,
                "/2015-03-31/functions".to_string(),
                vec![("content-type".to_string(), "application/json".to_string())],
                Some(body.clone().into_bytes()),
                body,
                "lambda:create-function".to_string(),
                "lambda",
            ))
        }
        "invoke" => {
            let function_name = required_flag(command, "--function-name")?;
            let payload =
                extract_flag_value(command, "--payload").unwrap_or_else(|| "{}".to_string());
            Ok(signed_request(
                Method::POST,
                format!("/2015-03-31/functions/{function_name}/invocations"),
                vec![("content-type".to_string(), "application/json".to_string())],
                Some(payload.clone().into_bytes()),
                payload,
                "lambda:invoke".to_string(),
                "lambda",
            ))
        }
        "delete-function" => {
            let function_name = required_flag(command, "--function-name")?;
            Ok(signed_request(
                Method::DELETE,
                format!("/2015-03-31/functions/{function_name}"),
                Vec::new(),
                None,
                String::new(),
                "lambda:delete-function".to_string(),
                "lambda",
            ))
        }
        _ => Err(TranslationOutcome::Unsupported(format!(
            "native HTTP translator not implemented for 'lambda' '{op}'"
        ))),
    }
}

fn translate_opensearch(
    op: &str,
    command: &[String],
) -> Result<NativeHttpPlan, TranslationOutcome> {
    match op {
        "list-domain-names" => Ok(signed_request(
            Method::GET,
            "/2021-01-01/domain".to_string(),
            Vec::new(),
            None,
            String::new(),
            "opensearch:list-domain-names".to_string(),
            "opensearch",
        )),
        "describe-domain" => {
            let domain_name = extract_flag_value(command, "--domain-name")
                .unwrap_or_else(|| "missing-domain".to_string());
            Ok(signed_request(
                Method::GET,
                format!("/2021-01-01/opensearch/domain/{domain_name}"),
                Vec::new(),
                None,
                String::new(),
                "opensearch:describe-domain".to_string(),
                "opensearch",
            ))
        }
        "create-domain" => {
            let cluster_config = extract_flag_value(command, "--cluster-config")
                .map(|value| parse_comma_kv(&value))
                .unwrap_or_default();
            let body = json!({
                "DomainName": required_flag(command, "--domain-name")?,
                "EngineVersion": extract_flag_value(command, "--engine-version").unwrap_or_else(|| "OpenSearch_2.11".to_string()),
                "ClusterConfig": {
                    "InstanceType": cluster_config.get("InstanceType").cloned().unwrap_or_else(|| "t3.small.search".to_string()),
                    "InstanceCount": cluster_config.get("InstanceCount").and_then(|v| v.parse::<u64>().ok()).unwrap_or(1),
                }
            })
            .to_string();
            Ok(signed_request(
                Method::POST,
                "/2021-01-01/opensearch/domain".to_string(),
                vec![("content-type".to_string(), "application/json".to_string())],
                Some(body.clone().into_bytes()),
                body,
                "opensearch:create-domain".to_string(),
                "opensearch",
            ))
        }
        "delete-domain" => {
            let domain_name = required_flag(command, "--domain-name")?;
            Ok(signed_request(
                Method::DELETE,
                format!("/2021-01-01/opensearch/domain/{domain_name}"),
                Vec::new(),
                None,
                String::new(),
                "opensearch:delete-domain".to_string(),
                "opensearch",
            ))
        }
        _ => Err(TranslationOutcome::Unsupported(format!(
            "native HTTP translator not implemented for 'opensearch' '{op}'"
        ))),
    }
}

fn query_plan(
    path: &str,
    service: &'static str,
    op: &str,
    params: Vec<(&str, String)>,
) -> Result<NativeHttpPlan, TranslationOutcome> {
    let body = encode_query_params(&params);
    Ok(signed_request(
        Method::POST,
        path.to_string(),
        vec![(
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        )],
        Some(body.clone().into_bytes()),
        body,
        format!("{service}:{op}"),
        service,
    ))
}

fn json_target_plan(
    service: &'static str,
    op: &str,
    target_prefix: &'static str,
    body: serde_json::Value,
    content_type: &'static str,
) -> Result<NativeHttpPlan, TranslationOutcome> {
    let body_text = body.to_string();
    Ok(signed_request(
        Method::POST,
        "/".to_string(),
        vec![
            ("content-type".to_string(), content_type.to_string()),
            (
                "x-amz-target".to_string(),
                format!("{target_prefix}.{}", pascal_case(op)),
            ),
        ],
        Some(body_text.clone().into_bytes()),
        body_text,
        format!("{service}:{op}"),
        &signed_service(service),
    ))
}

fn queue_path_and_url(command: &[String], endpoint: &str) -> (String, Option<String>) {
    let queue_url = extract_flag_value(command, "--queue-url");
    if let Some(queue_url) = queue_url.clone() {
        let path = extract_path_from_url(&queue_url).unwrap_or_else(|| "/".to_string());
        return (path, Some(queue_url));
    }

    let queue_name =
        extract_flag_value(command, "--queue-name").unwrap_or_else(|| "missing-queue".to_string());
    (
        "/".to_string(),
        Some(format!(
            "{}/000000000000/{}",
            endpoint.trim_end_matches('/'),
            queue_name
        )),
    )
}

fn extract_path_from_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let path = parsed.path().to_string();
    let query = parsed
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    Some(format!("{path}{query}"))
}

fn load_body_argument(value: &str) -> Result<(Vec<u8>, String), TranslationOutcome> {
    let path = Path::new(value);
    if !path.exists() {
        return Ok((value.as_bytes().to_vec(), value.to_string()));
    }

    let metadata = std::fs::metadata(path).map_err(|err| {
        TranslationOutcome::Invalid(format!(
            "failed to read metadata for '{}': {err}",
            path.display()
        ))
    })?;
    if metadata.len() > INLINE_BODY_LIMIT_BYTES {
        return Err(TranslationOutcome::Unsupported(format!(
            "body file '{}' exceeds inline native HTTP limit of {} bytes",
            path.display(),
            INLINE_BODY_LIMIT_BYTES
        )));
    }
    let body = std::fs::read(path).map_err(|err| {
        TranslationOutcome::Invalid(format!(
            "failed to read body file '{}': {err}",
            path.display()
        ))
    })?;
    Ok((
        body.clone(),
        format!("<file:{}:{} bytes>", path.display(), body.len()),
    ))
}

pub fn load_body_preview(value: &str) -> Result<String, String> {
    match load_body_argument(value) {
        Ok((_, preview)) => Ok(preview),
        Err(TranslationOutcome::Unsupported(reason)) | Err(TranslationOutcome::Invalid(reason)) => {
            Err(reason)
        }
    }
}

fn read_text_argument(value: &str) -> Result<String, TranslationOutcome> {
    let path_value = value.strip_prefix("file://").unwrap_or(value);
    let path = Path::new(path_value);
    if !path.exists() {
        return Ok(value.to_string());
    }
    std::fs::read_to_string(path).map_err(|err| {
        TranslationOutcome::Invalid(format!(
            "failed to read text argument '{}': {err}",
            path.display()
        ))
    })
}

pub fn seed_context_from_command(
    endpoint: &str,
    raw_command: &[String],
    context: &mut HashMap<String, String>,
) {
    if !context.contains_key("queue_name")
        && let Some(queue_name) = extract_flag_value(raw_command, "--queue-name")
    {
        context.insert("queue_name".to_string(), queue_name);
    }

    if !context.contains_key("queue_url")
        && let Some(queue_name) = context.get("queue_name")
    {
        context.insert(
            "queue_url".to_string(),
            format!(
                "{}/000000000000/{}",
                endpoint.trim_end_matches('/'),
                queue_name
            ),
        );
    }

    if !context.contains_key("bucket_name")
        && let Some(bucket_name) = extract_flag_value(raw_command, "--bucket")
    {
        context.insert("bucket_name".to_string(), bucket_name);
    }

    if !context.contains_key("bucket_host_url")
        && let Some(bucket_name) = context.get("bucket_name")
    {
        let endpoint_trimmed = endpoint
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        context.insert(
            "bucket_host_url".to_string(),
            format!("http://{}.{}", bucket_name, endpoint_trimmed),
        );
    }

    if !context.contains_key("queue_hostname_url")
        && let Some(queue_name) = context.get("queue_name")
    {
        context.insert(
            "queue_hostname_url".to_string(),
            format!(
                "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/{}",
                queue_name
            ),
        );
    }
}

pub fn render_command(raw: &[String], context: &HashMap<String, String>) -> Vec<String> {
    raw.iter()
        .map(|part| {
            let mut rendered = part.clone();
            for (key, value) in context {
                rendered = rendered.replace(&format!("{{{{{key}}}}}"), value);
            }
            rendered
        })
        .collect()
}

pub fn capture_output_value(
    stdout: &str,
    context: &mut HashMap<String, String>,
    capture: &CaptureJson,
) {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(stdout)
        && let Some(value) = json.pointer(&capture.json_pointer)
    {
        if let Some(as_str) = value.as_str() {
            context.insert(capture.output_key.clone(), as_str.to_string());
        } else {
            context.insert(capture.output_key.clone(), value.to_string());
        }
        return;
    }

    if let Some(value) = capture_xml_value(stdout, &capture.json_pointer) {
        context.insert(capture.output_key.clone(), value);
    }
}

fn capture_xml_value(stdout: &str, selector: &str) -> Option<String> {
    let tag = selector
        .trim()
        .trim_start_matches('/')
        .trim_start_matches('.')
        .split('/')
        .next_back()?
        .trim();
    if tag.is_empty() {
        return None;
    }

    let pattern = format!(r#"<{tag}>([^<]+)</{tag}>"#);
    let re = regex::Regex::new(&pattern).ok()?;
    let captures = re.captures(stdout)?;
    captures.get(1).map(|value| value.as_str().to_string())
}

pub fn normalize_payload(raw: &str, protocol: &ProtocolFamily) -> String {
    let trimmed = raw.trim();
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        return normalize_json(trimmed);
    }

    match protocol {
        ProtocolFamily::Json | ProtocolFamily::RestJson => normalize_json(raw),
        ProtocolFamily::QueryXml | ProtocolFamily::RestXml => normalize_xml(raw),
    }
}

pub fn normalize_json(raw: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(mut value) => {
            scrub_dynamic_json(&mut value);
            serde_json::to_string(&value).unwrap_or_else(|_| raw.trim().to_string())
        }
        Err(_) => raw.trim().to_string(),
    }
}

fn scrub_dynamic_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for key in ["RequestId", "requestId", "ResponseMetadata"] {
                map.remove(key);
            }
            for key in [
                "QueueUrl",
                "ReceiptHandle",
                "MessageId",
                "MD5OfBody",
                "MD5OfMessageBody",
                "ChecksumCRC32",
                "ServerSideEncryption",
                "AcceptRanges",
                "PackedPolicySize",
                "AccessKeyId",
                "SecretAccessKey",
                "SessionToken",
                "Expiration",
                "AssumedRoleId",
                "LastModified",
                "ContentType",
                "DisplayName",
                "ID",
            ] {
                map.remove(key);
            }

            if let Some(table) = map.get_mut("TableDescription")
                && let Some(table_map) = table.as_object_mut()
            {
                for key in [
                    "CreationDateTime",
                    "TableId",
                    "ProvisionedThroughput",
                    "DeletionProtectionEnabled",
                    "TableSizeBytes",
                    "StreamSpecification",
                ] {
                    table_map.remove(key);
                }

                if let Some(billing) = table_map.get_mut("BillingModeSummary")
                    && let Some(billing_map) = billing.as_object_mut()
                {
                    billing_map.remove("LastUpdateToPayPerRequestDateTime");
                }
            }

            for val in map.values_mut() {
                scrub_dynamic_json(val);
            }
        }
        serde_json::Value::Array(values) => {
            for item in values {
                scrub_dynamic_json(item);
            }
        }
        _ => {}
    }
}

pub fn normalize_xml(raw: &str) -> String {
    let mut text = raw.trim().replace('\n', "");
    text = text.replace('\t', "");
    while text.contains("  ") {
        text = text.replace("  ", " ");
    }

    text = RE_XML_DECL.replace_all(&text, "").to_string();
    text = RE_XMLNS.replace_all(&text, "").to_string();
    text = RE_RESPONSE_METADATA.replace_all(&text, "").to_string();
    text = RE_REQUEST_ID.replace_all(&text, "").to_string();
    text = RE_UUID_IN_TAG.replace_all(&text, "><").to_string();

    text = RE_RUN_ID.replace_all(&text, "<run-id>").to_string();
    text = RE_SQS_HOST_URL
        .replace_all(
            &text,
            "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/",
        )
        .to_string();
    text = RE_MESSAGE_ID.replace_all(&text, "").to_string();
    text = RE_RECEIPT_HANDLE.replace_all(&text, "").to_string();
    text = RE_MD5_OF_BODY.replace_all(&text, "").to_string();
    text = RE_MD5_OF_MESSAGE_BODY.replace_all(&text, "").to_string();
    text = RE_ACCESS_KEY_ID.replace_all(&text, "").to_string();
    text = RE_SECRET_ACCESS_KEY.replace_all(&text, "").to_string();
    text = RE_SESSION_TOKEN.replace_all(&text, "").to_string();
    text = RE_EXPIRATION.replace_all(&text, "").to_string();
    text = RE_ASSUMED_ROLE_ID.replace_all(&text, "").to_string();
    text = RE_PACKED_POLICY_SIZE.replace_all(&text, "").to_string();
    text = RE_SEQUENCE_NUMBER.replace_all(&text, "").to_string();
    text = RE_TYPE_TAG.replace_all(&text, "").to_string();
    text = RE_ATTRIBUTE_TAG.replace_all(&text, "").to_string();
    text = RE_MISSING_QUEUE_MESSAGE
        .replace_all(&text, "The specified queue does not exist")
        .to_string();
    text = RE_MISSING_DDB_TABLE_MESSAGE
        .replace_all(&text, "Cannot do operations on a non-existent table")
        .to_string();
    while text.contains("> <") {
        text = text.replace("> <", "><");
    }
    text = RE_XML_SPACE_BETWEEN_TAGS
        .replace_all(&text, "><")
        .to_string();
    text
}

pub fn normalize_headers(headers: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter(|(key, _)| RESPONSE_HEADER_ALLOWLIST.contains(&key.as_str()))
        .filter_map(|(key, value)| {
            let value = value.trim();
            if value.is_empty() {
                None
            } else {
                Some((key.clone(), value.to_string()))
            }
        })
        .collect()
}

pub fn extract_flag_value(command: &[String], flag: &str) -> Option<String> {
    command.windows(2).find_map(|window| {
        if window[0] == flag {
            Some(window[1].clone())
        } else {
            None
        }
    })
}

fn required_flag(command: &[String], flag: &str) -> Result<String, TranslationOutcome> {
    extract_flag_value(command, flag)
        .ok_or_else(|| TranslationOutcome::Invalid(format!("missing required flag '{flag}'")))
}

fn parse_json_flag(
    command: &[String],
    flag: &str,
) -> Result<serde_json::Value, TranslationOutcome> {
    let value = required_flag(command, flag)?;
    serde_json::from_str(&value)
        .map_err(|err| TranslationOutcome::Invalid(format!("invalid JSON for '{flag}': {err}")))
}

fn encode_query_params(params: &[(&str, String)]) -> String {
    params
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                urlencoding::encode(key),
                urlencoding::encode(value)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn parse_comma_kv(input: &str) -> BTreeMap<String, String> {
    input
        .split(',')
        .filter_map(|part| part.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect()
}

fn comma_kv_json(input: &str) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (key, value) in parse_comma_kv(input) {
        map.insert(key, serde_json::Value::String(value));
    }
    serde_json::Value::Object(map)
}

fn preview_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) if text.len() <= 4096 => text.to_string(),
        Ok(text) => format!("{}...<truncated:{} bytes>", &text[..4096], bytes.len()),
        Err(_) => format!("<binary:{} bytes>", bytes.len()),
    }
}

fn fake_auth(service: &str, region: &str) -> String {
    format!(
        "AWS4-HMAC-SHA256 Credential=test/20260306/{region}/{service}/aws4_request, SignedHeaders=host;x-amz-date, Signature=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    )
}

fn signed_service(service: &str) -> String {
    match service {
        "s3api" => "s3".to_string(),
        "stepfunctions" => "states".to_string(),
        "opensearch" => "es".to_string(),
        other => other.to_string(),
    }
}

fn camel_case(op: &str) -> String {
    pascal_case(op)
}

fn pascal_case(op: &str) -> String {
    op.split('-')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<String>()
}
