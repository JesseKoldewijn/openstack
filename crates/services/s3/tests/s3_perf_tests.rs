/// Performance integration tests for the S3 provider.
///
/// These tests gate the structural properties introduced by our Tier 1–3
/// optimisations. Each test validates a specific invariant; thresholds are
/// generously loose to tolerate debug-mode overhead and CI variance while
/// still catching catastrophic regressions.
///
/// Run with: `cargo test -p openstack-s3`
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use openstack_s3::provider::S3Provider;
use openstack_service_framework::{
    SpooledBody,
    traits::{RequestContext, ServiceProvider},
};

// ---------------------------------------------------------------------------
// Helpers (mirrors s3_tests.rs — test crates cannot share code)
// ---------------------------------------------------------------------------

fn make_ctx(method: &str, path: &str, body: &[u8]) -> RequestContext {
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

/// Build a RequestContext backed by a SpooledBody (raw_body=None).
/// Mirrors the real gateway path for large S3 PutObject bodies.
fn make_ctx_spooled(method: &str, path: &str, data: Vec<u8>, threshold: usize) -> RequestContext {
    let spooled = SpooledBody::from_bytes(Bytes::from(data), threshold)
        .expect("SpooledBody::from_bytes failed");
    RequestContext {
        service: "s3".to_string(),
        operation: String::new(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::Value::Null,
        raw_body: None,
        headers: Default::default(),
        path: path.to_string(),
        method: method.to_string(),
        query_params: HashMap::new(),
        request_id: String::new(),
        spooled_body: Some(Mutex::new(spooled)),
    }
}

async fn new_provider() -> (tempfile::TempDir, S3Provider) {
    let dir = tempfile::tempdir().unwrap();
    let provider = S3Provider::new(dir.path().to_path_buf());
    provider.start().await.unwrap();
    (dir, provider)
}

// ---------------------------------------------------------------------------
// Perf test 1 — BTreeMap key ordering (Tier 2.2)
//
// Objects are PUT in reverse lexicographic order. ListObjectsV2 must return
// them in ascending key order because the store uses a BTreeMap. A failure
// means the BTreeMap was replaced by an unordered HashMap or a Vec without
// an explicit sort.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_btreemap_key_ordering() {
    let (_dir, provider) = new_provider().await;

    let ctx = make_ctx("PUT", "/order-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    // Insert N objects in reverse lexicographic order.
    let n = 100usize;
    for i in (0..n).rev() {
        let path = format!("/order-bucket/obj-{i:04}.dat");
        let ctx = make_ctx("PUT", &path, b"x");
        let resp = provider.dispatch(&ctx).await.unwrap();
        assert_eq!(resp.status_code, 200, "PUT {path} failed");
    }

    // ListObjectsV2 — expect natural BTreeMap order (ascending).
    let mut qp = HashMap::new();
    qp.insert("list-type".to_string(), "2".to_string());
    let ctx = make_ctx_with_query("GET", "/order-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);

    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();

    // Extract <Key> values in document order.
    let keys: Vec<&str> = body
        .split("<Key>")
        .skip(1)
        .filter_map(|s| s.split("</Key>").next())
        .collect();

    assert_eq!(keys.len(), n, "expected {n} keys in listing");

    for (i, key) in keys.iter().enumerate() {
        let expected = format!("obj-{i:04}.dat");
        assert_eq!(
            *key, expected,
            "key[{i}] = {key:?}, want {expected:?}; BTreeMap ordering broken"
        );
    }
}

// ---------------------------------------------------------------------------
// Perf test 2 — ListObjectsV2 max-keys early terminate (Tier 2.4)
//
// With 1,000 objects in the bucket, a request with max-keys=1 should return
// quickly and indicate IsTruncated=true. We gate latency to 500 ms — a full
// linear scan of 1,000 keys would take far longer on slow debug builds.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_list_objects_max_keys_early_terminate() {
    let (_dir, provider) = new_provider().await;

    let ctx = make_ctx("PUT", "/maxk-perf-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    let n = 1_000usize;
    for i in 0..n {
        let path = format!("/maxk-perf-bucket/key-{i:04}.txt");
        let ctx = make_ctx("PUT", &path, b"x");
        let resp = provider.dispatch(&ctx).await.unwrap();
        assert_eq!(resp.status_code, 200);
    }

    // max-keys=1 with 1000 objects — should be near-instant.
    let mut qp = HashMap::new();
    qp.insert("list-type".to_string(), "2".to_string());
    qp.insert("max-keys".to_string(), "1".to_string());
    let ctx = make_ctx_with_query("GET", "/maxk-perf-bucket", b"", qp);

    let start = Instant::now();
    let resp = provider.dispatch(&ctx).await.unwrap();
    let elapsed = start.elapsed();

    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();

    assert!(
        body.contains("<IsTruncated>true</IsTruncated>"),
        "max-keys=1 with {n} objects should produce IsTruncated=true"
    );

    // Count <Key> occurrences — should be exactly 1.
    let key_count = body.matches("<Key>").count();
    assert_eq!(key_count, 1, "max-keys=1 should return exactly 1 key");

    assert!(
        elapsed.as_millis() < 500,
        "ListObjectsV2 max-keys=1 took {}ms — expected <500ms; \
         early-termination path may be broken",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Perf test 3 — ListObjectsV2 prefix filter + pagination
//
// Many objects are stored with two different prefixes. The prefix filter must
// return only the matching subset, and max-keys must produce correct
// IsTruncated + NextContinuationToken pagination.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_list_objects_prefix_and_pagination() {
    let (_dir, provider) = new_provider().await;

    let ctx = make_ctx("PUT", "/prefix-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    // 30 objects under prefix "alpha/", 20 objects under prefix "beta/".
    let alpha_count = 30usize;
    let beta_count = 20usize;
    for i in 0..alpha_count {
        let path = format!("/prefix-bucket/alpha/item-{i:03}.txt");
        let ctx = make_ctx("PUT", &path, b"a");
        provider.dispatch(&ctx).await.unwrap();
    }
    for i in 0..beta_count {
        let path = format!("/prefix-bucket/beta/item-{i:03}.txt");
        let ctx = make_ctx("PUT", &path, b"b");
        provider.dispatch(&ctx).await.unwrap();
    }

    // List with prefix="alpha/" — expect exactly alpha_count items, no beta keys.
    let mut qp = HashMap::new();
    qp.insert("list-type".to_string(), "2".to_string());
    qp.insert("prefix".to_string(), "alpha/".to_string());
    let ctx = make_ctx_with_query("GET", "/prefix-bucket", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();

    assert!(
        !body.contains("beta/"),
        "prefix=alpha/ listing must not include beta/ keys"
    );
    let alpha_keys: Vec<&str> = body
        .split("<Key>")
        .skip(1)
        .filter_map(|s| s.split("</Key>").next())
        .collect();
    assert_eq!(
        alpha_keys.len(),
        alpha_count,
        "prefix=alpha/ should return {alpha_count} keys, got {}",
        alpha_keys.len()
    );

    // List with prefix="alpha/" + max-keys=10 — must produce IsTruncated=true
    // and a NextContinuationToken for the remaining 20 items.
    let mut qp2 = HashMap::new();
    qp2.insert("list-type".to_string(), "2".to_string());
    qp2.insert("prefix".to_string(), "alpha/".to_string());
    qp2.insert("max-keys".to_string(), "10".to_string());
    let ctx2 = make_ctx_with_query("GET", "/prefix-bucket", b"", qp2);
    let resp2 = provider.dispatch(&ctx2).await.unwrap();
    assert_eq!(resp2.status_code, 200);
    let body2 = std::str::from_utf8(resp2.body.as_bytes()).unwrap();

    assert!(
        body2.contains("<IsTruncated>true</IsTruncated>"),
        "max-keys=10 over {alpha_count} alpha keys should be truncated"
    );
    assert!(
        body2.contains("NextContinuationToken"),
        "truncated listing must include NextContinuationToken"
    );
    let page1_keys: Vec<&str> = body2
        .split("<Key>")
        .skip(1)
        .filter_map(|s| s.split("</Key>").next())
        .collect();
    assert_eq!(
        page1_keys.len(),
        10,
        "first page should contain exactly 10 keys"
    );

    // Round-trip the continuation token: extract it from page 1 and use it
    // to request page 2, verifying we get the next batch of keys.
    let token = body2
        .split("<NextContinuationToken>")
        .nth(1)
        .and_then(|s| s.split("</NextContinuationToken>").next())
        .expect("NextContinuationToken must be present in truncated response");
    let mut qp3 = std::collections::HashMap::new();
    qp3.insert("list-type".to_string(), "2".to_string());
    qp3.insert("prefix".to_string(), "alpha/".to_string());
    qp3.insert("max-keys".to_string(), "10".to_string());
    qp3.insert("continuation-token".to_string(), token.to_string());
    let ctx3 = make_ctx_with_query("GET", "/prefix-bucket", b"", qp3);
    let resp3 = provider.dispatch(&ctx3).await.unwrap();
    assert_eq!(resp3.status_code, 200, "page 2 list failed");
    let body3 = std::str::from_utf8(resp3.body.as_bytes()).unwrap();
    let page2_keys: Vec<&str> = body3
        .split("<Key>")
        .skip(1)
        .filter_map(|s| s.split("</Key>").next())
        .collect();
    assert!(
        !page2_keys.is_empty(),
        "page 2 should contain at least one key (continuation token not honoured)"
    );
    // Every page-2 key must be lexicographically greater than every page-1 key.
    let last_page1 = page1_keys.last().copied().unwrap();
    for k in &page2_keys {
        assert!(
            *k > last_page1,
            "page 2 key {k:?} is not after last page-1 key {last_page1:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Perf test 4 — CopyObject fast path for large objects
//
// A large object (~300 KiB) is PUT via spooled body, then CopyObject copies
// it to a new key. We verify:
//   a) data integrity (GET after copy returns identical bytes)
//   b) CopyObject latency < 2 s (file-level copy, not in-memory re-read)
//   c) the destination object can be overwritten by a second CopyObject
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_copy_object_large_fast_path() {
    let (_dir, provider) = new_provider().await;

    let ctx = make_ctx("PUT", "/copy-src-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();
    let ctx = make_ctx("PUT", "/copy-dst-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    // Large deterministic payload (~300 KiB).
    let src_data: Vec<u8> = (0u8..=254).cycle().take(300 * 1024).collect();

    // PUT via spooled body (threshold=0 → disk spill).
    let ctx = make_ctx_spooled("PUT", "/copy-src-bucket/large.bin", src_data.clone(), 0);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200, "PUT large.bin failed");

    // CopyObject to the destination bucket — time it.
    let start = Instant::now();
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::HeaderName::from_static("x-amz-copy-source"),
        http::header::HeaderValue::from_static("/copy-src-bucket/large.bin"),
    );
    let ctx = make_ctx_with_headers("PUT", "/copy-dst-bucket/copy.bin", b"", headers);
    let copy_resp = provider.dispatch(&ctx).await.unwrap();
    let elapsed = start.elapsed();

    assert_eq!(copy_resp.status_code, 200, "CopyObject failed");
    let copy_body = std::str::from_utf8(copy_resp.body.as_bytes()).unwrap();
    assert!(
        copy_body.contains("CopyObjectResult"),
        "CopyObject response should contain CopyObjectResult"
    );
    assert!(
        elapsed.as_millis() < 2000,
        "CopyObject of 300 KiB took {}ms — expected <2000ms",
        elapsed.as_millis()
    );

    // Verify data integrity.
    let ctx = make_ctx("GET", "/copy-dst-bucket/copy.bin", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let got = resp.body.into_bytes().await.unwrap();
    assert_eq!(
        got.len(),
        src_data.len(),
        "copy length mismatch: got {} bytes, expected {}",
        got.len(),
        src_data.len()
    );
    assert_eq!(&got[..], &src_data[..], "copy data integrity check failed");

    // Overwrite the destination with a second CopyObject from a different source.
    let src_data_b: Vec<u8> = (0..(300 * 1024)).map(|i| ((i * 7) % 251) as u8).collect();
    let ctx = make_ctx_spooled("PUT", "/copy-src-bucket/large-b.bin", src_data_b.clone(), 0);
    provider.dispatch(&ctx).await.unwrap();

    let mut headers2 = http::HeaderMap::new();
    headers2.insert(
        http::header::HeaderName::from_static("x-amz-copy-source"),
        http::header::HeaderValue::from_static("/copy-src-bucket/large-b.bin"),
    );
    let ctx = make_ctx_with_headers("PUT", "/copy-dst-bucket/copy.bin", b"", headers2);
    let overwrite_resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(
        overwrite_resp.status_code, 200,
        "overwrite CopyObject failed"
    );

    let ctx = make_ctx("GET", "/copy-dst-bucket/copy.bin", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    let got_b = resp.body.into_bytes().await.unwrap();
    assert_eq!(
        &got_b[..],
        &src_data_b[..],
        "overwrite copy integrity check failed"
    );
}

// ---------------------------------------------------------------------------
// Perf test 5 — Concurrent PutObject across independent buckets (Tier 3.1)
//
// N tasks each write M objects to their own bucket simultaneously via an
// Arc<S3Provider>. All writes must succeed (no deadlock, no data loss) and
// the final object count per bucket must be exact.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_concurrent_put_across_buckets() {
    let (_dir, provider) = new_provider().await;
    let provider = Arc::new(provider);
    let n_buckets = 10usize;
    let objects_per_bucket = 20usize;

    // Pre-create all buckets sequentially.
    for b in 0..n_buckets {
        let ctx = make_ctx("PUT", &format!("/conc-bucket-{b}"), b"");
        provider.dispatch(&ctx).await.unwrap();
    }

    // Spawn one task per bucket.
    let mut handles = Vec::with_capacity(n_buckets);
    for b in 0..n_buckets {
        let p = Arc::clone(&provider);
        handles.push(tokio::spawn(async move {
            for i in 0..objects_per_bucket {
                let path = format!("/conc-bucket-{b}/obj-{i:03}.bin");
                let data: Vec<u8> = vec![(b * objects_per_bucket + i) as u8; 64];
                let ctx = make_ctx("PUT", &path, &data);
                let resp = p.dispatch(&ctx).await.unwrap();
                assert_eq!(
                    resp.status_code, 200,
                    "conc-bucket-{b} PUT obj-{i:03}.bin failed"
                );
            }
        }));
    }

    for handle in handles {
        tokio::time::timeout(Duration::from_secs(30), handle)
            .await
            .expect("concurrent PUT task timed out after 30 s — possible deadlock")
            .expect("concurrent PUT task panicked");
    }

    // Verify object count per bucket via ListObjectsV2.
    for b in 0..n_buckets {
        let mut qp = HashMap::new();
        qp.insert("list-type".to_string(), "2".to_string());
        let ctx = make_ctx_with_query("GET", &format!("/conc-bucket-{b}"), b"", qp);
        let resp = provider.dispatch(&ctx).await.unwrap();
        assert_eq!(resp.status_code, 200);

        let body = std::str::from_utf8(resp.body.as_bytes()).unwrap();
        let key_count = body.matches("<Key>").count();
        assert_eq!(
            key_count, objects_per_bucket,
            "conc-bucket-{b} expected {objects_per_bucket} objects, got {key_count}"
        );
    }
}

// ---------------------------------------------------------------------------
// Perf test 6 — Multipart VecDeque part ordering
//
// Parts are uploaded out-of-order (3, 1, 2). CompleteMultipartUpload must
// assemble them in part-number order regardless of upload order. This
// validates that the VecDeque / part storage does not depend on insertion
// order when assembling the final object.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_multipart_out_of_order_parts() {
    let (_dir, provider) = new_provider().await;

    let ctx = make_ctx("PUT", "/mp-order-bucket", b"");
    provider.dispatch(&ctx).await.unwrap();

    // Initiate multipart upload.
    let mut qp = HashMap::new();
    qp.insert("uploads".to_string(), String::new());
    let ctx = make_ctx_with_query("POST", "/mp-order-bucket/assembled.bin", b"", qp);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let body_str = std::str::from_utf8(resp.body.as_bytes()).unwrap();
    let upload_id = {
        let start = body_str.find("<UploadId>").unwrap() + "<UploadId>".len();
        let end = body_str.find("</UploadId>").unwrap();
        body_str[start..end].to_string()
    };

    // Upload parts in reverse order: part 3, part 2, part 1.
    // Each part has distinct content so wrong ordering is detectable.
    let parts = [
        (3u32, b"CCCCC" as &[u8]),
        (2u32, b"BBBBB" as &[u8]),
        (1u32, b"AAAAA" as &[u8]),
    ];
    let mut etags: Vec<(u32, String)> = Vec::new();
    for (part_num, content) in &parts {
        let mut qp = HashMap::new();
        qp.insert("uploadId".to_string(), upload_id.clone());
        qp.insert("partNumber".to_string(), part_num.to_string());
        let ctx = make_ctx_with_query("PUT", "/mp-order-bucket/assembled.bin", content, qp);
        let resp = provider.dispatch(&ctx).await.unwrap();
        assert_eq!(resp.status_code, 200, "part {part_num} upload failed");
        let etag = resp
            .headers
            .iter()
            .find(|(k, _)| k == "ETag")
            .map(|(_, v)| v.clone())
            .expect("ETag header missing for part");
        etags.push((*part_num, etag));
    }

    // Sort etags by part number for CompleteMultipartUpload (ascending).
    etags.sort_by_key(|(num, _)| *num);

    let complete_body = {
        let parts_xml: String = etags
            .iter()
            .map(|(num, etag)| {
                format!("<Part><PartNumber>{num}</PartNumber><ETag>{etag}</ETag></Part>")
            })
            .collect();
        format!("<CompleteMultipartUpload>{parts_xml}</CompleteMultipartUpload>")
    };

    let mut qp = HashMap::new();
    qp.insert("uploadId".to_string(), upload_id.clone());
    let ctx = make_ctx_with_query(
        "POST",
        "/mp-order-bucket/assembled.bin",
        complete_body.as_bytes(),
        qp,
    );
    let complete_resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(
        complete_resp.status_code,
        200,
        "CompleteMultipartUpload failed: {}",
        std::str::from_utf8(complete_resp.body.as_bytes()).unwrap_or("<invalid utf8>")
    );
    let complete_str = std::str::from_utf8(complete_resp.body.as_bytes()).unwrap();
    assert!(
        complete_str.contains("CompleteMultipartUploadResult"),
        "response should contain CompleteMultipartUploadResult"
    );

    // GET the assembled object — must be AAAAABBBBBCCCCC (part 1, 2, 3 in order).
    let ctx = make_ctx("GET", "/mp-order-bucket/assembled.bin", b"");
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let assembled = resp.body.into_bytes().await.unwrap();
    assert_eq!(
        &assembled[..],
        b"AAAAABBBBBCCCCC",
        "assembled object bytes are wrong — part ordering is broken; got: {:?}",
        std::str::from_utf8(&assembled).unwrap_or("<non-utf8>")
    );
}
