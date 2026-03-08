//! Integration tests for all AWS protocol parsers and serializers.
//! Tests use realistic request samples matching what AWS SDKs send.

#[cfg(test)]
mod protocol_integration_tests {
    use std::collections::HashMap;

    use openstack_aws_protocol::{
        ec2::{parse_ec2_request, serialize_ec2_response},
        error::serialize_error,
        json::{parse_json_request, serialize_json_response},
        protocol::AwsProtocol,
        query::{parse_query_request, serialize_query_response},
        rest_json::parse_rest_json_request,
        rest_xml::parse_rest_xml_request,
    };

    // ─── Query Protocol (SQS, SNS, IAM, STS, CloudFormation) ───────────────────

    #[test]
    fn sqs_create_queue_roundtrip() {
        let body = b"Action=CreateQueue&QueueName=my-queue&Version=2012-11-05";
        let (action, params) = parse_query_request(body).expect("parse failed");
        assert_eq!(action, "CreateQueue");
        assert_eq!(params["QueueName"].as_str().unwrap(), "my-queue");

        let xml = serialize_query_response("CreateQueue", &params, "test-request-id");
        assert!(xml.contains("<CreateQueueResponse"));
        assert!(xml.contains("<RequestId>test-request-id</RequestId>"));
    }

    #[test]
    fn sns_publish_roundtrip() {
        let body =
            b"Action=Publish&TopicArn=arn%3Aaws%3Asns%3Aus-east-1%3A000000000000%3Atest&Message=hello&Version=2010-03-31";
        let (action, params) = parse_query_request(body).expect("parse failed");
        assert_eq!(action, "Publish");
        assert!(params["TopicArn"].as_str().unwrap().contains("sns"));
        assert_eq!(params["Message"].as_str().unwrap(), "hello");
    }

    #[test]
    fn iam_create_role_roundtrip() {
        let body =
            b"Action=CreateRole&RoleName=MyRole&AssumeRolePolicyDocument=%7B%7D&Version=2010-05-08";
        let (action, params) = parse_query_request(body).expect("parse failed");
        assert_eq!(action, "CreateRole");
        assert_eq!(params["RoleName"].as_str().unwrap(), "MyRole");
    }

    #[test]
    fn sts_get_caller_identity_roundtrip() {
        let body = b"Action=GetCallerIdentity&Version=2011-06-15";
        let (action, params) = parse_query_request(body).expect("parse failed");
        assert_eq!(action, "GetCallerIdentity");
        let xml = serialize_query_response("GetCallerIdentity", &params, "req-123");
        assert!(xml.contains("<GetCallerIdentityResponse"));
    }

    // ─── JSON Protocol (DynamoDB, Kinesis, Lambda, KMS, etc.) ──────────────────

    #[test]
    fn dynamodb_get_item_roundtrip() {
        let body = br#"{"TableName":"Users","Key":{"id":{"S":"user-1"}}}"#;
        let (op, params) =
            parse_json_request(body, Some("DynamoDB_20120810.GetItem")).expect("parse failed");
        assert_eq!(op, "GetItem");
        assert_eq!(params["TableName"].as_str().unwrap(), "Users");

        let resp_bytes = serialize_json_response(&params);
        let reparsed: serde_json::Value =
            serde_json::from_slice(&resp_bytes).expect("invalid JSON response");
        assert_eq!(reparsed["TableName"], "Users");
    }

    #[test]
    fn kinesis_put_record_roundtrip() {
        let body = br#"{"StreamName":"my-stream","Data":"SGVsbG8=","PartitionKey":"pk1"}"#;
        let (op, params) =
            parse_json_request(body, Some("Kinesis_20131202.PutRecord")).expect("parse failed");
        assert_eq!(op, "PutRecord");
        assert_eq!(params["StreamName"].as_str().unwrap(), "my-stream");
    }

    #[test]
    fn lambda_invoke_roundtrip() {
        let body = br#"{"key": "value"}"#;
        let (op, params) =
            parse_json_request(body, Some("AWSLambda.Invoke")).expect("parse failed");
        assert_eq!(op, "Invoke");
        assert_eq!(params["key"].as_str().unwrap(), "value");
    }

    #[test]
    fn json_empty_body_is_ok() {
        let (op, params) =
            parse_json_request(b"", Some("DynamoDB_20120810.ListTables")).expect("parse failed");
        assert_eq!(op, "ListTables");
        assert!(params.is_object());
    }

    // ─── REST-JSON Protocol (API Gateway, newer services) ──────────────────────

    #[test]
    fn rest_json_with_body() {
        let body = br#"{"FunctionName":"my-fn","Runtime":"python3.9"}"#;
        let qp = HashMap::new();
        let params = parse_rest_json_request("POST", "/2015-03-31/functions", body, &qp)
            .expect("parse failed");
        assert_eq!(params["FunctionName"].as_str().unwrap(), "my-fn");
    }

    #[test]
    fn rest_json_query_params_merged() {
        let body = b"{}";
        let mut qp = HashMap::new();
        qp.insert("MaxResults".to_string(), "10".to_string());
        let params = parse_rest_json_request("GET", "/2015-03-31/functions", body, &qp)
            .expect("parse failed");
        assert_eq!(params["MaxResults"].as_str().unwrap(), "10");
    }

    // ─── REST-XML Protocol (S3, Route53) ───────────────────────────────────────

    #[test]
    fn rest_xml_put_object() {
        let body = b"<CreateBucketConfiguration><LocationConstraint>eu-west-1</LocationConstraint></CreateBucketConfiguration>";
        let qp = HashMap::new();
        let params = parse_rest_xml_request("PUT", "/my-bucket", body, &qp).expect("parse failed");
        assert_eq!(params["Method"].as_str().unwrap(), "PUT");
        assert_eq!(params["Path"].as_str().unwrap(), "/my-bucket");
        assert!(
            params["__xml_body"]
                .as_str()
                .unwrap()
                .contains("LocationConstraint")
        );
    }

    #[test]
    fn rest_xml_no_body() {
        let qp = HashMap::new();
        let params =
            parse_rest_xml_request("GET", "/my-bucket?location", b"", &qp).expect("parse failed");
        assert_eq!(params["Method"].as_str().unwrap(), "GET");
    }

    // ─── EC2 Protocol ──────────────────────────────────────────────────────────

    #[test]
    fn ec2_describe_instances_roundtrip() {
        let body = b"Action=DescribeInstances&Version=2016-11-15&Filter.1.Name=instance-state-name&Filter.1.Value.1=running";
        let (action, params) = parse_ec2_request(body).expect("parse failed");
        assert_eq!(action, "DescribeInstances");

        let xml = serialize_ec2_response("DescribeInstances", &params, "req-ec2-1");
        assert!(xml.contains("<DescribeInstancesResponse"));
        assert!(xml.contains("http://ec2.amazonaws.com"));
    }

    #[test]
    fn ec2_run_instances_roundtrip() {
        let body =
            b"Action=RunInstances&ImageId=ami-12345&MinCount=1&MaxCount=1&Version=2016-11-15";
        let (action, params) = parse_ec2_request(body).expect("parse failed");
        assert_eq!(action, "RunInstances");
        assert_eq!(params["ImageId"].as_str().unwrap(), "ami-12345");
    }

    // ─── Error Serialization ───────────────────────────────────────────────────

    #[test]
    fn error_query_protocol() {
        let (status, body, ct) = serialize_error(
            &AwsProtocol::Query,
            "QueueDoesNotExist",
            "Queue not found",
            404,
            "req-1",
        );
        assert_eq!(status, 404);
        assert_eq!(ct, "text/xml");
        let xml = std::str::from_utf8(&body).unwrap();
        assert!(xml.contains("<Code>QueueDoesNotExist</Code>"));
        assert!(xml.contains("<RequestId>req-1</RequestId>"));
    }

    #[test]
    fn error_json_protocol() {
        let (status, body, ct) = serialize_error(
            &AwsProtocol::Json,
            "ResourceNotFoundException",
            "Table not found",
            400,
            "req-2",
        );
        assert_eq!(status, 400);
        assert!(ct.contains("json"));
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["__type"].as_str().unwrap(), "ResourceNotFoundException");
    }

    #[test]
    fn error_rest_json_protocol() {
        let (status, body, _ct) = serialize_error(
            &AwsProtocol::RestJson,
            "NotFoundException",
            "Not found",
            404,
            "req-3",
        );
        assert_eq!(status, 404);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["code"].as_str().unwrap(), "NotFoundException");
    }

    #[test]
    fn error_rest_xml_protocol() {
        let (status, body, _ct) = serialize_error(
            &AwsProtocol::RestXml,
            "NoSuchBucket",
            "Bucket not found",
            404,
            "req-4",
        );
        assert_eq!(status, 404);
        let xml = std::str::from_utf8(&body).unwrap();
        assert!(xml.contains("<Code>NoSuchBucket</Code>"));
    }

    // ─── Protocol routing ──────────────────────────────────────────────────────

    #[test]
    fn protocol_routing_by_service() {
        assert_eq!(AwsProtocol::from_service("s3"), AwsProtocol::RestXml);
        assert_eq!(AwsProtocol::from_service("sqs"), AwsProtocol::Query);
        assert_eq!(AwsProtocol::from_service("sns"), AwsProtocol::Query);
        assert_eq!(AwsProtocol::from_service("iam"), AwsProtocol::Query);
        assert_eq!(AwsProtocol::from_service("sts"), AwsProtocol::Query);
        assert_eq!(
            AwsProtocol::from_service("cloudformation"),
            AwsProtocol::Query
        );
        assert_eq!(AwsProtocol::from_service("ec2"), AwsProtocol::Ec2);
        assert_eq!(AwsProtocol::from_service("dynamodb"), AwsProtocol::Json);
        assert_eq!(AwsProtocol::from_service("kinesis"), AwsProtocol::Json);
        assert_eq!(AwsProtocol::from_service("kms"), AwsProtocol::Json);
        // unknown service falls back to rest-json
        assert_eq!(AwsProtocol::from_service("unknown"), AwsProtocol::RestJson);
    }
}
