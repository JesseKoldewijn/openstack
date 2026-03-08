use std::collections::HashMap;

use bytes::Bytes;
use openstack_s3::{provider::S3Provider, store::S3Store};
use openstack_service_framework::traits::{RequestContext, ServiceProvider};

fn make_ctx(method: &str, path: &str, body: &[u8]) -> RequestContext {
    RequestContext {
        service: "s3".to_string(),
        operation: String::new(), // derived by provider
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::Value::Null,
        raw_body: Bytes::from(body.to_vec()),
        headers: HashMap::new(),
        path: path.to_string(),
        method: method.to_string(),
        query_params: HashMap::new(),
    }
}

fn make_ctx_with_headers(
    method: &str,
    path: &str,
    body: &[u8],
    headers: HashMap<String, String>,
) -> RequestContext {
    RequestContext {
        service: "s3".to_string(),
        operation: String::new(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::Value::Null,
        raw_body: Bytes::from(body.to_vec()),
        headers,
        path: path.to_string(),
        method: method.to_string(),
        query_params: HashMap::new(),
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
        raw_body: Bytes::from(body.to_vec()),
        headers: HashMap::new(),
        path: path.to_string(),
        method: method.to_string(),
        query_params,
    }
}

// ---------------------------------------------------------------------------
// Bucket operations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_list_buckets() {
    let provider = S3Provider::new();

    // Initially empty
    let ctx = make_ctx("GET", "/", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(&resp.body).unwrap();
    assert!(body.contains("ListAllMyBucketsResult"));
    assert!(!body.contains("my-bucket"));

    // Create bucket
    let ctx = make_ctx("PUT", "/my-bucket", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);

    // List again
    let ctx = make_ctx("GET", "/", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    let body = std::str::from_utf8(&resp.body).unwrap();
    assert!(body.contains("my-bucket"));
}

#[tokio::test]
async fn test_create_bucket_already_exists() {
    let provider = S3Provider::new();
    let ctx = make_ctx("PUT", "/test-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    // Second create same bucket
    let ctx = make_ctx("PUT", "/test-bucket", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 409);
    let body = std::str::from_utf8(&resp.body).unwrap();
    assert!(body.contains("BucketAlreadyOwnedByYou"));
}

#[tokio::test]
async fn test_delete_bucket() {
    let provider = S3Provider::new();
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
    let provider = S3Provider::new();
    let ctx = make_ctx("PUT", "/ne-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "text/plain".to_string());
    let ctx = make_ctx_with_headers("PUT", "/ne-bucket/obj.txt", b"data", headers);
    provider.dispatch(&ctx).await.unwrap();

    let ctx = make_ctx("DELETE", "/ne-bucket", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 409);
}

#[tokio::test]
async fn test_head_bucket() {
    let provider = S3Provider::new();
    let ctx = make_ctx("PUT", "/hb-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let ctx = make_ctx("HEAD", "/hb-bucket", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn test_get_bucket_location() {
    let provider = S3Provider::new();
    let ctx = make_ctx("PUT", "/loc-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let mut qp = HashMap::new();
    qp.insert("location".to_string(), String::new());
    let ctx = make_ctx_with_query("GET", "/loc-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(&resp.body).unwrap();
    assert!(body.contains("LocationConstraint"));
}

// ---------------------------------------------------------------------------
// Object operations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_put_and_get_object() {
    let provider = S3Provider::new();

    // Create bucket
    let ctx = make_ctx("PUT", "/obj-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    // Put object
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "text/plain".to_string());
    let ctx = make_ctx_with_headers("PUT", "/obj-bucket/hello.txt", b"hello world", headers);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(resp.headers.iter().any(|(k, _)| k == "ETag"));

    // Get object
    let ctx = make_ctx("GET", "/obj-bucket/hello.txt", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(&resp.body[..], b"hello world");
    assert_eq!(resp.content_type, "text/plain");
}

#[tokio::test]
async fn test_get_nonexistent_object() {
    let provider = S3Provider::new();
    let ctx = make_ctx("PUT", "/miss-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let ctx = make_ctx("GET", "/miss-bucket/no-such-key", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 404);
    let body = std::str::from_utf8(&resp.body).unwrap();
    assert!(body.contains("NoSuchKey"));
}

#[tokio::test]
async fn test_head_object() {
    let provider = S3Provider::new();
    let ctx = make_ctx("PUT", "/ho-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "application/json".to_string());
    let ctx = make_ctx_with_headers("PUT", "/ho-bucket/data.json", b"{}", headers);
    provider.dispatch(&ctx).await.unwrap();

    let ctx = make_ctx("HEAD", "/ho-bucket/data.json", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(resp.body.is_empty()); // HEAD returns no body
    assert_eq!(resp.content_type, "application/json");
}

#[tokio::test]
async fn test_delete_object() {
    let provider = S3Provider::new();
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
    let provider = S3Provider::new();
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
    let body = std::str::from_utf8(&resp.body).unwrap();
    assert!(body.contains("a.txt"));
    assert!(body.contains("b.txt"));

    // c.txt still there
    let ctx = make_ctx("GET", "/bulk-bucket/c.txt", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn test_copy_object() {
    let provider = S3Provider::new();
    let ctx = make_ctx("PUT", "/src-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();
    let ctx = make_ctx("PUT", "/dst-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "text/plain".to_string());
    let ctx = make_ctx_with_headers(
        "PUT",
        "/src-bucket/original.txt",
        b"original content",
        headers,
    );
    provider.dispatch(&ctx).await.unwrap();

    let mut headers = HashMap::new();
    headers.insert(
        "x-amz-copy-source".to_string(),
        "/src-bucket/original.txt".to_string(),
    );
    let ctx = make_ctx_with_headers("PUT", "/dst-bucket/copy.txt", b"", headers);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(&resp.body).unwrap();
    assert!(body.contains("CopyObjectResult"));

    let ctx = make_ctx("GET", "/dst-bucket/copy.txt", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(&resp.body[..], b"original content");
}

// ---------------------------------------------------------------------------
// ListObjectsV2
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_objects_v2_prefix() {
    let provider = S3Provider::new();
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
    let body = std::str::from_utf8(&resp.body).unwrap();
    assert!(body.contains("a/1.txt"));
    assert!(body.contains("a/2.txt"));
    assert!(!body.contains("b/1.txt"));
}

#[tokio::test]
async fn test_list_objects_v2_delimiter() {
    let provider = S3Provider::new();
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
    let body = std::str::from_utf8(&resp.body).unwrap();
    assert!(body.contains("CommonPrefixes"));
    assert!(body.contains("root.txt")); // top-level object in Contents
}

#[tokio::test]
async fn test_list_objects_v2_max_keys() {
    let provider = S3Provider::new();
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
    let body = std::str::from_utf8(&resp.body).unwrap();
    assert!(body.contains("<IsTruncated>true</IsTruncated>"));
    assert!(body.contains("NextContinuationToken"));
}

// ---------------------------------------------------------------------------
// Multipart upload
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_multipart_upload() {
    let provider = S3Provider::new();
    let ctx = make_ctx("PUT", "/mp-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    // Create multipart upload
    let mut qp = HashMap::new();
    qp.insert("uploads".to_string(), String::new());
    let ctx = make_ctx_with_query("POST", "/mp-bucket/large.bin", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(&resp.body).unwrap();
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
    let body = std::str::from_utf8(&resp.body).unwrap();
    assert!(body.contains("CompleteMultipartUploadResult"));

    // Get the assembled object
    let ctx = make_ctx("GET", "/mp-bucket/large.bin", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(&resp.body[..], b"part-one-data-part-two-data");
}

#[tokio::test]
async fn test_abort_multipart_upload() {
    let provider = S3Provider::new();
    let ctx = make_ctx("PUT", "/abort-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let mut qp = HashMap::new();
    qp.insert("uploads".to_string(), String::new());
    let ctx = make_ctx_with_query("POST", "/abort-bucket/file.bin", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    let body = std::str::from_utf8(&resp.body).unwrap();
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
    let provider = S3Provider::new();
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
    assert_eq!(&resp.body[..], b"version-2");

    // List versions — should have 2 entries
    let mut qp = HashMap::new();
    qp.insert("versions".to_string(), String::new());
    let ctx = make_ctx_with_query("GET", "/ver-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    let body = std::str::from_utf8(&resp.body).unwrap();
    assert!(body.contains("ListVersionsResult"));
    // Should contain two <Version> entries
    assert_eq!(body.matches("<Version>").count(), 2);
}

// ---------------------------------------------------------------------------
// Bucket policy
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_bucket_policy() {
    let provider = S3Provider::new();
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
    assert_eq!(std::str::from_utf8(&resp.body).unwrap(), policy);

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
    let provider = S3Provider::new();
    let ctx = make_ctx("PUT", "/acl-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let mut qp = HashMap::new();
    qp.insert("acl".to_string(), String::new());
    let ctx = make_ctx_with_query("GET", "/acl-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(&resp.body).unwrap();
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
        b"hello".to_vec(),
        "text/plain",
        HashMap::new(),
    );

    let v = store.get_object("bucket", "key").unwrap();
    assert_eq!(v.data, b"hello");
    assert!(!v.etag.is_empty());
}

#[test]
fn test_store_delete_object() {
    let mut store = S3Store::new();
    store.create_bucket("bucket", "us-east-1");
    store.put_object(
        "bucket",
        "key",
        b"data".to_vec(),
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

    store.put_object("bucket", "k", b"v1".to_vec(), "text/plain", HashMap::new());
    store.put_object("bucket", "k", b"v2".to_vec(), "text/plain", HashMap::new());

    let current = store.get_object("bucket", "k").unwrap();
    assert_eq!(current.data, b"v2");

    let objs = store.list_objects("bucket");
    let obj = objs.into_iter().find(|o| o.key == "k").unwrap();
    assert_eq!(obj.versions.len(), 2);
}

#[test]
fn test_store_multipart() {
    let mut store = S3Store::new();
    store.create_bucket("bucket", "us-east-1");
    let uid =
        store.create_multipart_upload("bucket", "key", "application/octet-stream", HashMap::new());
    store.upload_part(&uid, 1, b"part1".to_vec());
    store.upload_part(&uid, 2, b"part2".to_vec());
    let v = store
        .complete_multipart_upload(&uid, &[(1, String::new()), (2, String::new())])
        .unwrap();
    assert_eq!(v.data, b"part1part2");
}
