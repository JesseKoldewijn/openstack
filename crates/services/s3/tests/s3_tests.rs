use std::collections::HashMap;
use std::sync::Mutex;

use bytes::Bytes;
use digest::Digest as _;
use openstack_s3::{
    provider::S3Provider,
    store::{ObjectDataRef, S3Store},
};
use openstack_service_framework::{
    SpooledBody,
    traits::{RequestContext, ServiceProvider},
};

fn make_ctx(method: &str, path: &str, body: &[u8]) -> RequestContext {
    RequestContext {
        service: "s3".to_string(),
        operation: String::new(), // derived by provider
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::Value::Null,
        raw_body: Some(Bytes::from(body.to_vec())),
        headers: Default::default(),
        path: path.to_string(),
        method: method.to_string(),
        query_params: HashMap::new(),
        request_id: String::new(),
        spooled_body: None,
    }
}

fn make_ctx_with_headers(
    method: &str,
    path: &str,
    body: &[u8],
    headers: http::HeaderMap,
) -> RequestContext {
    RequestContext {
        service: "s3".to_string(),
        operation: String::new(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::Value::Null,
        raw_body: Some(Bytes::from(body.to_vec())),
        headers,
        path: path.to_string(),
        method: method.to_string(),
        query_params: HashMap::new(),
        request_id: String::new(),
        spooled_body: None,
    }
}

fn make_ctx_with_query(
    method: &str,
    path: &str,
    body: &[u8],
    query_params: HashMap<String, String>,
) -> RequestContext {
    RequestContext {
        service: "s3".to_string(),
        operation: String::new(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::Value::Null,
        raw_body: Some(Bytes::from(body.to_vec())),
        headers: Default::default(),
        path: path.to_string(),
        method: method.to_string(),
        query_params,
        request_id: String::new(),
        spooled_body: None,
    }
}

// ---------------------------------------------------------------------------
// Bucket operations
// ---------------------------------------------------------------------------

async fn new_provider() -> S3Provider {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.keep();
    let provider = S3Provider::new(path);
    provider.start().await.unwrap();
    provider
}

#[tokio::test]
async fn test_create_and_list_buckets() {
    let provider = new_provider().await;

    // Initially empty
    let ctx = make_ctx("GET", "/", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("ListAllMyBucketsResult"));
    assert!(!body.contains("my-bucket"));

    // Create bucket
    let ctx = make_ctx("PUT", "/my-bucket", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);

    // List again
    let ctx = make_ctx("GET", "/", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("my-bucket"));
}

#[tokio::test]
async fn test_create_bucket_already_exists() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/test-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    // Second create same bucket
    let ctx = make_ctx("PUT", "/test-bucket", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 409);
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("BucketAlreadyOwnedByYou"));
}

#[tokio::test]
async fn test_delete_bucket() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/del-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let ctx = make_ctx("DELETE", "/del-bucket", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 204);

    // Bucket gone
    let ctx = make_ctx("HEAD", "/del-bucket", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 404);
}

#[tokio::test]
async fn test_delete_non_empty_bucket_fails() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/ne-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        http::header::HeaderValue::from_static("text/plain"),
    );
    let ctx = make_ctx_with_headers("PUT", "/ne-bucket/obj.txt", b"data", headers);
    provider.dispatch(&ctx).await.unwrap();

    let ctx = make_ctx("DELETE", "/ne-bucket", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 409);
}

#[tokio::test]
async fn test_head_bucket() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/hb-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let ctx = make_ctx("HEAD", "/hb-bucket", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn test_get_bucket_location() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/loc-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let mut qp = HashMap::new();
    qp.insert("location".to_string(), String::new());
    let ctx = make_ctx_with_query("GET", "/loc-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("LocationConstraint"));
}

#[tokio::test]
async fn test_get_bucket_location_missing_bucket_matches_localstack_shape() {
    let provider = new_provider().await;
    let mut qp = HashMap::new();
    qp.insert("location".to_string(), String::new());
    let ctx = make_ctx_with_query("GET", "/missing-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 404);
    assert_eq!(resp.content_type, "application/xml");
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("<Code>NoSuchBucket</Code>"));
    assert!(body.contains("<Message>The specified bucket does not exist</Message>"));
    assert!(body.contains("<BucketName>missing-bucket</BucketName>"));
}

// ---------------------------------------------------------------------------
// Object operations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_put_and_get_object() {
    let provider = new_provider().await;

    // Create bucket
    let ctx = make_ctx("PUT", "/obj-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    // Put object
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        http::header::HeaderValue::from_static("text/plain"),
    );
    let ctx = make_ctx_with_headers("PUT", "/obj-bucket/hello.txt", b"hello world", headers);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(resp.headers.iter().any(|(k, _)| k == "ETag"));

    // Get object
    let ctx = make_ctx("GET", "/obj-bucket/hello.txt", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body_bytes = resp.body.into_bytes().await.unwrap();
    assert_eq!(&body_bytes[..], b"hello world");
    assert_eq!(resp.content_type, "text/plain");
}

#[tokio::test]
async fn test_get_nonexistent_object() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/miss-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let ctx = make_ctx("GET", "/miss-bucket/no-such-key", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 404);
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("NoSuchKey"));
}

#[tokio::test]
async fn test_head_object() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/ho-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        http::header::HeaderValue::from_static("application/json"),
    );
    let ctx = make_ctx_with_headers("PUT", "/ho-bucket/data.json", b"{}", headers);
    provider.dispatch(&ctx).await.unwrap();

    let ctx = make_ctx("HEAD", "/ho-bucket/data.json", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(resp.body.as_bytes().is_empty()); // HEAD returns no body
    assert_eq!(resp.content_type, "application/json");
}

#[tokio::test]
async fn test_delete_object() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/do-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let ctx = make_ctx("PUT", "/do-bucket/key.txt", b"data");
    provider.dispatch(&ctx).await.unwrap();

    let ctx = make_ctx("DELETE", "/do-bucket/key.txt", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 204);

    // Confirm gone
    let ctx = make_ctx("GET", "/do-bucket/key.txt", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 404);
}

#[tokio::test]
async fn test_delete_objects_bulk() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/bulk-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    for key in &["a.txt", "b.txt", "c.txt"] {
        let path = format!("/bulk-bucket/{key}");
        let ctx = make_ctx("PUT", &path, b"data");
        provider.dispatch(&ctx).await.unwrap();
    }

    let body = b"<?xml version=\"1.0\"?><Delete><Object><Key>a.txt</Key></Object><Object><Key>b.txt</Key></Object></Delete>";
    let mut qp = HashMap::new();
    qp.insert("delete".to_string(), String::new());
    let ctx = make_ctx_with_query("POST", "/bulk-bucket", body, qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("a.txt"));
    assert!(body.contains("b.txt"));

    // c.txt still there
    let ctx = make_ctx("GET", "/bulk-bucket/c.txt", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn test_copy_object() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/src-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();
    let ctx = make_ctx("PUT", "/dst-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        http::header::HeaderValue::from_static("text/plain"),
    );
    let ctx = make_ctx_with_headers(
        "PUT",
        "/src-bucket/original.txt",
        b"original content",
        headers,
    );
    provider.dispatch(&ctx).await.unwrap();

    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::HeaderName::from_static("x-amz-copy-source"),
        http::header::HeaderValue::from_static("/src-bucket/original.txt"),
    );
    let ctx = make_ctx_with_headers("PUT", "/dst-bucket/copy.txt", b"", headers);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("CopyObjectResult"));

    let ctx = make_ctx("GET", "/dst-bucket/copy.txt", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    let body_bytes = resp.body.into_bytes().await.unwrap();
    assert_eq!(&body_bytes[..], b"original content");
}

#[tokio::test]
async fn test_non_versioned_large_copy_overwrite_keeps_data() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/src-large-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();
    let ctx = make_ctx("PUT", "/dst-large-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let src_a: Vec<u8> = (0u8..=250).cycle().take(300 * 1024).collect();
    let src_b: Vec<u8> = (0..(300 * 1024)).map(|i| ((i * 7) % 251) as u8).collect();

    let ctx = make_ctx_spooled("PUT", "/src-large-bucket/a.bin", src_a.clone(), 0);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);

    let ctx = make_ctx_spooled("PUT", "/src-large-bucket/b.bin", src_b.clone(), 0);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);

    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::HeaderName::from_static("x-amz-copy-source"),
        http::header::HeaderValue::from_static("/src-large-bucket/a.bin"),
    );
    let ctx = make_ctx_with_headers("PUT", "/dst-large-bucket/target.bin", b"", headers);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);

    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::HeaderName::from_static("x-amz-copy-source"),
        http::header::HeaderValue::from_static("/src-large-bucket/b.bin"),
    );
    let ctx = make_ctx_with_headers("PUT", "/dst-large-bucket/target.bin", b"", headers);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);

    let ctx = make_ctx("GET", "/dst-large-bucket/target.bin", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body_bytes = resp.body.into_bytes().await.unwrap();
    assert_eq!(&body_bytes[..], &src_b[..]);
}

// ---------------------------------------------------------------------------
// ListObjectsV2
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_objects_v2_prefix() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/list-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    for key in &["a/1.txt", "a/2.txt", "b/1.txt"] {
        let path = format!("/list-bucket/{key}");
        let ctx = make_ctx("PUT", &path, b"x");
        provider.dispatch(&ctx).await.unwrap();
    }

    let mut qp = HashMap::new();
    qp.insert("list-type".to_string(), "2".to_string());
    qp.insert("prefix".to_string(), "a/".to_string());
    let ctx = make_ctx_with_query("GET", "/list-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("a/1.txt"));
    assert!(body.contains("a/2.txt"));
    assert!(!body.contains("b/1.txt"));
}

#[tokio::test]
async fn test_list_objects_v2_delimiter() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/delim-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    for key in &["a/1.txt", "a/2.txt", "b/1.txt", "root.txt"] {
        let path = format!("/delim-bucket/{key}");
        let ctx = make_ctx("PUT", &path, b"x");
        provider.dispatch(&ctx).await.unwrap();
    }

    let mut qp = HashMap::new();
    qp.insert("list-type".to_string(), "2".to_string());
    qp.insert("delimiter".to_string(), "/".to_string());
    let ctx = make_ctx_with_query("GET", "/delim-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("CommonPrefixes"));
    assert!(body.contains("root.txt")); // top-level object in Contents
}

#[tokio::test]
async fn test_list_objects_v2_max_keys() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/maxk-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    for i in 0..5 {
        let path = format!("/maxk-bucket/key-{i:02}.txt");
        let ctx = make_ctx("PUT", &path, b"x");
        provider.dispatch(&ctx).await.unwrap();
    }

    let mut qp = HashMap::new();
    qp.insert("list-type".to_string(), "2".to_string());
    qp.insert("max-keys".to_string(), "2".to_string());
    let ctx = make_ctx_with_query("GET", "/maxk-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("<IsTruncated>true</IsTruncated>"));
    assert!(body.contains("NextContinuationToken"));
}

// ---------------------------------------------------------------------------
// Multipart upload
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_multipart_upload() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/mp-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    // Create multipart upload
    let mut qp = HashMap::new();
    qp.insert("uploads".to_string(), String::new());
    let ctx = make_ctx_with_query("POST", "/mp-bucket/large.bin", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("InitiateMultipartUploadResult"));

    // Extract upload_id
    let upload_id = {
        let start = body.find("<UploadId>").unwrap() + 10;
        let end = body.find("</UploadId>").unwrap();
        body[start..end].to_string()
    };

    // Upload parts
    let mut qp1 = HashMap::new();
    qp1.insert("uploadId".to_string(), upload_id.clone());
    qp1.insert("partNumber".to_string(), "1".to_string());
    let ctx = make_ctx_with_query("PUT", "/mp-bucket/large.bin", b"part-one-data", qp1);
    let resp1 = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp1.status_code, 200);
    let etag1 = resp1
        .headers
        .iter()
        .find(|(k, _)| k == "ETag")
        .map(|(_, v)| v.clone())
        .unwrap();

    let mut qp2 = HashMap::new();
    qp2.insert("uploadId".to_string(), upload_id.clone());
    qp2.insert("partNumber".to_string(), "2".to_string());
    let ctx = make_ctx_with_query("PUT", "/mp-bucket/large.bin", b"-part-two-data", qp2);
    let resp2 = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp2.status_code, 200);
    let etag2 = resp2
        .headers
        .iter()
        .find(|(k, _)| k == "ETag")
        .map(|(_, v)| v.clone())
        .unwrap();

    // Complete upload
    let complete_body = format!(
        "<CompleteMultipartUpload>\
<Part><PartNumber>1</PartNumber><ETag>{etag1}</ETag></Part>\
<Part><PartNumber>2</PartNumber><ETag>{etag2}</ETag></Part>\
</CompleteMultipartUpload>"
    );
    let mut qp3 = HashMap::new();
    qp3.insert("uploadId".to_string(), upload_id.clone());
    let ctx = make_ctx_with_query(
        "POST",
        "/mp-bucket/large.bin",
        complete_body.as_bytes(),
        qp3,
    );
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("CompleteMultipartUploadResult"));
    assert!(
        body.contains("<ETag>&quot;c1863f721cc0c27dc4f7316053f28451-2&quot;</ETag>"),
        "multipart ETag should include part-count suffix"
    );

    // Get the assembled object
    let ctx = make_ctx("GET", "/mp-bucket/large.bin", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body_bytes = resp.body.into_bytes().await.unwrap();
    assert_eq!(&body_bytes[..], b"part-one-data-part-two-data");
}

#[tokio::test]
async fn test_non_versioned_large_multipart_overwrite_keeps_data() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/mp-overwrite-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    async fn run_large_multipart_upload(
        provider: &S3Provider,
        bucket: &str,
        key: &str,
        part1: &[u8],
        part2: &[u8],
    ) {
        let mut qp = HashMap::new();
        qp.insert("uploads".to_string(), String::new());
        let ctx = make_ctx_with_query("POST", &format!("/{bucket}/{key}"), b"", qp);
        let resp = provider.dispatch(&ctx).await.unwrap();
        assert_eq!(resp.status_code, 200);
        let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
        let start = body.find("<UploadId>").unwrap() + 10;
        let end = body.find("</UploadId>").unwrap();
        let upload_id = body[start..end].to_string();

        let mut qp1 = HashMap::new();
        qp1.insert("uploadId".to_string(), upload_id.clone());
        qp1.insert("partNumber".to_string(), "1".to_string());
        let ctx = make_ctx_with_query("PUT", &format!("/{bucket}/{key}"), part1, qp1);
        let resp1 = provider.dispatch(&ctx).await.unwrap();
        assert_eq!(resp1.status_code, 200);
        let etag1 = resp1
            .headers
            .iter()
            .find(|(k, _)| k == "ETag")
            .map(|(_, v)| v.clone())
            .unwrap();

        let mut qp2 = HashMap::new();
        qp2.insert("uploadId".to_string(), upload_id.clone());
        qp2.insert("partNumber".to_string(), "2".to_string());
        let ctx = make_ctx_with_query("PUT", &format!("/{bucket}/{key}"), part2, qp2);
        let resp2 = provider.dispatch(&ctx).await.unwrap();
        assert_eq!(resp2.status_code, 200);
        let etag2 = resp2
            .headers
            .iter()
            .find(|(k, _)| k == "ETag")
            .map(|(_, v)| v.clone())
            .unwrap();

        let complete_body = format!(
            "<CompleteMultipartUpload>\
<Part><PartNumber>1</PartNumber><ETag>{etag1}</ETag></Part>\
<Part><PartNumber>2</PartNumber><ETag>{etag2}</ETag></Part>\
</CompleteMultipartUpload>"
        );
        let mut qp3 = HashMap::new();
        qp3.insert("uploadId".to_string(), upload_id);
        let ctx = make_ctx_with_query(
            "POST",
            &format!("/{bucket}/{key}"),
            complete_body.as_bytes(),
            qp3,
        );
        let resp = provider.dispatch(&ctx).await.unwrap();
        assert_eq!(resp.status_code, 200);
        let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
        assert!(body.contains("-2&quot;</ETag>"));
    }

    let first_p1: Vec<u8> = (0u8..=255).cycle().take(180 * 1024).collect();
    let first_p2: Vec<u8> = (0u8..=200).cycle().take(140 * 1024).collect();
    run_large_multipart_upload(
        &provider,
        "mp-overwrite-bucket",
        "large.bin",
        &first_p1,
        &first_p2,
    )
    .await;

    let second_p1: Vec<u8> = (0..(170 * 1024)).map(|i| ((i * 11) % 251) as u8).collect();
    let second_p2: Vec<u8> = (0..(150 * 1024)).map(|i| ((i * 13) % 247) as u8).collect();
    run_large_multipart_upload(
        &provider,
        "mp-overwrite-bucket",
        "large.bin",
        &second_p1,
        &second_p2,
    )
    .await;

    let ctx = make_ctx("GET", "/mp-overwrite-bucket/large.bin", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body_bytes = resp.body.into_bytes().await.unwrap();
    let mut expected = Vec::with_capacity(second_p1.len() + second_p2.len());
    expected.extend_from_slice(&second_p1);
    expected.extend_from_slice(&second_p2);
    assert_eq!(&body_bytes[..], &expected[..]);
}

#[tokio::test]
async fn test_abort_multipart_upload() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/abort-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let mut qp = HashMap::new();
    qp.insert("uploads".to_string(), String::new());
    let ctx = make_ctx_with_query("POST", "/abort-bucket/file.bin", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    let start = body.find("<UploadId>").unwrap() + 10;
    let end = body.find("</UploadId>").unwrap();
    let upload_id = body[start..end].to_string();

    let mut qp = HashMap::new();
    qp.insert("uploadId".to_string(), upload_id);
    let ctx = make_ctx_with_query("DELETE", "/abort-bucket/file.bin", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 204);
}

// ---------------------------------------------------------------------------
// Versioning
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_versioning() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/ver-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    // Enable versioning
    let mut qp = HashMap::new();
    qp.insert("versioning".to_string(), String::new());
    let ctx = make_ctx_with_query(
        "PUT",
        "/ver-bucket",
        b"<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>",
        qp,
    );
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);

    // Put object twice
    let ctx = make_ctx("PUT", "/ver-bucket/key.txt", b"version-1");
    provider.dispatch(&ctx).await.unwrap();
    let ctx = make_ctx("PUT", "/ver-bucket/key.txt", b"version-2");
    provider.dispatch(&ctx).await.unwrap();

    // Current version is v2
    let ctx = make_ctx("GET", "/ver-bucket/key.txt", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    let body_bytes = resp.body.into_bytes().await.unwrap();
    assert_eq!(&body_bytes[..], b"version-2");

    // List versions — should have 2 entries
    let mut qp = HashMap::new();
    qp.insert("versions".to_string(), String::new());
    let ctx = make_ctx_with_query("GET", "/ver-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("ListVersionsResult"));
    // Should contain two <Version> entries
    assert_eq!(body.matches("<Version>").count(), 2);
}

// ---------------------------------------------------------------------------
// Bucket policy
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_bucket_policy() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/pol-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    // No policy initially
    let mut qp = HashMap::new();
    qp.insert("policy".to_string(), String::new());
    let ctx = make_ctx_with_query("GET", "/pol-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 404);

    // Put policy
    let policy = r#"{"Version":"2012-10-17","Statement":[]}"#;
    let mut qp = HashMap::new();
    qp.insert("policy".to_string(), String::new());
    let ctx = make_ctx_with_query("PUT", "/pol-bucket", policy.as_bytes(), qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 204);

    // Get policy
    let mut qp = HashMap::new();
    qp.insert("policy".to_string(), String::new());
    let ctx = make_ctx_with_query("GET", "/pol-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(std::str::from_utf8(resp.body.as_bytes()).unwrap(), policy);

    // Delete policy
    let mut qp = HashMap::new();
    qp.insert("policy".to_string(), String::new());
    let ctx = make_ctx_with_query("DELETE", "/pol-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 204);

    // Gone again
    let mut qp = HashMap::new();
    qp.insert("policy".to_string(), String::new());
    let ctx = make_ctx_with_query("GET", "/pol-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 404);
}

// ---------------------------------------------------------------------------
// ACLs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_acl_bucket() {
    let provider = new_provider().await;
    let ctx = make_ctx("PUT", "/acl-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let mut qp = HashMap::new();
    qp.insert("acl".to_string(), String::new());
    let ctx = make_ctx_with_query("GET", "/acl-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    assert!(body.contains("AccessControlPolicy"));
}

// ---------------------------------------------------------------------------
// S3Store unit tests
// ---------------------------------------------------------------------------

#[test]
fn test_store_put_get() {
    let mut store = S3Store::new();
    store.create_bucket("bucket", "us-east-1");
    store.put_object(
        "bucket",
        "key",
        ObjectDataRef::Inline(Bytes::from_static(b"hello")),
        "text/plain",
        HashMap::new(),
    );

    let v = store.get_object("bucket", "key").unwrap();
    assert_eq!(v.data, ObjectDataRef::Inline(Bytes::from_static(b"hello")));
    assert!(!v.etag.is_empty());
}

#[test]
fn test_store_delete_object() {
    let mut store = S3Store::new();
    store.create_bucket("bucket", "us-east-1");
    store.put_object(
        "bucket",
        "key",
        ObjectDataRef::Inline(Bytes::from_static(b"data")),
        "text/plain",
        HashMap::new(),
    );
    store.delete_object("bucket", "key");
    assert!(store.get_object("bucket", "key").is_none());
    assert!(store.is_bucket_empty("bucket"));
}

#[test]
fn test_store_versioning() {
    let mut store = S3Store::new();
    store.create_bucket("bucket", "us-east-1");
    store.buckets.get_mut("bucket").unwrap().versioning = "Enabled".to_string();

    store.put_object(
        "bucket",
        "k",
        ObjectDataRef::Inline(Bytes::from_static(b"v1")),
        "text/plain",
        HashMap::new(),
    );
    store.put_object(
        "bucket",
        "k",
        ObjectDataRef::Inline(Bytes::from_static(b"v2")),
        "text/plain",
        HashMap::new(),
    );

    let current = store.get_object("bucket", "k").unwrap();
    assert_eq!(
        current.data,
        ObjectDataRef::Inline(Bytes::from_static(b"v2"))
    );

    let objs = store.list_objects("bucket");
    let obj = objs.into_iter().find(|o| o.key == "k").unwrap();
    assert_eq!(obj.versions.len(), 2);
}

#[test]
fn test_store_non_versioned_overwrite_keeps_single_version() {
    let mut store = S3Store::new();
    store.create_bucket("bucket", "us-east-1");

    store.put_object(
        "bucket",
        "k",
        ObjectDataRef::Inline(Bytes::from_static(b"v1")),
        "text/plain",
        HashMap::new(),
    );
    store.put_object(
        "bucket",
        "k",
        ObjectDataRef::Inline(Bytes::from_static(b"v2")),
        "text/plain",
        HashMap::new(),
    );

    let objs = store.list_objects("bucket");
    let obj = objs.into_iter().find(|o| o.key == "k").unwrap();
    assert_eq!(obj.versions.len(), 1);
    assert_eq!(obj.versions[0].version_id, "null");
    assert_eq!(
        obj.versions[0].data,
        ObjectDataRef::Inline(Bytes::from_static(b"v2"))
    );
}

#[test]
fn test_store_multipart() {
    let mut store = S3Store::new();
    store.create_bucket("bucket", "us-east-1");
    let uid =
        store.create_multipart_upload("bucket", "key", "application/octet-stream", HashMap::new());
    store.upload_part(&uid, 1, ObjectDataRef::Inline(Bytes::from_static(b"part1")));
    store.upload_part(&uid, 2, ObjectDataRef::Inline(Bytes::from_static(b"part2")));
    let v = store
        .complete_multipart_upload(&uid, &[(1, String::new()), (2, String::new())])
        .unwrap();
    assert_eq!(
        v.data,
        ObjectDataRef::Inline(Bytes::from(b"part1part2".as_ref()))
    );
}

// ---------------------------------------------------------------------------
// Phase 4 — Streaming PutObject via spooled_body (tasks 4.5 and 4.6)
// ---------------------------------------------------------------------------

/// Helper: build a RequestContext with raw_body=None and spooled_body=Some.
/// This mimics the real gateway path for S3 PutObject where the body is
/// streamed through SpooledBody and never materialised into raw_body.
fn make_ctx_spooled(method: &str, path: &str, data: Vec<u8>, threshold: usize) -> RequestContext {
    let spooled = SpooledBody::from_bytes(Bytes::from(data), threshold)
        .expect("SpooledBody::from_bytes failed");
    RequestContext {
        service: "s3".to_string(),
        operation: String::new(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::Value::Null,
        raw_body: None, // gateway does NOT materialise for S3 object bodies
        headers: Default::default(),
        path: path.to_string(),
        method: method.to_string(),
        query_params: HashMap::new(),
        request_id: String::new(),
        spooled_body: Some(Mutex::new(spooled)),
    }
}

/// Task 4.5 — PutObject with spooled_body=Some and raw_body=None: object is
/// written, ETag is correct, and raw_body is never needed.
#[tokio::test]
async fn test_put_object_via_spooled_body() {
    let provider = new_provider().await;

    // Create bucket
    let ctx = make_ctx("PUT", "/spool-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    // Put object using the spooled path (raw_body=None)
    let body_data = b"spooled content for streaming put".to_vec();
    let expected_etag = format!("\"{}\"", hex::encode(md5::Md5::digest(&body_data)));

    let ctx = make_ctx_spooled(
        "PUT",
        "/spool-bucket/streamed.txt",
        body_data.clone(),
        1024 * 1024,
    );
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200, "PutObject via spooled_body failed");

    // Verify ETag header matches expected MD5
    let etag_header = resp
        .headers
        .iter()
        .find(|(k, _)| k == "ETag")
        .map(|(_, v)| v.clone())
        .expect("ETag header missing");
    assert_eq!(etag_header, expected_etag, "ETag mismatch");

    // GetObject — verify content round-trips correctly
    let ctx = make_ctx("GET", "/spool-bucket/streamed.txt", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let got = resp.body.into_bytes().await.unwrap();
    assert_eq!(&got[..], &body_data[..]);
}

/// Task 4.6 — Parameterized test: small (inline, 100 B) and large (disk-spilled,
/// 300 KiB, above 256 KiB threshold) bodies both produce correct ETags and content.
#[tokio::test]
async fn test_put_object_spooled_inline_and_disk() {
    let provider = new_provider().await;

    // Create bucket
    let ctx = make_ctx("PUT", "/threshold-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    // Inline case: 100 bytes — well below the 256 KiB threshold
    let inline_data: Vec<u8> = (0u8..100).collect();
    let inline_etag = format!("\"{}\"", hex::encode(md5::Md5::digest(&inline_data)));

    // Use a threshold high enough that the SpooledBody stays in memory
    let ctx = make_ctx_spooled(
        "PUT",
        "/threshold-bucket/inline.bin",
        inline_data.clone(),
        1024 * 1024,
    );
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200, "inline PutObject failed");
    let etag = resp
        .headers
        .iter()
        .find(|(k, _)| k == "ETag")
        .map(|(_, v)| v.clone())
        .unwrap();
    assert_eq!(etag, inline_etag, "inline ETag mismatch");

    let ctx = make_ctx("GET", "/threshold-bucket/inline.bin", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    let got = resp.body.into_bytes().await.unwrap();
    assert_eq!(&got[..], &inline_data[..]);

    // Disk-spilled case: 300 KiB — above the 256 KiB provider threshold
    let large_data: Vec<u8> = (0u8..255).cycle().take(300 * 1024).collect();
    let large_etag = format!("\"{}\"", hex::encode(md5::Md5::digest(&large_data)));

    // Use threshold=0 so SpooledBody spills immediately to disk
    let ctx = make_ctx_spooled("PUT", "/threshold-bucket/large.bin", large_data.clone(), 0);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200, "disk-spilled PutObject failed");
    let etag = resp
        .headers
        .iter()
        .find(|(k, _)| k == "ETag")
        .map(|(_, v)| v.clone())
        .unwrap();
    assert_eq!(etag, large_etag, "disk-spilled ETag mismatch");

    let ctx = make_ctx("GET", "/threshold-bucket/large.bin", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    let got = resp.body.into_bytes().await.unwrap();
    assert_eq!(&got[..], &large_data[..]);
}

#[tokio::test]
async fn test_non_versioned_large_put_overwrite_keeps_data() {
    let provider = new_provider().await;

    let ctx = make_ctx("PUT", "/overwrite-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let first_data: Vec<u8> = (0u8..=250).cycle().take(300 * 1024).collect();
    let second_data: Vec<u8> = (0..(300 * 1024)).map(|i| ((i * 5) % 251) as u8).collect();

    let ctx = make_ctx_spooled("PUT", "/overwrite-bucket/blob.bin", first_data, 0);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);

    let ctx = make_ctx_spooled("PUT", "/overwrite-bucket/blob.bin", second_data.clone(), 0);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);

    let ctx = make_ctx("GET", "/overwrite-bucket/blob.bin", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let got = resp.body.into_bytes().await.unwrap();
    assert_eq!(&got[..], &second_data[..]);
}
