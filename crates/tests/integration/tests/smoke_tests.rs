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
