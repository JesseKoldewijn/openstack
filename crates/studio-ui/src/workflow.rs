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
    /// Creates a simple S3 guided workflow that creates a bucket and then puts an object into it.
    ///
    /// The workflow contains two steps:
    /// 1. "Create bucket" — a PUT to `/{bucket}` with no body.
    /// 2. "Put object" — a PUT to `/{bucket}/{key}` with `body` as the request body.
    ///
    /// # Returns
    ///
    /// A `GuidedWorkflow` of kind `GuidedWorkflowKind::S3` containing the two steps.
    ///
    /// # Examples
    ///
    /// ```
    /// let wf = GuidedWorkflow::s3_basic("my-bucket", "path/to/key.txt", "hello");
    /// assert!(matches!(wf.kind, GuidedWorkflowKind::S3));
    /// assert_eq!(wf.steps.len(), 2);
    /// assert_eq!(wf.steps[0].title, "Create bucket");
    /// assert_eq!(wf.steps[1].title, "Put object");
    /// ```
    pub fn s3_basic(bucket: &str, key: &str, body: &str) -> Self {
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
                path: format!("/{bucket}/{key}"),
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

    /// Creates a basic SQS guided workflow that creates a queue and then sends a message.
    ///
    /// The returned workflow contains two steps configured for SQS: one to create the queue
    /// and one to send the provided message to that queue.
    ///
    /// # Parameters
    /// - `queue_name`: the name or URL of the queue to create and to which the message will be sent.
    /// - `message`: the message body to send in the second step.
    ///
    /// # Returns
    /// A `GuidedWorkflow` with kind `GuidedWorkflowKind::Sqs` and two `WorkflowStep`s (create queue, send message).
    ///
    /// # Examples
    ///
    /// ```
    /// let wf = GuidedWorkflow::sqs_basic("my-queue", "hello");
    /// assert_eq!(wf.kind, GuidedWorkflowKind::Sqs);
    /// assert_eq!(wf.steps.len(), 2);
    /// ```
    pub fn sqs_basic(queue_name: &str, message: &str) -> Self {
        let create_queue = WorkflowStep {
            title: "Create queue".to_string(),
            request: RawRequest {
                method: "POST".to_string(),
                path: "/".to_string(),
                query: HashMap::new(),
                headers: HashMap::new(),
                body: Some(format!(
                    "Action=CreateQueue&QueueName={queue_name}&Version=2012-11-05"
                )),
            },
        };

        let send_message = WorkflowStep {
            title: "Send message".to_string(),
            request: RawRequest {
                method: "POST".to_string(),
                path: "/".to_string(),
                query: HashMap::new(),
                headers: HashMap::new(),
                body: Some(format!(
                    "Action=SendMessage&QueueUrl={queue_name}&MessageBody={message}&Version=2012-11-05"
                )),
            },
        };

        Self {
            kind: GuidedWorkflowKind::Sqs,
            steps: vec![create_queue, send_message],
        }
    }
}
