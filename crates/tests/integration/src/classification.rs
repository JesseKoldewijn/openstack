use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceExecutionClass {
    InProcStateful,
    MixedOrchestration,
    ExternalEngineAdjacent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceDurabilityClass {
    Durable,
    RecoverableWithKnownLimits,
    NonDurable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PersistenceMode {
    Durable,
    NonDurable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ClassPerformanceEnvelope {
    pub max_latency_p95_ratio: f64,
    pub max_latency_p99_ratio: f64,
    pub min_throughput_ratio: f64,
    pub max_memory_ratio: f64,
}

pub fn service_execution_class(service: &str) -> Option<ServiceExecutionClass> {
    Some(match service {
        "s3" | "sqs" | "sns" | "dynamodb" | "iam" | "sts" | "kms" | "secretsmanager" | "ssm"
        | "acm" | "events" | "states" | "apigateway" | "ec2" | "route53" | "ses" | "ecr"
        | "cloudformation" | "cloudwatch" => ServiceExecutionClass::InProcStateful,
        "firehose" | "redshift" => ServiceExecutionClass::MixedOrchestration,
        "kinesis" | "lambda" | "opensearch" => ServiceExecutionClass::ExternalEngineAdjacent,
        _ => return None,
    })
}

pub fn service_durability_class(service: &str) -> Option<ServiceDurabilityClass> {
    Some(match service {
        "s3" | "sqs" | "sns" | "dynamodb" | "iam" | "sts" | "kms" | "secretsmanager" | "ssm"
        | "acm" | "events" | "states" | "apigateway" | "ec2" | "route53" | "ses" | "ecr"
        | "cloudformation" | "cloudwatch" | "redshift" => ServiceDurabilityClass::Durable,
        "kinesis" | "opensearch" => ServiceDurabilityClass::RecoverableWithKnownLimits,
        "lambda" | "firehose" => ServiceDurabilityClass::NonDurable,
        _ => return None,
    })
}

pub fn class_envelope(class: ServiceExecutionClass, lane: &str) -> ClassPerformanceEnvelope {
    let strict = lane.contains("core");
    match class {
        ServiceExecutionClass::InProcStateful => ClassPerformanceEnvelope {
            max_latency_p95_ratio: if strict { 1.10 } else { 1.20 },
            max_latency_p99_ratio: if strict { 1.15 } else { 1.25 },
            min_throughput_ratio: if strict { 0.95 } else { 0.85 },
            max_memory_ratio: if strict { 1.00 } else { 1.10 },
        },
        ServiceExecutionClass::MixedOrchestration => ClassPerformanceEnvelope {
            max_latency_p95_ratio: 1.25,
            max_latency_p99_ratio: 1.35,
            min_throughput_ratio: 0.80,
            max_memory_ratio: 1.20,
        },
        ServiceExecutionClass::ExternalEngineAdjacent => ClassPerformanceEnvelope {
            max_latency_p95_ratio: 1.40,
            max_latency_p99_ratio: 1.50,
            min_throughput_ratio: 0.70,
            max_memory_ratio: 1.30,
        },
    }
}

pub fn parse_persistence_mode(value: &str) -> Option<PersistenceMode> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "durable" | "persistent" | "persistence" => Some(PersistenceMode::Durable),
        "non-durable" | "ephemeral" | "in-memory" | "memory" => Some(PersistenceMode::NonDurable),
        _ => None,
    }
}
