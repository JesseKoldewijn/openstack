use std::collections::HashMap;

use crate::api::RawRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuidedWorkflowKind {
    S3,
    Sqs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStep {
    pub title: String,
    pub request: RawRequest,
}

#[derive(Debug, Clone)]
pub struct GuidedWorkflow {
    pub kind: GuidedWorkflowKind,
    pub steps: Vec<WorkflowStep>,
}

impl GuidedWorkflow {
    pub fn s3_basic(bucket: &str, key: &str, body: &str) -> Self {
        let encoded_key = url_encode(key);
        let create_bucket = WorkflowStep {
            title: "Create bucket".to_string(),
            request: RawRequest {
                method: "PUT".to_string(),
                path: format!("/{bucket}"),
                query: HashMap::new(),
                headers: HashMap::new(),
                body: None,
            },
        };

        let put_object = WorkflowStep {
            title: "Put object".to_string(),
            request: RawRequest {
                method: "PUT".to_string(),
                path: format!("/{bucket}/{encoded_key}"),
                query: HashMap::new(),
                headers: HashMap::new(),
                body: Some(body.to_string()),
            },
        };

        Self {
            kind: GuidedWorkflowKind::S3,
            steps: vec![create_bucket, put_object],
        }
    }

    pub fn sqs_basic(queue_name: &str, message: &str) -> Self {
        let create_queue_body = serde_urlencoded::to_string([
            ("Action", "CreateQueue".to_string()),
            ("QueueName", queue_name.to_string()),
            ("Version", "2012-11-05".to_string()),
        ])
        .expect("failed to serialize CreateQueue request body");
        let queue_url = format!("https://sqs.us-east-1.amazonaws.com/000000000000/{queue_name}");
        let send_message_body = serde_urlencoded::to_string([
            ("Action", "SendMessage".to_string()),
            ("QueueUrl", queue_url),
            ("MessageBody", message.to_string()),
            ("Version", "2012-11-05".to_string()),
        ])
        .expect("failed to serialize SendMessage request body");

        let create_queue = WorkflowStep {
            title: "Create queue".to_string(),
            request: RawRequest {
                method: "POST".to_string(),
                path: "/".to_string(),
                query: HashMap::new(),
                headers: HashMap::from([(
                    "content-type".to_string(),
                    "application/x-www-form-urlencoded; charset=utf-8".to_string(),
                )]),
                body: Some(create_queue_body),
            },
        };

        let send_message = WorkflowStep {
            title: "Send message".to_string(),
            request: RawRequest {
                method: "POST".to_string(),
                path: "/".to_string(),
                query: HashMap::new(),
                headers: HashMap::from([(
                    "content-type".to_string(),
                    "application/x-www-form-urlencoded; charset=utf-8".to_string(),
                )]),
                body: Some(send_message_body),
            },
        };

        Self {
            kind: GuidedWorkflowKind::Sqs,
            steps: vec![create_queue, send_message],
        }
    }
}

fn url_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}
