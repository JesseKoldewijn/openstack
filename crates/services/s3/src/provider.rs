use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use tokio_util::io::ReaderStream;
use tracing::{debug, warn};

use crate::object_store::ObjectFileStore;
use crate::store::{ObjectDataRef, S3Store};

pub struct S3Provider {
    store: Arc<AccountRegionBundle<S3Store>>,
    /// Path for S3 object file storage.
    s3_objects_dir: PathBuf,
    /// Filesystem object store, initialized in `start()`.
    object_store: tokio::sync::OnceCell<ObjectFileStore>,
}

impl S3Provider {
    pub fn new(s3_objects_dir: impl Into<PathBuf>) -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
            s3_objects_dir: s3_objects_dir.into(),
            object_store: tokio::sync::OnceCell::new(),
        }
    }

    /// Returns a [`PersistableStore`](openstack_state::PersistableStore) that
    /// shares the same in-memory store as this provider.  Call this **before**
    /// consuming the provider via `ServicePluginManager::register()`.
    pub fn persistable_store(&self) -> Arc<dyn openstack_state::PersistableStore> {
        Arc::new(crate::persistence::S3PersistableStore::new(
            Arc::clone(&self.store),
            self.s3_objects_dir.clone(),
        ))
    }

    /// Get the object file store, panicking if not yet initialized.
    ///
    /// This is safe because `ensure_running()` always calls `start()`
    /// before any `dispatch()` call.
    fn file_store(&self) -> &ObjectFileStore {
        self.object_store
            .get()
            .expect("ObjectFileStore not initialized — start() not called")
    }
}

// ---------------------------------------------------------------------------
// XML helpers
// ---------------------------------------------------------------------------

fn xml_ok(xml: &str) -> DispatchResponse {
    DispatchResponse::ok_xml(xml.to_string())
}

fn xml_response(status: u16, xml: String) -> DispatchResponse {
    DispatchResponse {
        status_code: status,
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
        content_type: "text/xml".to_string(),
        headers: Vec::new(),
    }
}

fn s3_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    xml_response(
        status,
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Error><Code>{code}</Code><Message>{message}</Message></Error>"
        ),
    )
}

fn empty_200() -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::new()),
        content_type: String::new(),
        headers: Vec::new(),
    }
}

fn empty_204() -> DispatchResponse {
    DispatchResponse {
        status_code: 204,
        body: ResponseBody::Buffered(Bytes::new()),
        content_type: String::new(),
        headers: Vec::new(),
    }
}

/// Extract bucket name from S3 path: first path segment.
/// Path may look like `/my-bucket` or `/my-bucket/some/key`.
fn bucket_from_path(path: &str) -> Option<String> {
    let path = path.trim_start_matches('/');
    let seg = path.split('/').next()?;
    if seg.is_empty() {
        None
    } else {
        Some(seg.to_string())
    }
}

/// Extract key from path (everything after /{bucket}/).
fn key_from_path(path: &str) -> String {
    let path = path.trim_start_matches('/');
    let slash = path.find('/').unwrap_or(path.len());
    path[slash..].trim_start_matches('/').to_string()
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// Bucket operations
// ---------------------------------------------------------------------------

fn handle_create_bucket(store: &mut S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };

    if store.bucket_exists(&bucket) {
        return s3_error(
            "BucketAlreadyOwnedByYou",
            "Bucket already owned by you",
            409,
        );
    }

    store.create_bucket(&bucket, &ctx.region);
    debug!(bucket = %bucket, "CreateBucket");

    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::new()),
        content_type: String::new(),
        headers: vec![("Location".to_string(), format!("/{bucket}"))],
    }
}

/// Async DeleteBucket — also removes the bucket directory from filesystem.
async fn handle_delete_bucket_async(
    store_bundle: &AccountRegionBundle<S3Store>,
    file_store: &ObjectFileStore,
    ctx: &RequestContext,
) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };

    {
        let store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        if !store.bucket_exists(&bucket) {
            return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
        }
        if !store.is_bucket_empty(&bucket) {
            return s3_error("BucketNotEmpty", "The bucket is not empty", 409);
        }
    }

    {
        let mut store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        store.delete_bucket(&bucket);
    }

    // Clean up bucket directory on filesystem (async I/O)
    let _ = file_store
        .delete_bucket_dir(&ctx.account_id, &ctx.region, &bucket)
        .await;

    empty_204()
}

fn handle_head_bucket(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };

    if store.bucket_exists(&bucket) {
        empty_200()
    } else {
        s3_error("NoSuchBucket", "The specified bucket does not exist", 404)
    }
}

fn handle_list_buckets(store: &S3Store, _ctx: &RequestContext) -> DispatchResponse {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListAllMyBucketsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Owner><ID>000000000000</ID><DisplayName>localstack</DisplayName></Owner><Buckets>",
    );
    let mut buckets: Vec<_> = store.buckets.values().collect();
    buckets.sort_by_key(|b| &b.name);
    for b in buckets {
        xml.push_str(&format!(
            "<Bucket><Name>{}</Name><CreationDate>{}</CreationDate></Bucket>",
            escape_xml(&b.name),
            b.creation_date.format("%Y-%m-%dT%H:%M:%S.000Z")
        ));
    }
    xml.push_str("</Buckets></ListAllMyBucketsResult>");
    xml_ok(&xml)
}

fn handle_get_bucket_location(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };

    let b = match store.get_bucket(&bucket) {
        Some(b) => b,
        None => return s3_error("NoSuchBucket", "The specified bucket does not exist", 404),
    };

    // us-east-1 is represented as empty string in the XML
    let location = if b.region == "us-east-1" {
        String::new()
    } else {
        b.region.clone()
    };

    xml_ok(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<LocationConstraint xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">{location}</LocationConstraint>"
    ))
}

// ---------------------------------------------------------------------------
// Object operations
// ---------------------------------------------------------------------------

/// Async PutObject — writes body to filesystem via ObjectFileStore, then
/// stores metadata in S3Store.
async fn handle_put_object_async(
    store_bundle: &AccountRegionBundle<S3Store>,
    file_store: &ObjectFileStore,
    ctx: &RequestContext,
) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    let key = key_from_path(&ctx.path);

    // Check bucket exists (short-lived guard)
    {
        let store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        if !store.bucket_exists(&bucket) {
            return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
        }
    }

    let content_type = ctx
        .headers
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let metadata: HashMap<String, String> = ctx
        .headers
        .iter()
        .filter_map(|(k, v)| {
            k.strip_prefix("x-amz-meta-")
                .map(|mk| (mk.to_string(), v.clone()))
        })
        .collect();

    // Determine versioning state (short-lived guard)
    let versioning_enabled = {
        let store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        store
            .get_bucket(&bucket)
            .map(|b| b.versioning.as_str() == "Enabled")
            .unwrap_or(false)
    };

    // Generate version_id now so we can use it for the file path
    let version_id = if versioning_enabled {
        uuid::Uuid::new_v4().to_string()
    } else {
        "null".to_string()
    };

    // Get body data — prefer spooled_body, fall back to raw_body
    let body_bytes = ctx.raw_body.to_vec();

    // Compute MD5 etag from the data
    let etag = format!("\"{}\"", hex::encode(md5::compute(&body_bytes).0));
    let size = body_bytes.len() as u64;

    // Write to filesystem (async I/O — no store guard held)
    let file_path = match file_store
        .write_object(
            &ctx.account_id,
            &ctx.region,
            &bucket,
            &key,
            &version_id,
            &body_bytes,
        )
        .await
    {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "Failed to write object to filesystem");
            return s3_error("InternalError", "Failed to store object", 500);
        }
    };

    // Build version and store in S3Store (short-lived guard)
    let response_headers = {
        let mut store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        let version = crate::store::ObjectVersion {
            version_id: version_id.clone(),
            last_modified: chrono::Utc::now(),
            etag: etag.clone(),
            content_type,
            content_encoding: None,
            content_disposition: None,
            cache_control: None,
            size,
            metadata,
            acl: "private".to_string(),
            data: ObjectDataRef::FileRef(file_path),
            delete_marker: false,
        };
        store.put_object_version(&bucket, &key, version);

        let mut headers = vec![("ETag".to_string(), etag)];
        if version_id != "null" {
            headers.push(("x-amz-version-id".to_string(), version_id));
        }
        headers
    };

    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::new()),
        content_type: String::new(),
        headers: response_headers,
    }
}

/// Async GetObject — streams file-backed objects via ReaderStream.
async fn handle_get_object_async(
    store_bundle: &AccountRegionBundle<S3Store>,
    ctx: &RequestContext,
) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    let key = key_from_path(&ctx.path);

    // Short-lived guard to read version metadata
    let version_info = {
        let store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);

        if !store.bucket_exists(&bucket) {
            return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
        }

        let version_id = ctx.query_params.get("versionId").cloned();
        let version = if let Some(ref vid) = version_id {
            store.get_object_version(&bucket, &key, vid)
        } else {
            store.get_object(&bucket, &key)
        };

        match version {
            None => None,
            Some(v) => Some((
                v.data.clone(),
                v.etag.clone(),
                v.last_modified,
                v.size,
                v.version_id.clone(),
                v.content_type.clone(),
                v.content_encoding.clone(),
                v.metadata.clone(),
            )),
        }
    };

    match version_info {
        None => s3_error("NoSuchKey", "The specified key does not exist", 404),
        Some((data, etag, last_modified, size, version_id, content_type, content_encoding, metadata)) => {
            let mut headers = vec![
                ("ETag".to_string(), etag),
                (
                    "Last-Modified".to_string(),
                    last_modified
                        .format("%a, %d %b %Y %H:%M:%S GMT")
                        .to_string(),
                ),
                ("Content-Length".to_string(), size.to_string()),
            ];
            if version_id != "null" {
                headers.push(("x-amz-version-id".to_string(), version_id));
            }
            for (mk, mv) in &metadata {
                headers.push((format!("x-amz-meta-{mk}"), mv.clone()));
            }
            if let Some(enc) = &content_encoding {
                headers.push(("Content-Encoding".to_string(), enc.clone()));
            }

            let body = match data {
                ObjectDataRef::Inline(bytes) => ResponseBody::Buffered(Bytes::from(bytes)),
                ObjectDataRef::FileRef(path) => {
                    match ObjectFileStore::read_object_at(&path).await {
                        Ok(file) => {
                            let stream = ReaderStream::new(file);
                            ResponseBody::Streaming {
                                stream: Box::pin(stream),
                                content_length: Some(size),
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, path = %path.display(), "Failed to open object file for streaming");
                            return s3_error("InternalError", "Failed to read object", 500);
                        }
                    }
                }
            };

            DispatchResponse {
                status_code: 200,
                body,
                content_type,
                headers,
            }
        }
    }
}

fn handle_head_object(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    let key = key_from_path(&ctx.path);

    if !store.bucket_exists(&bucket) {
        return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
    }

    match store.get_object(&bucket, &key) {
        None => s3_error("NoSuchKey", "The specified key does not exist", 404),
        Some(v) => {
            let mut headers = vec![
                ("ETag".to_string(), v.etag.clone()),
                (
                    "Last-Modified".to_string(),
                    v.last_modified
                        .format("%a, %d %b %Y %H:%M:%S GMT")
                        .to_string(),
                ),
                ("Content-Length".to_string(), v.size.to_string()),
            ];
            if v.version_id != "null" {
                headers.push(("x-amz-version-id".to_string(), v.version_id.clone()));
            }
            for (mk, mv) in &v.metadata {
                headers.push((format!("x-amz-meta-{mk}"), mv.clone()));
            }
            DispatchResponse {
                status_code: 200,
                body: ResponseBody::Buffered(Bytes::new()),
                content_type: v.content_type.clone(),
                headers,
            }
        }
    }
}

/// Async DeleteObject — deletes backing file for FileRef data.
async fn handle_delete_object_async(
    store_bundle: &AccountRegionBundle<S3Store>,
    ctx: &RequestContext,
) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    let key = key_from_path(&ctx.path);

    {
        let store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        if !store.bucket_exists(&bucket) {
            return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
        }
    }

    let version_id_param = ctx.query_params.get("versionId").cloned();
    let mut headers = Vec::new();

    if let Some(vid) = version_id_param {
        // Delete a specific version — get it first for file cleanup
        let removed = {
            let mut store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
            store.delete_object_version(&bucket, &key, &vid)
        };
        if let Some(removed_version) = &removed {
            if let ObjectDataRef::FileRef(path) = &removed_version.data {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
        headers.push(("x-amz-version-id".to_string(), vid));
    } else {
        let deleted = {
            let mut store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
            store.delete_object(&bucket, &key)
        };
        if let Some(deleted_version) = &deleted {
            // If not versioned, the actual object was removed — clean up file
            if !deleted_version.delete_marker {
                if let ObjectDataRef::FileRef(path) = &deleted_version.data {
                    let _ = tokio::fs::remove_file(path).await;
                }
            }
            if deleted_version.delete_marker {
                headers.push(("x-amz-delete-marker".to_string(), "true".to_string()));
                headers.push(("x-amz-version-id".to_string(), deleted_version.version_id.clone()));
            }
        }
    }

    DispatchResponse {
        status_code: 204,
        body: ResponseBody::Buffered(Bytes::new()),
        content_type: String::new(),
        headers,
    }
}

/// Async DeleteObjects (batch) — deletes backing files for removed objects.
async fn handle_delete_objects_async(
    store_bundle: &AccountRegionBundle<S3Store>,
    ctx: &RequestContext,
) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };

    {
        let store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        if !store.bucket_exists(&bucket) {
            return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
        }
    }

    // Parse the XML body for object keys
    let body = std::str::from_utf8(&ctx.raw_body).unwrap_or("");

    let keys: Vec<(String, Option<String>)> = {
        let mut result = Vec::new();
        let mut remaining = body;
        while let Some(obj_start) = remaining.find("<Object>") {
            remaining = &remaining[obj_start + 8..];
            let obj_end = remaining.find("</Object>").unwrap_or(remaining.len());
            let obj_xml = &remaining[..obj_end];
            let key = extract_xml_text(obj_xml, "Key").unwrap_or_default();
            let version_id = extract_xml_text(obj_xml, "VersionId");
            if !key.is_empty() {
                result.push((key, version_id));
            }
            remaining = &remaining[obj_end..];
        }
        result
    };

    let mut deleted_xml = String::new();
    let mut files_to_delete: Vec<PathBuf> = Vec::new();

    {
        let mut store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);

        for (key, version_id) in &keys {
            if let Some(vid) = version_id {
                if let Some(removed) = store.delete_object_version(&bucket, key, vid) {
                    if let ObjectDataRef::FileRef(path) = &removed.data {
                        files_to_delete.push(path.clone());
                    }
                }
                deleted_xml.push_str(&format!(
                    "<Deleted><Key>{}</Key><VersionId>{}</VersionId></Deleted>",
                    escape_xml(key),
                    escape_xml(vid)
                ));
            } else {
                if let Some(removed) = store.delete_object(&bucket, key) {
                    if !removed.delete_marker {
                        if let ObjectDataRef::FileRef(path) = &removed.data {
                            files_to_delete.push(path.clone());
                        }
                    }
                }
                deleted_xml.push_str(&format!(
                    "<Deleted><Key>{}</Key></Deleted>",
                    escape_xml(key)
                ));
            }
        }
    }

    // Clean up files (async I/O — no store guard held)
    for path in &files_to_delete {
        let _ = tokio::fs::remove_file(path).await;
    }

    xml_ok(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<DeleteResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">{deleted_xml}</DeleteResult>"
    ))
}

/// Async CopyObject — uses filesystem-level copy for FileRef sources.
async fn handle_copy_object_async(
    store_bundle: &AccountRegionBundle<S3Store>,
    file_store: &ObjectFileStore,
    ctx: &RequestContext,
) -> DispatchResponse {
    let dest_bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Destination bucket required", 400),
    };
    let dest_key = key_from_path(&ctx.path);

    let copy_source = match ctx.headers.get("x-amz-copy-source") {
        Some(s) => s.clone(),
        None => return s3_error("InvalidRequest", "Missing x-amz-copy-source header", 400),
    };

    let copy_source = urlencoding_decode(&copy_source);
    let (src_bucket, src_key) = parse_copy_source(&copy_source);

    // Read source object metadata (short-lived guard)
    let src_info = {
        let store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);

        if !store.bucket_exists(&dest_bucket) {
            return s3_error("NoSuchBucket", "Destination bucket does not exist", 404);
        }

        let src = store.get_object(&src_bucket, &src_key);
        match src {
            None => return s3_error("NoSuchKey", "Source key does not exist", 404),
            Some(v) => (
                v.data.clone(),
                v.content_type.clone(),
                v.metadata.clone(),
                v.etag.clone(),
                v.size,
                v.version_id.clone(),
                v.last_modified,
            ),
        }
    };

    let (src_data, ct, meta, src_etag, src_size, src_version_id, src_last_modified) = src_info;

    // Determine versioning for destination
    let versioning_enabled = {
        let store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        store
            .get_bucket(&dest_bucket)
            .map(|b| b.versioning.as_str() == "Enabled")
            .unwrap_or(false)
    };

    let dest_version_id = if versioning_enabled {
        uuid::Uuid::new_v4().to_string()
    } else {
        "null".to_string()
    };

    // Copy data — filesystem copy for FileRef, write for Inline
    let dest_data = match &src_data {
        ObjectDataRef::FileRef(_path) => {
            match file_store
                .copy_object(
                    &ctx.account_id,
                    &ctx.region,
                    &src_bucket,
                    &src_key,
                    &src_version_id,
                    &ctx.account_id,
                    &ctx.region,
                    &dest_bucket,
                    &dest_key,
                    &dest_version_id,
                )
                .await
            {
                Ok(dest_path) => ObjectDataRef::FileRef(dest_path),
                Err(e) => {
                    warn!(error = %e, "Failed to copy object on filesystem");
                    return s3_error("InternalError", "Failed to copy object", 500);
                }
            }
        }
        ObjectDataRef::Inline(bytes) => {
            // Write inline data to filesystem for the destination
            match file_store
                .write_object(
                    &ctx.account_id,
                    &ctx.region,
                    &dest_bucket,
                    &dest_key,
                    &dest_version_id,
                    bytes,
                )
                .await
            {
                Ok(dest_path) => ObjectDataRef::FileRef(dest_path),
                Err(e) => {
                    warn!(error = %e, "Failed to write copied object to filesystem");
                    return s3_error("InternalError", "Failed to copy object", 500);
                }
            }
        }
    };

    // Build version and store
    let version = crate::store::ObjectVersion {
        version_id: dest_version_id.clone(),
        last_modified: chrono::Utc::now(),
        etag: src_etag.clone(),
        content_type: ct,
        content_encoding: None,
        content_disposition: None,
        cache_control: None,
        size: src_size,
        metadata: meta,
        acl: "private".to_string(),
        data: dest_data,
        delete_marker: false,
    };

    let (etag, last_modified) = {
        let mut store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        store.put_object_version(&dest_bucket, &dest_key, version);

        let v = store.get_object(&dest_bucket, &dest_key);
        match v {
            Some(v) => (v.etag.clone(), v.last_modified),
            None => (src_etag, src_last_modified),
        }
    };

    xml_ok(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<CopyObjectResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<LastModified>{}</LastModified><ETag>{}</ETag></CopyObjectResult>",
        last_modified.format("%Y-%m-%dT%H:%M:%S.000Z"),
        escape_xml(&etag)
    ))
}

// ---------------------------------------------------------------------------
// ListObjectsV2
// ---------------------------------------------------------------------------

fn handle_list_objects_v2(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };

    if !store.bucket_exists(&bucket) {
        return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
    }

    let prefix = ctx.query_params.get("prefix").cloned().unwrap_or_default();
    let delimiter = ctx
        .query_params
        .get("delimiter")
        .cloned()
        .unwrap_or_default();
    let max_keys: usize = ctx
        .query_params
        .get("max-keys")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000)
        .min(1000);
    let continuation_token = ctx.query_params.get("continuation-token").cloned();
    let start_after = ctx.query_params.get("start-after").cloned();

    // Collect and sort all current (non-delete-marker) objects with matching prefix
    let mut all_keys: Vec<String> = store
        .list_objects(&bucket)
        .into_iter()
        .filter_map(|obj| {
            if obj.current().is_some() && obj.key.starts_with(&prefix) {
                Some(obj.key.clone())
            } else {
                None
            }
        })
        .collect();
    all_keys.sort();

    // Apply start_after / continuation_token
    let skip_after = continuation_token.as_deref().or(start_after.as_deref());
    if let Some(skip) = skip_after {
        all_keys.retain(|k| k.as_str() > skip);
    }

    // Common prefix (delimiter) handling
    let mut common_prefixes: Vec<String> = Vec::new();
    let mut content_keys: Vec<String> = Vec::new();

    if delimiter.is_empty() {
        content_keys = all_keys.clone();
    } else {
        for key in &all_keys {
            let suffix = &key[prefix.len()..];
            if let Some(pos) = suffix.find(&*delimiter) {
                let cp = format!("{}{}{}", prefix, &suffix[..pos], delimiter);
                if !common_prefixes.contains(&cp) {
                    common_prefixes.push(cp);
                }
            } else {
                content_keys.push(key.clone());
            }
        }
    }

    let truncated = content_keys.len() + common_prefixes.len() > max_keys;
    content_keys.truncate(max_keys.saturating_sub(common_prefixes.len()));

    let next_token = if truncated {
        content_keys.last().cloned()
    } else {
        None
    };

    let key_count = content_keys.len() + common_prefixes.len();

    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Name>{}</Name><Prefix>{}</Prefix><MaxKeys>{}</MaxKeys>\
<KeyCount>{}</KeyCount><IsTruncated>{}</IsTruncated>",
        escape_xml(&bucket),
        escape_xml(&prefix),
        max_keys,
        key_count,
        truncated
    );

    if let Some(ref t) = next_token {
        xml.push_str(&format!(
            "<NextContinuationToken>{}</NextContinuationToken>",
            escape_xml(t)
        ));
    }

    for key in &content_keys {
        if let Some(v) = store.get_object(&bucket, key) {
            xml.push_str(&format!(
                "<Contents>\
<Key>{key}</Key>\
<LastModified>{lm}</LastModified>\
<ETag>{etag}</ETag>\
<Size>{size}</Size>\
<StorageClass>STANDARD</StorageClass>\
</Contents>",
                key = escape_xml(key),
                lm = v.last_modified.format("%Y-%m-%dT%H:%M:%S.000Z"),
                etag = escape_xml(&v.etag),
                size = v.size,
            ));
        }
    }

    for cp in &common_prefixes {
        xml.push_str(&format!(
            "<CommonPrefixes><Prefix>{}</Prefix></CommonPrefixes>",
            escape_xml(cp)
        ));
    }

    xml.push_str("</ListBucketResult>");
    xml_ok(&xml)
}

// ListObjectsV1 (backwards compat)
fn handle_list_objects(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };

    if !store.bucket_exists(&bucket) {
        return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
    }

    let prefix = ctx.query_params.get("prefix").cloned().unwrap_or_default();
    let delimiter = ctx
        .query_params
        .get("delimiter")
        .cloned()
        .unwrap_or_default();
    let max_keys: usize = ctx
        .query_params
        .get("max-keys")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000)
        .min(1000);
    let marker = ctx.query_params.get("marker").cloned().unwrap_or_default();

    let mut all_keys: Vec<String> = store
        .list_objects(&bucket)
        .into_iter()
        .filter_map(|obj| {
            if obj.current().is_some() && obj.key.starts_with(&prefix) {
                Some(obj.key.clone())
            } else {
                None
            }
        })
        .collect();
    all_keys.sort();
    if !marker.is_empty() {
        all_keys.retain(|k| k.as_str() > marker.as_str());
    }

    let mut common_prefixes: Vec<String> = Vec::new();
    let mut content_keys: Vec<String> = Vec::new();
    if delimiter.is_empty() {
        content_keys = all_keys.clone();
    } else {
        for key in &all_keys {
            let suffix = &key[prefix.len()..];
            if let Some(pos) = suffix.find(&*delimiter) {
                let cp = format!("{}{}{}", prefix, &suffix[..pos], delimiter);
                if !common_prefixes.contains(&cp) {
                    common_prefixes.push(cp);
                }
            } else {
                content_keys.push(key.clone());
            }
        }
    }

    let truncated = content_keys.len() + common_prefixes.len() > max_keys;
    content_keys.truncate(max_keys.saturating_sub(common_prefixes.len()));
    let next_marker = if truncated {
        content_keys.last().cloned().unwrap_or_default()
    } else {
        String::new()
    };

    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Name>{name}</Name><Prefix>{prefix}</Prefix>\
<MaxKeys>{max_keys}</MaxKeys><IsTruncated>{truncated}</IsTruncated>",
        name = escape_xml(&bucket),
        prefix = escape_xml(&prefix),
        max_keys = max_keys,
        truncated = truncated,
    );

    if truncated && !next_marker.is_empty() {
        xml.push_str(&format!(
            "<NextMarker>{}</NextMarker>",
            escape_xml(&next_marker)
        ));
    }

    for key in &content_keys {
        if let Some(v) = store.get_object(&bucket, key) {
            xml.push_str(&format!(
                "<Contents>\
<Key>{key}</Key>\
<LastModified>{lm}</LastModified>\
<ETag>{etag}</ETag>\
<Size>{size}</Size>\
<StorageClass>STANDARD</StorageClass>\
</Contents>",
                key = escape_xml(key),
                lm = v.last_modified.format("%Y-%m-%dT%H:%M:%S.000Z"),
                etag = escape_xml(&v.etag),
                size = v.size,
            ));
        }
    }

    for cp in &common_prefixes {
        xml.push_str(&format!(
            "<CommonPrefixes><Prefix>{}</Prefix></CommonPrefixes>",
            escape_xml(cp)
        ));
    }

    xml.push_str("</ListBucketResult>");
    xml_ok(&xml)
}

// ---------------------------------------------------------------------------
// Multipart upload operations
// ---------------------------------------------------------------------------

fn handle_create_multipart_upload(store: &mut S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    let key = key_from_path(&ctx.path);

    if !store.bucket_exists(&bucket) {
        return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
    }

    let content_type = ctx
        .headers
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let metadata: HashMap<String, String> = ctx
        .headers
        .iter()
        .filter_map(|(k, v)| {
            k.strip_prefix("x-amz-meta-")
                .map(|mk| (mk.to_string(), v.clone()))
        })
        .collect();

    let upload_id = store.create_multipart_upload(&bucket, &key, content_type, metadata);

    xml_ok(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<InitiateMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Bucket>{bucket}</Bucket><Key>{key}</Key><UploadId>{upload_id}</UploadId>\
</InitiateMultipartUploadResult>",
        bucket = escape_xml(&bucket),
        key = escape_xml(&key),
        upload_id = escape_xml(&upload_id)
    ))
}

/// Async UploadPart — writes part body to filesystem via ObjectFileStore.
async fn handle_upload_part_async(
    store_bundle: &AccountRegionBundle<S3Store>,
    file_store: &ObjectFileStore,
    ctx: &RequestContext,
) -> DispatchResponse {
    let upload_id = match ctx.query_params.get("uploadId") {
        Some(id) => id.clone(),
        None => return s3_error("InvalidRequest", "uploadId required", 400),
    };
    let part_number: u32 = match ctx
        .query_params
        .get("partNumber")
        .and_then(|v| v.parse().ok())
    {
        Some(n) => n,
        None => return s3_error("InvalidRequest", "partNumber required", 400),
    };

    // Look up the multipart upload to get bucket/key (short-lived guard)
    let (bucket, key) = {
        let store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        match store.get_multipart_upload(&upload_id) {
            None => return s3_error("NoSuchUpload", "The specified upload does not exist", 404),
            Some(u) => (u.bucket.clone(), u.key.clone()),
        }
    };

    let data = ctx.raw_body.to_vec();
    let etag = format!("\"{}\"", hex::encode(md5::compute(&data).0));
    let size = data.len() as u64;

    // Write part to filesystem: use a part-specific version_id
    let part_version_id = format!("part-{}", part_number);
    let file_path = match file_store
        .write_object(
            &ctx.account_id,
            &ctx.region,
            &bucket,
            // Use a sub-path under the upload_id to keep parts separate
            &format!("__multipart/{upload_id}/{key}"),
            &part_version_id,
            &data,
        )
        .await
    {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "Failed to write upload part to filesystem");
            return s3_error("InternalError", "Failed to store part", 500);
        }
    };

    // Store part metadata in S3Store (short-lived guard)
    {
        let mut store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        store.upload_part_with_etag(
            &upload_id,
            part_number,
            ObjectDataRef::FileRef(file_path),
            etag.clone(),
            size,
        );
    }

    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::new()),
        content_type: String::new(),
        headers: vec![("ETag".to_string(), etag)],
    }
}

/// Async CompleteMultipartUpload — concatenates file-backed parts into
/// a single object file on disk, then stores the version in S3Store.
async fn handle_complete_multipart_upload_async(
    store_bundle: &AccountRegionBundle<S3Store>,
    file_store: &ObjectFileStore,
    ctx: &RequestContext,
) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    let key = key_from_path(&ctx.path);

    let upload_id = match ctx.query_params.get("uploadId") {
        Some(id) => id.clone(),
        None => return s3_error("InvalidRequest", "uploadId required", 400),
    };

    // Parse parts from body XML
    let body = std::str::from_utf8(&ctx.raw_body).unwrap_or("");
    let parts: Vec<(u32, String)> = {
        let mut result = Vec::new();
        let mut remaining = body;
        while let Some(start) = remaining.find("<Part>") {
            remaining = &remaining[start + 6..];
            let end = remaining.find("</Part>").unwrap_or(remaining.len());
            let part_xml = &remaining[..end];
            let pn: u32 = extract_xml_text(part_xml, "PartNumber")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let etag = extract_xml_text(part_xml, "ETag").unwrap_or_default();
            if pn > 0 {
                result.push((pn, etag));
            }
        }
        result
    };

    // Gather part file paths and metadata from the store (short-lived guard)
    let upload_info = {
        let store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        match store.get_multipart_upload(&upload_id) {
            None => return s3_error("NoSuchUpload", "The specified upload does not exist", 404),
            Some(u) => {
                let content_type = u.content_type.clone();
                let metadata = u.metadata.clone();
                let mut sorted_parts: Vec<u32> = parts.iter().map(|(n, _)| *n).collect();
                sorted_parts.sort_unstable();

                let mut part_paths: Vec<(u32, ObjectDataRef, u64)> = Vec::new();
                for pn in &sorted_parts {
                    if let Some(part) = u.parts.get(pn) {
                        part_paths.push((*pn, part.data.clone(), part.size));
                    }
                }
                (content_type, metadata, part_paths)
            }
        }
    };

    let (content_type, metadata, part_data) = upload_info;

    // Determine versioning
    let versioning_enabled = {
        let store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        store
            .get_bucket(&bucket)
            .map(|b| b.versioning.as_str() == "Enabled")
            .unwrap_or(false)
    };

    let version_id = if versioning_enabled {
        uuid::Uuid::new_v4().to_string()
    } else {
        "null".to_string()
    };

    // Concatenate parts to final file on disk (async I/O)
    // Read each part, compute combined MD5, write to final location
    let mut combined = Vec::new();
    for (_pn, data_ref, _size) in &part_data {
        match data_ref {
            ObjectDataRef::Inline(bytes) => {
                combined.extend_from_slice(bytes);
            }
            ObjectDataRef::FileRef(path) => {
                match tokio::fs::read(path).await {
                    Ok(bytes) => combined.extend_from_slice(&bytes),
                    Err(e) => {
                        warn!(error = %e, path = %path.display(), "Failed to read part file");
                        return s3_error("InternalError", "Failed to read part", 500);
                    }
                }
            }
        }
    }

    let etag = format!("\"{}\"", hex::encode(md5::compute(&combined).0));
    let size = combined.len() as u64;

    // Write combined data to final object file
    let file_path = match file_store
        .write_object(
            &ctx.account_id,
            &ctx.region,
            &bucket,
            &key,
            &version_id,
            &combined,
        )
        .await
    {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "Failed to write assembled object to filesystem");
            return s3_error("InternalError", "Failed to store object", 500);
        }
    };

    // Clean up part files
    for (_pn, data_ref, _size) in &part_data {
        if let ObjectDataRef::FileRef(path) = data_ref {
            let _ = tokio::fs::remove_file(path).await;
        }
    }

    // Build version and store in S3Store
    let version = crate::store::ObjectVersion {
        version_id: version_id.clone(),
        last_modified: chrono::Utc::now(),
        etag: etag.clone(),
        content_type: content_type.clone(),
        content_encoding: None,
        content_disposition: None,
        cache_control: None,
        size,
        metadata,
        acl: "private".to_string(),
        data: ObjectDataRef::FileRef(file_path),
        delete_marker: false,
    };

    {
        let mut store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        store.complete_multipart_upload_with_version(&upload_id, version);
    }

    let location = format!("http://localhost:4566/{bucket}/{key}");
    xml_ok(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<CompleteMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Location>{location}</Location>\
<Bucket>{bucket}</Bucket>\
<Key>{key}</Key>\
<ETag>{etag}</ETag>\
</CompleteMultipartUploadResult>",
        location = escape_xml(&location),
        bucket = escape_xml(&bucket),
        key = escape_xml(&key),
        etag = escape_xml(&etag)
    ))
}

fn handle_abort_multipart_upload(store: &mut S3Store, ctx: &RequestContext) -> DispatchResponse {
    let upload_id = match ctx.query_params.get("uploadId") {
        Some(id) => id.clone(),
        None => return s3_error("InvalidRequest", "uploadId required", 400),
    };

    if store.abort_multipart_upload(&upload_id) {
        empty_204()
    } else {
        s3_error("NoSuchUpload", "The specified upload does not exist", 404)
    }
}

fn handle_list_multipart_uploads(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };

    if !store.bucket_exists(&bucket) {
        return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
    }

    let uploads = store.list_multipart_uploads(&bucket);

    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListMultipartUploadsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Bucket>{}</Bucket><KeyMarker></KeyMarker><UploadIdMarker></UploadIdMarker>\
<IsTruncated>false</IsTruncated>",
        escape_xml(&bucket)
    );

    for u in uploads {
        xml.push_str(&format!(
            "<Upload>\
<Key>{key}</Key>\
<UploadId>{id}</UploadId>\
<Initiated>{initiated}</Initiated>\
</Upload>",
            key = escape_xml(&u.key),
            id = escape_xml(&u.upload_id),
            initiated = u.initiated.format("%Y-%m-%dT%H:%M:%S.000Z"),
        ));
    }

    xml.push_str("</ListMultipartUploadsResult>");
    xml_ok(&xml)
}

// ---------------------------------------------------------------------------
// ACL operations
// ---------------------------------------------------------------------------

fn default_acl_xml(owner: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<AccessControlPolicy xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Owner><ID>{owner}</ID><DisplayName>localstack</DisplayName></Owner>\
<AccessControlList>\
<Grant><Grantee xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"CanonicalUser\">\
<ID>{owner}</ID><DisplayName>localstack</DisplayName></Grantee>\
<Permission>FULL_CONTROL</Permission></Grant>\
</AccessControlList>\
</AccessControlPolicy>",
        owner = owner
    )
}

fn handle_get_bucket_acl(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    if !store.bucket_exists(&bucket) {
        return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
    }
    xml_ok(&default_acl_xml(&ctx.account_id))
}

fn handle_put_bucket_acl(store: &mut S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    if !store.bucket_exists(&bucket) {
        return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
    }
    let acl = ctx
        .headers
        .get("x-amz-acl")
        .cloned()
        .unwrap_or_else(|| "private".to_string());
    if let Some(b) = store.get_bucket_mut(&bucket) {
        b.acl = acl;
    }
    empty_200()
}

fn handle_get_object_acl(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    let key = key_from_path(&ctx.path);
    if !store.bucket_exists(&bucket) {
        return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
    }
    if store.get_object(&bucket, &key).is_none() {
        return s3_error("NoSuchKey", "The specified key does not exist", 404);
    }
    xml_ok(&default_acl_xml(&ctx.account_id))
}

fn handle_put_object_acl(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    let key = key_from_path(&ctx.path);
    if !store.bucket_exists(&bucket) {
        return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
    }
    if store.get_object(&bucket, &key).is_none() {
        return s3_error("NoSuchKey", "The specified key does not exist", 404);
    }
    // ACL is stored on the version; for simplicity we accept the request and return 200
    empty_200()
}

// ---------------------------------------------------------------------------
// Bucket policy
// ---------------------------------------------------------------------------

fn handle_get_bucket_policy(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    match store.get_bucket(&bucket) {
        None => s3_error("NoSuchBucket", "The specified bucket does not exist", 404),
        Some(b) => match &b.policy {
            None => s3_error(
                "NoSuchBucketPolicy",
                "The bucket policy does not exist",
                404,
            ),
            Some(policy) => DispatchResponse {
                status_code: 200,
                body: ResponseBody::Buffered(Bytes::from(policy.clone().into_bytes())),
                content_type: "application/json".to_string(),
                headers: Vec::new(),
            },
        },
    }
}

fn handle_put_bucket_policy(store: &mut S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    if let Some(b) = store.get_bucket_mut(&bucket) {
        let policy = String::from_utf8_lossy(&ctx.raw_body).to_string();
        b.policy = Some(policy);
        empty_204()
    } else {
        s3_error("NoSuchBucket", "The specified bucket does not exist", 404)
    }
}

fn handle_delete_bucket_policy(store: &mut S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    if let Some(b) = store.get_bucket_mut(&bucket) {
        b.policy = None;
        empty_204()
    } else {
        s3_error("NoSuchBucket", "The specified bucket does not exist", 404)
    }
}

// ---------------------------------------------------------------------------
// Versioning
// ---------------------------------------------------------------------------

fn handle_get_bucket_versioning(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    match store.get_bucket(&bucket) {
        None => s3_error("NoSuchBucket", "The specified bucket does not exist", 404),
        Some(b) => {
            let status_xml = if b.versioning.is_empty() {
                String::new()
            } else {
                format!("<Status>{}</Status>", b.versioning)
            };
            xml_ok(&format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<VersioningConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">{status_xml}</VersioningConfiguration>"
            ))
        }
    }
}

fn handle_put_bucket_versioning(store: &mut S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    let body = std::str::from_utf8(&ctx.raw_body).unwrap_or("");
    let status = extract_xml_text(body, "Status").unwrap_or_default();

    if let Some(b) = store.get_bucket_mut(&bucket) {
        b.versioning = status;
        empty_200()
    } else {
        s3_error("NoSuchBucket", "The specified bucket does not exist", 404)
    }
}

fn handle_list_object_versions(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };

    if !store.bucket_exists(&bucket) {
        return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
    }

    let prefix = ctx.query_params.get("prefix").cloned().unwrap_or_default();
    let mut objects = store.list_objects(&bucket);
    objects.retain(|o| o.key.starts_with(&prefix));
    objects.sort_by_key(|o| o.key.clone());

    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListVersionsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Name>{}</Name><Prefix>{}</Prefix><IsTruncated>false</IsTruncated>",
        escape_xml(&bucket),
        escape_xml(&prefix)
    );

    for obj in objects {
        for v in &obj.versions {
            let is_latest = obj
                .versions
                .first()
                .map(|fv| fv.version_id == v.version_id)
                .unwrap_or(false);
            if v.delete_marker {
                xml.push_str(&format!(
                    "<DeleteMarker>\
<Key>{key}</Key><VersionId>{vid}</VersionId>\
<IsLatest>{latest}</IsLatest>\
<LastModified>{lm}</LastModified>\
</DeleteMarker>",
                    key = escape_xml(&obj.key),
                    vid = escape_xml(&v.version_id),
                    latest = is_latest,
                    lm = v.last_modified.format("%Y-%m-%dT%H:%M:%S.000Z"),
                ));
            } else {
                xml.push_str(&format!(
                    "<Version>\
<Key>{key}</Key><VersionId>{vid}</VersionId>\
<IsLatest>{latest}</IsLatest>\
<LastModified>{lm}</LastModified>\
<ETag>{etag}</ETag><Size>{size}</Size>\
<StorageClass>STANDARD</StorageClass>\
</Version>",
                    key = escape_xml(&obj.key),
                    vid = escape_xml(&v.version_id),
                    latest = is_latest,
                    lm = v.last_modified.format("%Y-%m-%dT%H:%M:%S.000Z"),
                    etag = escape_xml(&v.etag),
                    size = v.size,
                ));
            }
        }
    }

    xml.push_str("</ListVersionsResult>");
    xml_ok(&xml)
}

// ---------------------------------------------------------------------------
// Pre-signed URL (validation only — actual serving is done by treating a
// request with X-Amz-Signature query param as a valid GetObject)
// ---------------------------------------------------------------------------

// Pre-signed URL handling — uses the async GetObject path directly.

// ---------------------------------------------------------------------------
// Notification configuration (stub)
// ---------------------------------------------------------------------------

fn handle_get_bucket_notification(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    if !store.bucket_exists(&bucket) {
        return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
    }
    xml_ok(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<NotificationConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"></NotificationConfiguration>",
    )
}

fn handle_put_bucket_notification(store: &mut S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    if !store.bucket_exists(&bucket) {
        return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
    }
    // Parse notifications from body (stub: store raw XML, emit events later)
    // For now, accept and return 200.
    let _ = ctx; // suppress unused warning
    empty_200()
}

// ---------------------------------------------------------------------------
// Utility helpers
// ---------------------------------------------------------------------------

/// Naive XML text extractor: finds first occurrence of <tag>text</tag>
fn extract_xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)?;
    Some(xml[start..start + end].trim().to_string())
}

fn parse_copy_source(source: &str) -> (String, String) {
    let s = source.trim_start_matches('/');
    let slash = s.find('/').unwrap_or(s.len());
    let bucket = s[..slash].to_string();
    let key = s[slash..].trim_start_matches('/').to_string();
    (bucket, key)
}

fn urlencoding_decode(s: &str) -> String {
    // Simple percent-decode for copy source
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex_str) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(byte) = u8::from_str_radix(hex_str, 16)
        {
            result.push(byte as char);
            i += 3;
            continue;
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

// ---------------------------------------------------------------------------
// ServiceProvider impl — operation routing
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for S3Provider {
    fn service_name(&self) -> &str {
        "s3"
    }

    async fn start(&self) -> Result<(), anyhow::Error> {
        let dir = self.s3_objects_dir.clone();
        let store = self.object_store
            .get_or_try_init(|| async { ObjectFileStore::new(dir).await.map_err(anyhow::Error::from) })
            .await?;
        debug!("S3 ObjectFileStore initialized at {:?}", self.s3_objects_dir);

        // Clean up any orphaned .tmp files from previous crashes.
        match store.cleanup_orphaned_temps().await {
            Ok(0) => {}
            Ok(n) => debug!("Cleaned up {} orphaned temp files in S3 object store", n),
            Err(e) => warn!("Failed to clean up orphaned temp files: {}", e),
        }

        Ok(())
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let op_start = std::time::Instant::now();
        debug!(
            service = "s3",
            operation = %ctx.operation,
            path = %ctx.path,
            method = %ctx.method,
            "S3 dispatch"
        );

        // Determine what operation this is. S3 uses rest-xml so there's no
        // X-Amz-Target — we derive the operation from method + path + query params.
        let op = derive_s3_operation(ctx);

        // For read operations we use get_or_create (RefMut derefs to &S3Store).
        // Mutations need a separate block to avoid borrow issues.
        let response = match op.as_str() {
            // ---- Bucket ops ----
            "ListBuckets" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_list_buckets(&store, ctx)
            }
            "CreateBucket" => {
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_create_bucket(&mut store, ctx)
            }
            "DeleteBucket" => {
                handle_delete_bucket_async(&self.store, self.file_store(), ctx).await
            }
            "HeadBucket" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_head_bucket(&store, ctx)
            }
            "GetBucketLocation" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_get_bucket_location(&store, ctx)
            }
            // ---- Object ops ----
            "PutObject" => {
                handle_put_object_async(&self.store, self.file_store(), ctx).await
            }
            "GetObject" => {
                handle_get_object_async(&self.store, ctx).await
            }
            "HeadObject" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_head_object(&store, ctx)
            }
            "DeleteObject" => {
                handle_delete_object_async(&self.store, ctx).await
            }
            "DeleteObjects" => {
                handle_delete_objects_async(&self.store, ctx).await
            }
            "CopyObject" => {
                handle_copy_object_async(&self.store, self.file_store(), ctx).await
            }
            // ---- Listing ----
            "ListObjectsV2" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_list_objects_v2(&store, ctx)
            }
            "ListObjects" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_list_objects(&store, ctx)
            }
            // ---- Multipart ----
            "CreateMultipartUpload" => {
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_create_multipart_upload(&mut store, ctx)
            }
            "UploadPart" => {
                handle_upload_part_async(&self.store, self.file_store(), ctx).await
            }
            "CompleteMultipartUpload" => {
                handle_complete_multipart_upload_async(&self.store, self.file_store(), ctx).await
            }
            "AbortMultipartUpload" => {
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_abort_multipart_upload(&mut store, ctx)
            }
            "ListMultipartUploads" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_list_multipart_uploads(&store, ctx)
            }
            // ---- ACL ----
            "GetBucketAcl" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_get_bucket_acl(&store, ctx)
            }
            "PutBucketAcl" => {
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_put_bucket_acl(&mut store, ctx)
            }
            "GetObjectAcl" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_get_object_acl(&store, ctx)
            }
            "PutObjectAcl" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_put_object_acl(&store, ctx)
            }
            // ---- Policy ----
            "GetBucketPolicy" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_get_bucket_policy(&store, ctx)
            }
            "PutBucketPolicy" => {
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_put_bucket_policy(&mut store, ctx)
            }
            "DeleteBucketPolicy" => {
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_delete_bucket_policy(&mut store, ctx)
            }
            // ---- Versioning ----
            "GetBucketVersioning" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_get_bucket_versioning(&store, ctx)
            }
            "PutBucketVersioning" => {
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_put_bucket_versioning(&mut store, ctx)
            }
            "ListObjectVersions" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_list_object_versions(&store, ctx)
            }
            // ---- Notifications ----
            "GetBucketNotificationConfiguration" => {
                let store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_get_bucket_notification(&store, ctx)
            }
            "PutBucketNotificationConfiguration" => {
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_put_bucket_notification(&mut store, ctx)
            }
            // ---- Pre-signed ----
            "PresignedGetObject" => {
                handle_get_object_async(&self.store, ctx).await
            }
            _ => {
                warn!(service = "s3", operation = %op, "S3 operation not implemented");
                return Err(DispatchError::NotImplemented(op));
            }
        };

        debug!(
            service = "s3",
            operation = %op,
            op_latency_us = op_start.elapsed().as_micros(),
            "S3 operation complete"
        );

        Ok(response)
    }
}

// ---------------------------------------------------------------------------
// Operation derivation from HTTP method + path + query params
// ---------------------------------------------------------------------------

fn derive_s3_operation(ctx: &RequestContext) -> String {
    let method = ctx.method.to_uppercase();
    let has_key = !key_from_path(&ctx.path).is_empty();
    let has_bucket = bucket_from_path(&ctx.path).is_some();

    // Query param presence flags
    let q = &ctx.query_params;
    let has_upload_id = q.contains_key("uploadId");
    let has_part_number = q.contains_key("partNumber");
    let has_uploads = q.contains_key("uploads");
    let has_delete = q.contains_key("delete");
    let has_location = q.contains_key("location");
    let has_acl = q.contains_key("acl");
    let has_policy = q.contains_key("policy");
    let has_versioning = q.contains_key("versioning");
    let has_versions = q.contains_key("versions");
    let has_notification = q.contains_key("notification");
    let has_list_type_2 = q.get("list-type").map(|v| v == "2").unwrap_or(false);
    let has_x_amz_sig = q.contains_key("X-Amz-Signature") || q.contains_key("x-amz-signature");
    let has_copy_source = ctx.headers.contains_key("x-amz-copy-source");

    match (method.as_str(), has_bucket, has_key) {
        ("GET", false, _) => "ListBuckets".to_string(),
        ("GET", true, false) => {
            if has_location {
                "GetBucketLocation".to_string()
            } else if has_acl {
                "GetBucketAcl".to_string()
            } else if has_policy {
                "GetBucketPolicy".to_string()
            } else if has_versioning {
                "GetBucketVersioning".to_string()
            } else if has_versions {
                "ListObjectVersions".to_string()
            } else if has_notification {
                "GetBucketNotificationConfiguration".to_string()
            } else if has_uploads {
                "ListMultipartUploads".to_string()
            } else if has_list_type_2 {
                "ListObjectsV2".to_string()
            } else {
                "ListObjects".to_string()
            }
        }
        ("GET", true, true) => {
            if has_acl {
                "GetObjectAcl".to_string()
            } else if has_x_amz_sig {
                "PresignedGetObject".to_string()
            } else {
                "GetObject".to_string()
            }
        }
        ("HEAD", true, false) => "HeadBucket".to_string(),
        ("HEAD", true, true) => "HeadObject".to_string(),
        ("PUT", true, false) => {
            if has_acl {
                "PutBucketAcl".to_string()
            } else if has_policy {
                "PutBucketPolicy".to_string()
            } else if has_versioning {
                "PutBucketVersioning".to_string()
            } else if has_notification {
                "PutBucketNotificationConfiguration".to_string()
            } else {
                "CreateBucket".to_string()
            }
        }
        ("PUT", true, true) => {
            if has_copy_source {
                "CopyObject".to_string()
            } else if has_upload_id && has_part_number {
                "UploadPart".to_string()
            } else if has_acl {
                "PutObjectAcl".to_string()
            } else {
                "PutObject".to_string()
            }
        }
        ("DELETE", true, false) => {
            if has_policy {
                "DeleteBucketPolicy".to_string()
            } else {
                "DeleteBucket".to_string()
            }
        }
        ("DELETE", true, true) => {
            if has_upload_id {
                "AbortMultipartUpload".to_string()
            } else {
                "DeleteObject".to_string()
            }
        }
        ("POST", true, false) => {
            if has_delete {
                "DeleteObjects".to_string()
            } else if has_uploads {
                "CreateMultipartUpload".to_string()
            } else {
                "DeleteObjects".to_string()
            }
        }
        ("POST", true, true) => {
            if has_upload_id {
                "CompleteMultipartUpload".to_string()
            } else if has_uploads {
                "CreateMultipartUpload".to_string()
            } else {
                "PostObject".to_string()
            }
        }
        _ => format!("Unknown({method})"),
    }
}
