/// Runtime storage inspector models.
///
/// Provides typed snapshots of in-memory service state that can be surfaced
/// in the Studio Storage tab.  Each variant represents the storable resources
/// of one service.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Generic resource record
// ---------------------------------------------------------------------------

/// A key-value attribute on any storage resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceAttribute {
    pub key: String,
    pub value: String,
}

/// A single named resource with arbitrary attributes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageResource {
    /// Human-readable resource identifier (ARN, name, URL, …)
    pub id: String,
    /// Resource kind, e.g. `"bucket"`, `"queue"`, `"table"`.
    pub kind: String,
    /// ISO-8601 creation timestamp if available.
    pub created_at: Option<String>,
    /// Arbitrary scalar attributes displayed in the detail panel.
    pub attributes: Vec<ResourceAttribute>,
}

impl StorageResource {
    pub fn new(id: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            created_at: None,
            attributes: Vec::new(),
        }
    }

    pub fn with_attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.push(ResourceAttribute {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    pub fn with_created_at(mut self, ts: impl Into<String>) -> Self {
        self.created_at = Some(ts.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Per-service storage snapshots
// ---------------------------------------------------------------------------

/// S3 storage: buckets and per-bucket object counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct S3StorageSnapshot {
    pub buckets: Vec<StorageResource>,
}

/// SQS storage: queues and approximate message depths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SqsStorageSnapshot {
    pub queues: Vec<StorageResource>,
}

/// SNS storage: topics and subscription counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnsStorageSnapshot {
    pub topics: Vec<StorageResource>,
}

/// DynamoDB storage: tables with item count and status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamoDbStorageSnapshot {
    pub tables: Vec<StorageResource>,
}

/// Lambda storage: functions with runtime and handler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LambdaStorageSnapshot {
    pub functions: Vec<StorageResource>,
}

/// Secrets Manager storage: secret names/ARNs (no values).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretsManagerStorageSnapshot {
    pub secrets: Vec<StorageResource>,
}

/// KMS storage: key IDs and states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KmsStorageSnapshot {
    pub keys: Vec<StorageResource>,
}

/// Kinesis storage: streams and shard counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KinesisStorageSnapshot {
    pub streams: Vec<StorageResource>,
}

/// Step Functions storage: state machines and recent execution counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepFunctionsStorageSnapshot {
    pub state_machines: Vec<StorageResource>,
}

/// ECR storage: repositories and image counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EcrStorageSnapshot {
    pub repositories: Vec<StorageResource>,
}

/// EventBridge storage: event buses and rule counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventBridgeStorageSnapshot {
    pub buses: Vec<StorageResource>,
}

/// SSM storage: parameter names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsmStorageSnapshot {
    pub parameters: Vec<StorageResource>,
}

/// Route 53 storage: hosted zones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route53StorageSnapshot {
    pub hosted_zones: Vec<StorageResource>,
}

/// IAM storage: users, roles, and policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IamStorageSnapshot {
    pub users: Vec<StorageResource>,
    pub roles: Vec<StorageResource>,
    pub policies: Vec<StorageResource>,
}

// ---------------------------------------------------------------------------
// Union type
// ---------------------------------------------------------------------------

/// All storage snapshots for a service, dispatched by service slug.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServiceStorageSnapshot {
    S3(S3StorageSnapshot),
    Sqs(SqsStorageSnapshot),
    Sns(SnsStorageSnapshot),
    DynamoDb(DynamoDbStorageSnapshot),
    Lambda(LambdaStorageSnapshot),
    SecretsManager(SecretsManagerStorageSnapshot),
    Kms(KmsStorageSnapshot),
    Kinesis(KinesisStorageSnapshot),
    StepFunctions(StepFunctionsStorageSnapshot),
    Ecr(EcrStorageSnapshot),
    EventBridge(EventBridgeStorageSnapshot),
    Ssm(SsmStorageSnapshot),
    Route53(Route53StorageSnapshot),
    Iam(IamStorageSnapshot),
    /// Fallback for services where fine-grained storage modelling is not yet
    /// implemented; carries a flat list of generic resources.
    Generic {
        service: String,
        resources: Vec<StorageResource>,
    },
}

impl ServiceStorageSnapshot {
    /// Service slug this snapshot belongs to.
    pub fn service_slug(&self) -> &str {
        match self {
            Self::S3(_) => "s3",
            Self::Sqs(_) => "sqs",
            Self::Sns(_) => "sns",
            Self::DynamoDb(_) => "dynamodb",
            Self::Lambda(_) => "lambda",
            Self::SecretsManager(_) => "secretsmanager",
            Self::Kms(_) => "kms",
            Self::Kinesis(_) => "kinesis",
            Self::StepFunctions(_) => "states",
            Self::Ecr(_) => "ecr",
            Self::EventBridge(_) => "events",
            Self::Ssm(_) => "ssm",
            Self::Route53(_) => "route53",
            Self::Iam(_) => "iam",
            Self::Generic { service, .. } => service.as_str(),
        }
    }

    /// Total number of top-level resources in the snapshot.
    pub fn resource_count(&self) -> usize {
        match self {
            Self::S3(s) => s.buckets.len(),
            Self::Sqs(s) => s.queues.len(),
            Self::Sns(s) => s.topics.len(),
            Self::DynamoDb(s) => s.tables.len(),
            Self::Lambda(s) => s.functions.len(),
            Self::SecretsManager(s) => s.secrets.len(),
            Self::Kms(s) => s.keys.len(),
            Self::Kinesis(s) => s.streams.len(),
            Self::StepFunctions(s) => s.state_machines.len(),
            Self::Ecr(s) => s.repositories.len(),
            Self::EventBridge(s) => s.buses.len(),
            Self::Ssm(s) => s.parameters.len(),
            Self::Route53(s) => s.hosted_zones.len(),
            Self::Iam(s) => s.users.len() + s.roles.len() + s.policies.len(),
            Self::Generic { resources, .. } => resources.len(),
        }
    }
}

// ---------------------------------------------------------------------------
// Multi-service storage state
// ---------------------------------------------------------------------------

/// Aggregated storage state across all inspected services.
///
/// Built from a sequence of [`ServiceStorageSnapshot`] values returned by the
/// Studio API.  The map is keyed by service slug so callers can look up a
/// single service quickly.
#[derive(Debug, Clone, Default)]
pub struct RuntimeStorageState {
    snapshots: HashMap<String, ServiceStorageSnapshot>,
}

impl RuntimeStorageState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace the snapshot for a service.
    pub fn update(&mut self, snapshot: ServiceStorageSnapshot) {
        self.snapshots
            .insert(snapshot.service_slug().to_string(), snapshot);
    }

    pub fn get(&self, service: &str) -> Option<&ServiceStorageSnapshot> {
        self.snapshots.get(service)
    }

    pub fn all(&self) -> impl Iterator<Item = &ServiceStorageSnapshot> {
        self.snapshots.values()
    }

    pub fn service_slugs(&self) -> impl Iterator<Item = &str> {
        self.snapshots.keys().map(String::as_str)
    }

    pub fn total_resources(&self) -> usize {
        self.snapshots.values().map(|s| s.resource_count()).sum()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bucket(name: &str) -> StorageResource {
        StorageResource::new(name, "bucket").with_attr("region", "us-east-1")
    }

    #[test]
    fn storage_resource_builder_round_trip() {
        let r = StorageResource::new("my-bucket", "bucket")
            .with_attr("region", "eu-west-1")
            .with_created_at("2026-04-01T00:00:00Z");

        assert_eq!(r.id, "my-bucket");
        assert_eq!(r.kind, "bucket");
        assert_eq!(r.created_at.as_deref(), Some("2026-04-01T00:00:00Z"));
        assert_eq!(r.attributes[0].key, "region");
        assert_eq!(r.attributes[0].value, "eu-west-1");
    }

    #[test]
    fn s3_snapshot_resource_count() {
        let snap = ServiceStorageSnapshot::S3(S3StorageSnapshot {
            buckets: vec![make_bucket("a"), make_bucket("b"), make_bucket("c")],
        });
        assert_eq!(snap.resource_count(), 3);
        assert_eq!(snap.service_slug(), "s3");
    }

    #[test]
    fn iam_snapshot_sums_all_resource_kinds() {
        let snap = ServiceStorageSnapshot::Iam(IamStorageSnapshot {
            users: vec![StorageResource::new("user-1", "user")],
            roles: vec![
                StorageResource::new("role-a", "role"),
                StorageResource::new("role-b", "role"),
            ],
            policies: vec![],
        });
        assert_eq!(snap.resource_count(), 3);
    }

    #[test]
    fn runtime_storage_state_update_and_lookup() {
        let mut state = RuntimeStorageState::new();

        state.update(ServiceStorageSnapshot::S3(S3StorageSnapshot {
            buckets: vec![make_bucket("bucket-1")],
        }));
        state.update(ServiceStorageSnapshot::Sqs(SqsStorageSnapshot {
            queues: vec![StorageResource::new("https://sqs/my-q", "queue")],
        }));

        assert_eq!(state.total_resources(), 2);
        assert!(state.get("s3").is_some());
        assert!(state.get("sqs").is_some());
        assert!(state.get("lambda").is_none());
    }

    #[test]
    fn update_replaces_previous_snapshot() {
        let mut state = RuntimeStorageState::new();
        state.update(ServiceStorageSnapshot::S3(S3StorageSnapshot {
            buckets: vec![make_bucket("old")],
        }));
        state.update(ServiceStorageSnapshot::S3(S3StorageSnapshot {
            buckets: vec![make_bucket("new-1"), make_bucket("new-2")],
        }));
        assert_eq!(state.get("s3").map(|s| s.resource_count()), Some(2));
    }

    #[test]
    fn generic_snapshot_fallback() {
        let snap = ServiceStorageSnapshot::Generic {
            service: "firehose".to_string(),
            resources: vec![
                StorageResource::new("stream-a", "delivery_stream"),
                StorageResource::new("stream-b", "delivery_stream"),
            ],
        };
        assert_eq!(snap.service_slug(), "firehose");
        assert_eq!(snap.resource_count(), 2);
    }
}
