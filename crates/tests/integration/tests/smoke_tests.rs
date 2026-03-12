//! Smoke tests: for each service, create a resource, read it back, delete it.
//! These tests spin up a real HTTP server and use reqwest to talk to it.

mod common {
    use openstack_integration_tests::harness::TestHarness;

    pub async fn health_check(harness: &TestHarness) {
        let resp = harness
            .client
            .get(harness.url("/_localstack/health"))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success(), "health check failed");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// S3
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_s3_bucket_lifecycle() {
    let harness = openstack_integration_tests::harness::TestHarness::start_services("s3").await;
    common::health_check(&harness).await;

    // Create bucket
    let resp = harness
        .aws_put("/smoke-bucket", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "CreateBucket failed");

    // Put object
    let resp = harness
        .aws_put("/smoke-bucket/hello.txt", "s3", "us-east-1")
        .header("content-type", "text/plain")
        .body("hello world")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "PutObject failed");

    // Get object
    let resp = harness
        .aws_get("/smoke-bucket/hello.txt", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "GetObject failed");
    let body = resp.text().await.unwrap();
    assert_eq!(body, "hello world");

    // Delete object
    let resp = harness
        .aws_delete("/smoke-bucket/hello.txt", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 204, "DeleteObject failed");

    // Delete bucket
    let resp = harness
        .aws_delete("/smoke-bucket", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 204, "DeleteBucket failed");

    harness.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// SQS
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_sqs_queue_lifecycle() {
    let harness = openstack_integration_tests::harness::TestHarness::start_services("sqs").await;

    let body = "Action=CreateQueue&QueueName=smoke-queue&Version=2012-11-05";
    let resp = harness
        .aws_post("/", "sqs", "us-east-1")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "CreateQueue failed");
    let xml = resp.text().await.unwrap();
    assert!(xml.contains("QueueUrl"), "no QueueUrl in response");

    // Extract queue URL from response
    let start = xml.find("<QueueUrl>").unwrap() + 10;
    let end = xml.find("</QueueUrl>").unwrap();
    let queue_url_path = xml[start..end].trim_start_matches("http://");
    let queue_path = queue_url_path.split_once('/').map(|x| x.1).unwrap_or("");

    // Send message
    let send_body = format!(
        "Action=SendMessage&QueueUrl={queue_url}&MessageBody=hello&Version=2012-11-05",
        queue_url = urlencoding::encode(&xml[start..end])
    );
    let resp = harness
        .aws_post(&format!("/{}", queue_path), "sqs", "us-east-1")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(send_body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "SendMessage failed");

    // Receive message
    let recv_body = format!(
        "Action=ReceiveMessage&QueueUrl={}&Version=2012-11-05",
        urlencoding::encode(&xml[start..end])
    );
    let resp = harness
        .aws_post(&format!("/{}", queue_path), "sqs", "us-east-1")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(recv_body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "ReceiveMessage failed");
    let recv_xml = resp.text().await.unwrap();
    assert!(recv_xml.contains("hello"), "message body not found");

    // Delete queue
    let del_body = format!(
        "Action=DeleteQueue&QueueUrl={}&Version=2012-11-05",
        urlencoding::encode(&xml[start..end])
    );
    let resp = harness
        .aws_post(&format!("/{}", queue_path), "sqs", "us-east-1")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(del_body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "DeleteQueue failed");

    harness.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// DynamoDB
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_dynamodb_table_lifecycle() {
    let harness =
        openstack_integration_tests::harness::TestHarness::start_services("dynamodb").await;

    let create_body = serde_json::json!({
        "TableName": "smoke-table",
        "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
        "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}],
        "BillingMode": "PAY_PER_REQUEST"
    });

    let resp = harness
        .aws_post("/", "dynamodb", "us-east-1")
        .header("content-type", "application/x-amz-json-1.0")
        .header("x-amz-target", "DynamoDB_20120810.CreateTable")
        .json(&create_body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "CreateTable failed");

    // Put item
    let put_body = serde_json::json!({
        "TableName": "smoke-table",
        "Item": {"pk": {"S": "key1"}, "value": {"S": "hello"}}
    });
    let resp = harness
        .aws_post("/", "dynamodb", "us-east-1")
        .header("content-type", "application/x-amz-json-1.0")
        .header("x-amz-target", "DynamoDB_20120810.PutItem")
        .json(&put_body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "PutItem failed");

    // Get item
    let get_body = serde_json::json!({
        "TableName": "smoke-table",
        "Key": {"pk": {"S": "key1"}}
    });
    let resp = harness
        .aws_post("/", "dynamodb", "us-east-1")
        .header("content-type", "application/x-amz-json-1.0")
        .header("x-amz-target", "DynamoDB_20120810.GetItem")
        .json(&get_body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "GetItem failed");
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["Item"]["value"]["S"], "hello");

    // Delete item
    let del_body = serde_json::json!({
        "TableName": "smoke-table",
        "Key": {"pk": {"S": "key1"}}
    });
    let resp = harness
        .aws_post("/", "dynamodb", "us-east-1")
        .header("content-type", "application/x-amz-json-1.0")
        .header("x-amz-target", "DynamoDB_20120810.DeleteItem")
        .json(&del_body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "DeleteItem failed");

    // Delete table
    let resp = harness
        .aws_post("/", "dynamodb", "us-east-1")
        .header("content-type", "application/x-amz-json-1.0")
        .header("x-amz-target", "DynamoDB_20120810.DeleteTable")
        .json(&serde_json::json!({"TableName": "smoke-table"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "DeleteTable failed");

    harness.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// SNS
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_sns_topic_lifecycle() {
    let harness = openstack_integration_tests::harness::TestHarness::start_services("sns").await;

    let body = "Action=CreateTopic&Name=smoke-topic&Version=2010-03-31";
    let resp = harness
        .aws_post("/", "sns", "us-east-1")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "CreateTopic failed");
    let xml = resp.text().await.unwrap();
    assert!(xml.contains("TopicArn"));

    // Extract ARN
    let start = xml.find("<TopicArn>").unwrap() + 10;
    let end = xml.find("</TopicArn>").unwrap();
    let arn = &xml[start..end];

    // Delete topic
    let del_body = format!(
        "Action=DeleteTopic&TopicArn={}&Version=2010-03-31",
        urlencoding::encode(arn)
    );
    let resp = harness
        .aws_post("/", "sns", "us-east-1")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(del_body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "DeleteTopic failed");

    harness.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// KMS
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_kms_key_lifecycle() {
    let harness = openstack_integration_tests::harness::TestHarness::start_services("kms").await;

    // Create key
    let resp = harness
        .aws_post("/", "kms", "us-east-1")
        .header("content-type", "application/x-amz-json-1.1")
        .header("x-amz-target", "TrentService.CreateKey")
        .json(&serde_json::json!({"Description": "smoke test key"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "CreateKey failed");
    let json: serde_json::Value = resp.json().await.unwrap();
    let key_id = json["KeyMetadata"]["KeyId"].as_str().unwrap().to_string();

    // Describe key
    let resp = harness
        .aws_post("/", "kms", "us-east-1")
        .header("content-type", "application/x-amz-json-1.1")
        .header("x-amz-target", "TrentService.DescribeKey")
        .json(&serde_json::json!({"KeyId": key_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "DescribeKey failed");

    // Schedule deletion
    let resp = harness
        .aws_post("/", "kms", "us-east-1")
        .header("content-type", "application/x-amz-json-1.1")
        .header("x-amz-target", "TrentService.ScheduleKeyDeletion")
        .json(&serde_json::json!({"KeyId": key_id, "PendingWindowInDays": 7}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "ScheduleKeyDeletion failed");

    harness.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// SecretsManager
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_secretsmanager_lifecycle() {
    let harness =
        openstack_integration_tests::harness::TestHarness::start_services("secretsmanager").await;

    // Create secret
    let resp = harness
        .aws_post("/", "secretsmanager", "us-east-1")
        .header("content-type", "application/x-amz-json-1.1")
        .header("x-amz-target", "secretsmanager.CreateSecret")
        .json(&serde_json::json!({
            "Name": "smoke/secret",
            "SecretString": "s3cr3t"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "CreateSecret failed");

    // Get secret value
    let resp = harness
        .aws_post("/", "secretsmanager", "us-east-1")
        .header("content-type", "application/x-amz-json-1.1")
        .header("x-amz-target", "secretsmanager.GetSecretValue")
        .json(&serde_json::json!({"SecretId": "smoke/secret"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "GetSecretValue failed");
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["SecretString"], "s3cr3t");

    // Delete secret
    let resp = harness
        .aws_post("/", "secretsmanager", "us-east-1")
        .header("content-type", "application/x-amz-json-1.1")
        .header("x-amz-target", "secretsmanager.DeleteSecret")
        .json(&serde_json::json!({
            "SecretId": "smoke/secret",
            "ForceDeleteWithoutRecovery": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "DeleteSecret failed");

    harness.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// SSM Parameter Store
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_ssm_parameter_lifecycle() {
    let harness = openstack_integration_tests::harness::TestHarness::start_services("ssm").await;

    // Put parameter
    let resp = harness
        .aws_post("/", "ssm", "us-east-1")
        .header("content-type", "application/x-amz-json-1.1")
        .header("x-amz-target", "AmazonSSM.PutParameter")
        .json(&serde_json::json!({
            "Name": "/smoke/param",
            "Value": "param-value",
            "Type": "String"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "PutParameter failed");

    // Get parameter
    let resp = harness
        .aws_post("/", "ssm", "us-east-1")
        .header("content-type", "application/x-amz-json-1.1")
        .header("x-amz-target", "AmazonSSM.GetParameter")
        .json(&serde_json::json!({"Name": "/smoke/param"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "GetParameter failed");
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["Parameter"]["Value"], "param-value");

    // Delete parameter
    let resp = harness
        .aws_post("/", "ssm", "us-east-1")
        .header("content-type", "application/x-amz-json-1.1")
        .header("x-amz-target", "AmazonSSM.DeleteParameter")
        .json(&serde_json::json!({"Name": "/smoke/param"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "DeleteParameter failed");

    harness.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// Kinesis
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_kinesis_stream_lifecycle() {
    let harness =
        openstack_integration_tests::harness::TestHarness::start_services("kinesis").await;

    // Create stream
    let resp = harness
        .aws_post("/", "kinesis", "us-east-1")
        .header("content-type", "application/x-amz-json-1.1")
        .header("x-amz-target", "Kinesis_20131202.CreateStream")
        .json(&serde_json::json!({"StreamName": "smoke-stream", "ShardCount": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "CreateStream failed");

    // Describe stream
    let resp = harness
        .aws_post("/", "kinesis", "us-east-1")
        .header("content-type", "application/x-amz-json-1.1")
        .header("x-amz-target", "Kinesis_20131202.DescribeStream")
        .json(&serde_json::json!({"StreamName": "smoke-stream"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "DescribeStream failed");
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["StreamDescription"]["StreamName"], "smoke-stream");

    // Delete stream
    let resp = harness
        .aws_post("/", "kinesis", "us-east-1")
        .header("content-type", "application/x-amz-json-1.1")
        .header("x-amz-target", "Kinesis_20131202.DeleteStream")
        .json(&serde_json::json!({"StreamName": "smoke-stream"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "DeleteStream failed");

    harness.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal health API
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_health_and_info() {
    let harness = openstack_integration_tests::harness::TestHarness::start().await;

    let resp = harness
        .client
        .get(harness.url("/_localstack/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["edition"], "community");

    let resp = harness
        .client
        .head(harness.url("/_localstack/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let resp = harness
        .client
        .get(harness.url("/_localstack/info"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["version"].is_string());

    harness.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// S3 Streaming I/O
// ─────────────────────────────────────────────────────────────────────────────

/// Test PutObject/GetObject with a body larger than the default spool threshold
/// (1 MiB), ensuring end-to-end streaming through the gateway works.
#[tokio::test]
async fn smoke_s3_large_object_streaming() {
    let harness = openstack_integration_tests::harness::TestHarness::start_services("s3").await;

    // Create bucket
    let resp = harness
        .aws_put("/stream-bucket", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "CreateBucket failed");

    // Create a 2 MiB body (above 1 MiB spool threshold)
    let large_body: Vec<u8> = (0..2_097_152).map(|i| (i % 251) as u8).collect();

    // PutObject with large body
    let resp = harness
        .aws_put("/stream-bucket/large.bin", "s3", "us-east-1")
        .header("content-type", "application/octet-stream")
        .body(large_body.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "PutObject large failed");
    assert!(
        resp.headers().contains_key("etag"),
        "PutObject should return ETag"
    );

    // GetObject and verify full content
    let resp = harness
        .aws_get("/stream-bucket/large.bin", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "GetObject large failed");
    let got = resp.bytes().await.unwrap();
    assert_eq!(got.len(), large_body.len(), "body length mismatch");
    assert_eq!(&got[..], &large_body[..], "body content mismatch");

    // Cleanup
    let resp = harness
        .aws_delete("/stream-bucket/large.bin", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 204);
    let resp = harness
        .aws_delete("/stream-bucket", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 204);

    harness.shutdown();
}

/// Test multipart upload through the HTTP gateway with filesystem-backed parts.
#[tokio::test]
async fn smoke_s3_multipart_upload() {
    let harness = openstack_integration_tests::harness::TestHarness::start_services("s3").await;

    // Create bucket
    let resp = harness
        .aws_put("/mp-int-bucket", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    // Initiate multipart upload
    let resp = harness
        .aws_post("/mp-int-bucket/assembled.bin?uploads", "s3", "us-east-1")
        .header("content-type", "application/octet-stream")
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        200,
        "InitiateMultipartUpload failed"
    );
    let xml = resp.text().await.unwrap();
    let uid_start = xml.find("<UploadId>").unwrap() + 10;
    let uid_end = xml.find("</UploadId>").unwrap();
    let upload_id = &xml[uid_start..uid_end];

    // Upload part 1 (512 KiB)
    let part1: Vec<u8> = vec![0xAA; 524_288];
    let resp = harness
        .aws_put(
            &format!("/mp-int-bucket/assembled.bin?uploadId={upload_id}&partNumber=1"),
            "s3",
            "us-east-1",
        )
        .body(part1.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "UploadPart 1 failed");
    let etag1 = resp
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // Upload part 2 (512 KiB)
    let part2: Vec<u8> = vec![0xBB; 524_288];
    let resp = harness
        .aws_put(
            &format!("/mp-int-bucket/assembled.bin?uploadId={upload_id}&partNumber=2"),
            "s3",
            "us-east-1",
        )
        .body(part2.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "UploadPart 2 failed");
    let etag2 = resp
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // Complete multipart upload
    let complete_xml = format!(
        "<CompleteMultipartUpload>\
         <Part><PartNumber>1</PartNumber><ETag>{etag1}</ETag></Part>\
         <Part><PartNumber>2</PartNumber><ETag>{etag2}</ETag></Part>\
         </CompleteMultipartUpload>"
    );
    let resp = harness
        .aws_post(
            &format!("/mp-int-bucket/assembled.bin?uploadId={upload_id}"),
            "s3",
            "us-east-1",
        )
        .header("content-type", "application/xml")
        .body(complete_xml)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        200,
        "CompleteMultipartUpload failed"
    );
    let body = resp.text().await.unwrap();
    assert!(body.contains("CompleteMultipartUploadResult"));

    // GetObject — should be part1 ++ part2
    let resp = harness
        .aws_get("/mp-int-bucket/assembled.bin", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let got = resp.bytes().await.unwrap();
    let mut expected = part1;
    expected.extend_from_slice(&part2);
    assert_eq!(got.len(), expected.len());
    assert_eq!(&got[..], &expected[..]);

    // Cleanup
    harness
        .aws_delete("/mp-int-bucket/assembled.bin", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    harness
        .aws_delete("/mp-int-bucket", "s3", "us-east-1")
        .send()
        .await
        .unwrap();

    harness.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// S3 Persistence Round-Trip
// ─────────────────────────────────────────────────────────────────────────────

/// Verify that S3 objects survive a save/load persistence cycle.
/// Creates objects, saves state, creates a fresh server from the same data dir,
/// loads state, and verifies objects are still accessible and streamed correctly.
#[tokio::test]
async fn smoke_s3_persistence_round_trip() {
    use std::net::SocketAddr;
    use std::time::Duration;

    use openstack_config::{
        Config, CorsConfig, Directories, LogLevel, ServicesConfig, SnapshotLoadStrategy,
        SnapshotSaveStrategy,
    };
    use openstack_gateway::Gateway;
    use openstack_s3::S3Provider;
    use openstack_service_framework::ServicePluginManager;
    use openstack_state::StateManager;

    // Helper to build a persistence-enabled config
    fn persist_config(addr: SocketAddr, data_dir: &std::path::Path) -> Config {
        Config {
            gateway_listen: vec![addr],
            persistence: true,
            services: ServicesConfig::only(std::iter::once("s3".to_string())),
            debug: false,
            log_level: LogLevel::Warn,
            localstack_host: format!("{}:{}", addr.ip(), addr.port()),
            allow_nonstandard_regions: false,
            cors: CorsConfig {
                disable_cors_headers: false,
                disable_cors_checks: false,
                extra_allowed_origins: vec![],
                extra_allowed_headers: vec![],
            },
            snapshot_save_strategy: SnapshotSaveStrategy::OnShutdown,
            snapshot_load_strategy: SnapshotLoadStrategy::OnStartup,
            snapshot_flush_interval: Duration::from_secs(3600),
            dns_address: None,
            dns_port: 53,
            dns_resolve_ip: "127.0.0.1".to_string(),
            lambda_keepalive_ms: 0,
            lambda_remove_containers: true,
            bucket_marker_local: None,
            eager_service_loading: false,
            enable_config_updates: false,
            directories: Directories::from_root(data_dir),
            body_spool_threshold_bytes: 1_048_576,
        }
    }

    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let data_dir = temp_dir.path().to_owned();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    // ── Phase 1: start server, create objects, save state ──

    let listener1 = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr1: SocketAddr = listener1.local_addr().unwrap();
    let base_url1 = format!("http://{}", addr1);
    let config1 = persist_config(addr1, &data_dir);

    let pm1 = ServicePluginManager::new(config1.clone());
    let s3_provider = S3Provider::new(&config1.directories.s3_objects);
    let persistable = s3_provider.persistable_store();
    pm1.register("s3", s3_provider);

    let sm1 = StateManager::new(config1.clone());
    sm1.register_store(persistable).await;
    let _ = sm1.load_on_startup().await;

    let (tx1, rx1) = tokio::sync::oneshot::channel::<()>();
    let gw1 = Gateway::new(config1.clone(), pm1.clone());
    tokio::spawn(async move { gw1.run_with_listener(listener1, rx1).await.ok() });

    // Wait for ready
    let health1 = format!("{base_url1}/_localstack/health");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if std::time::Instant::now() > deadline {
            panic!("server1 not ready");
        }
        if let Ok(r) = reqwest::get(&health1).await {
            if r.status().is_success() {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let auth =
        "AWS4-HMAC-SHA256 Credential=test/20260306/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    // Create bucket + object
    client
        .put(format!("{base_url1}/persist-bucket"))
        .header("authorization", auth)
        .header("x-amz-date", "20260306T000000Z")
        .send()
        .await
        .unwrap();

    let object_data = b"persistence-test-payload-12345";
    client
        .put(format!("{base_url1}/persist-bucket/key.txt"))
        .header("authorization", auth)
        .header("x-amz-date", "20260306T000000Z")
        .header("content-type", "text/plain")
        .body(&object_data[..])
        .send()
        .await
        .unwrap();

    // Verify it's there
    let resp = client
        .get(format!("{base_url1}/persist-bucket/key.txt"))
        .header("authorization", auth)
        .header("x-amz-date", "20260306T000000Z")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(&resp.bytes().await.unwrap()[..], &object_data[..]);

    // Save state and shut down
    sm1.save_on_shutdown().await.expect("save failed");
    let _ = tx1.send(());
    tokio::time::sleep(Duration::from_millis(100)).await;

    // ── Phase 2: start fresh server from same data dir, load state ──

    let listener2 = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr2: SocketAddr = listener2.local_addr().unwrap();
    let base_url2 = format!("http://{}", addr2);
    let config2 = persist_config(addr2, &data_dir);

    let pm2 = ServicePluginManager::new(config2.clone());
    let s3_provider2 = S3Provider::new(&config2.directories.s3_objects);
    let persistable2 = s3_provider2.persistable_store();
    pm2.register("s3", s3_provider2);

    let sm2 = StateManager::new(config2.clone());
    sm2.register_store(persistable2).await;
    sm2.load_on_startup().await.expect("load failed");

    let (tx2, rx2) = tokio::sync::oneshot::channel::<()>();
    let gw2 = Gateway::new(config2.clone(), pm2.clone());
    tokio::spawn(async move { gw2.run_with_listener(listener2, rx2).await.ok() });

    // Wait for ready
    let health2 = format!("{base_url2}/_localstack/health");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if std::time::Instant::now() > deadline {
            panic!("server2 not ready");
        }
        if let Ok(r) = reqwest::get(&health2).await {
            if r.status().is_success() {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let auth2 =
        "AWS4-HMAC-SHA256 Credential=test/20260306/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    // GetObject from restored server — should return the persisted object
    let resp = client
        .get(format!("{base_url2}/persist-bucket/key.txt"))
        .header("authorization", auth2)
        .header("x-amz-date", "20260306T000000Z")
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        200,
        "GetObject after persistence restore should succeed"
    );
    let got = resp.bytes().await.unwrap();
    assert_eq!(
        &got[..],
        &object_data[..],
        "Object content should survive persistence round-trip"
    );

    // ListBuckets should include persist-bucket
    let resp = client
        .get(format!("{base_url2}/"))
        .header("authorization", auth2)
        .header("x-amz-date", "20260306T000000Z")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let list_body = resp.text().await.unwrap();
    assert!(
        list_body.contains("persist-bucket"),
        "Bucket should survive persistence round-trip"
    );

    let _ = tx2.send(());
}

/// Test CopyObject between buckets through the HTTP gateway with filesystem storage.
#[tokio::test]
async fn smoke_s3_copy_object_cross_bucket() {
    let harness = openstack_integration_tests::harness::TestHarness::start_services("s3").await;

    // Create source and destination buckets
    harness
        .aws_put("/copy-src-bucket", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    harness
        .aws_put("/copy-dst-bucket", "s3", "us-east-1")
        .send()
        .await
        .unwrap();

    // Put an object in the source bucket
    let original = b"cross-bucket copy content here";
    let resp = harness
        .aws_put("/copy-src-bucket/origin.txt", "s3", "us-east-1")
        .header("content-type", "text/plain")
        .body(&original[..])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "PutObject failed");

    // CopyObject to destination bucket
    let resp = harness
        .aws_put("/copy-dst-bucket/copied.txt", "s3", "us-east-1")
        .header("x-amz-copy-source", "/copy-src-bucket/origin.txt")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "CopyObject failed");
    let body = resp.text().await.unwrap();
    assert!(body.contains("CopyObjectResult"));

    // GetObject from destination — should match original
    let resp = harness
        .aws_get("/copy-dst-bucket/copied.txt", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let got = resp.bytes().await.unwrap();
    assert_eq!(&got[..], &original[..]);

    // Delete source object and verify copy still works (independent file)
    harness
        .aws_delete("/copy-src-bucket/origin.txt", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    let resp = harness
        .aws_get("/copy-dst-bucket/copied.txt", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        200,
        "Copy should survive source deletion"
    );
    let got = resp.bytes().await.unwrap();
    assert_eq!(&got[..], &original[..]);

    // Cleanup
    harness
        .aws_delete("/copy-dst-bucket/copied.txt", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    harness
        .aws_delete("/copy-src-bucket", "s3", "us-east-1")
        .send()
        .await
        .unwrap();
    harness
        .aws_delete("/copy-dst-bucket", "s3", "us-east-1")
        .send()
        .await
        .unwrap();

    harness.shutdown();
}
