use std::borrow::Cow;
use std::collections::VecDeque;
use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use bytes::Bytes;
use digest::Digest as _;
use openstack_service_framework::HashingReader;
use openstack_service_framework::traits::{
    CrossServiceDispatcher, DispatchError, DispatchResponse, RequestContext, ResponseBody,
    ServiceProvider,
};
use openstack_service_framework::xml::{url_encode, xml_escape};
use openstack_state::AccountRegionBundle;
use tokio_util::io::ReaderStream;
use tracing::{debug, warn};

use crate::object_store::{ObjectFileStore, ObjectLocation};
use crate::store::{ListPagedResult, ObjectDataRef, S3Store};

/// Returns the threshold (in bytes) below which objects are stored inline
/// in memory rather than written to disk. Objects at or below this size
/// use `ObjectDataRef::Inline`; larger objects are written to the filesystem.
///
/// The value is read once from the `S3_INLINE_OBJECT_THRESHOLD_BYTES`
/// environment variable on first call and cached for the process lifetime.
/// This same threshold is also used for file-backed GET buffering so PUT/GET
/// behavior stays aligned under all configurations.
/// If the variable is unset or unparseable the default is **1 MiB**, which
/// keeps tiny objects inline while pushing larger benchmark tiers through the
/// streaming path.
fn inline_object_threshold() -> u64 {
    static THRESHOLD: OnceLock<u64> = OnceLock::new();
    *THRESHOLD.get_or_init(|| {
        std::env::var("S3_INLINE_OBJECT_THRESHOLD_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1024 * 1024) // 1 MiB default
    })
}

const GET_OBJECT_STREAM_READ_BUF_SMALL: usize = 1024 * 1024;
const GET_OBJECT_STREAM_READ_BUF_LARGE: usize = 4 * 1024 * 1024;
const GET_OBJECT_STREAM_LARGE_CUTOFF: u64 = 50 * 1024 * 1024;

fn get_object_stream_read_buf(size: u64) -> usize {
    if size <= GET_OBJECT_STREAM_LARGE_CUTOFF {
        GET_OBJECT_STREAM_READ_BUF_SMALL
    } else {
        GET_OBJECT_STREAM_READ_BUF_LARGE
    }
}

/// A [`std::io::Read`] adapter that feeds every byte through a running MD5
/// accumulator.  Used inside `spawn_blocking` for the large-object PUT path
/// so that hashing and disk writes happen on a blocking thread rather than
/// a tokio worker.
struct Md5Read<R> {
    inner: R,
    hasher: md5::Md5,
}

impl<R: std::io::Read> Md5Read<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: md5::Md5::new(),
        }
    }

    fn finalize(self) -> md5::digest::Output<md5::Md5> {
        self.hasher.finalize()
    }
}

impl<R: std::io::Read> std::io::Read for Md5Read<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.hasher.update(&buf[..n]);
        }
        Ok(n)
    }
}

enum MultipartPartReader {
    Inline(std::io::Cursor<Bytes>),
    /// File not yet opened — opened lazily on first read.
    PendingFile(PathBuf),
    File(std::fs::File),
}

impl std::io::Read for MultipartPartReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Inline(reader) => std::io::Read::read(reader, buf),
            Self::PendingFile(path) => {
                let mut file = std::fs::File::open(path)?;
                let n = std::io::Read::read(&mut file, buf)?;
                *self = Self::File(file);
                Ok(n)
            }
            Self::File(reader) => std::io::Read::read(reader, buf),
        }
    }
}

struct MultipartRead {
    readers: VecDeque<MultipartPartReader>,
}

impl MultipartRead {
    fn from_data_refs(data_refs: Vec<ObjectDataRef>) -> io::Result<Self> {
        let mut readers = VecDeque::with_capacity(data_refs.len());
        for data_ref in data_refs {
            match data_ref {
                ObjectDataRef::Inline(bytes) => {
                    readers.push_back(MultipartPartReader::Inline(std::io::Cursor::new(bytes)));
                }
                ObjectDataRef::FileRef(path) => {
                    // Store the path; the file is opened lazily on first read
                    // to avoid holding open file descriptors for all parts
                    // before any data is consumed.
                    readers.push_back(MultipartPartReader::PendingFile(path));
                }
            }
        }
        Ok(Self { readers })
    }
}

impl std::io::Read for MultipartRead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while let Some(reader) = self.readers.front_mut() {
            let n = std::io::Read::read(reader, buf)?;
            if n > 0 {
                return Ok(n);
            }
            self.readers.pop_front();
        }
        Ok(0)
    }
}

pub struct S3Provider {
    store: Arc<AccountRegionBundle<S3Store>>,
    /// Path for S3 object file storage.
    s3_objects_dir: PathBuf,
    /// Filesystem object store, initialized in `start()`.
    object_store: tokio::sync::OnceCell<ObjectFileStore>,
    /// Optional cross-service dispatcher for emitting bucket notifications.
    dispatcher: Option<Arc<dyn CrossServiceDispatcher>>,
}

impl S3Provider {
    pub fn new(s3_objects_dir: impl Into<PathBuf>) -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
            s3_objects_dir: s3_objects_dir.into(),
            object_store: tokio::sync::OnceCell::new(),
            dispatcher: None,
        }
    }

    /// Construct with a cross-service dispatcher for bucket notification delivery.
    pub fn new_with_dispatcher(
        s3_objects_dir: impl Into<PathBuf>,
        dispatcher: Arc<dyn CrossServiceDispatcher>,
    ) -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
            s3_objects_dir: s3_objects_dir.into(),
            object_store: tokio::sync::OnceCell::new(),
            dispatcher: Some(dispatcher),
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

fn xml_ok(xml: String) -> DispatchResponse {
    DispatchResponse::ok_xml(xml)
}

// ---------------------------------------------------------------------------
// ETag helpers
// ---------------------------------------------------------------------------

/// Format a 16-byte MD5 digest as a quoted ETag string (`"<hex>"`).
///
/// Uses a single allocation of exactly 34 bytes (1 quote + 32 hex chars + 1
/// quote), avoiding the two-allocation chain of `hex::encode` + `format!`.
#[inline]
fn format_etag(digest: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(2 + digest.len() * 2);
    s.push('"');
    for b in digest {
        write!(s, "{b:02x}").unwrap();
    }
    s.push('"');
    s
}

fn xml_response(status: u16, xml: String) -> DispatchResponse {
    DispatchResponse {
        status_code: status,
        body: ResponseBody::Buffered(Bytes::from(xml.into_bytes())),
        content_type: Cow::Borrowed("application/xml"),
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

fn s3_bucket_error(code: &str, message: &str, bucket: &str, status: u16) -> DispatchResponse {
    xml_response(
        status,
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Error><Code>{code}</Code><Message>{message}</Message><RequestId>00000000-0000-0000-0000-000000000000</RequestId><BucketName>{}</BucketName></Error>",
            xml_escape(bucket)
        ),
    )
}

fn empty_200() -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::new()),
        content_type: Cow::Borrowed(""),
        headers: Vec::new(),
    }
}

fn empty_204() -> DispatchResponse {
    DispatchResponse {
        status_code: 204,
        body: ResponseBody::Buffered(Bytes::new()),
        content_type: Cow::Borrowed(""),
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

/// Returns `true` if the path contains a non-empty key segment (after the bucket).
#[inline]
fn path_has_key(path: &str) -> bool {
    let path = path.trim_start_matches('/');
    let slash = path.find('/').unwrap_or(path.len());
    !path[slash..].trim_start_matches('/').is_empty()
}

/// Returns `true` if the path contains a non-empty bucket segment.
#[inline]
fn path_has_bucket(path: &str) -> bool {
    let path = path.trim_start_matches('/');
    path.split('/')
        .next()
        .map(|s| !s.is_empty())
        .unwrap_or(false)
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
        content_type: Cow::Borrowed(""),
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
        let store = store_bundle.get(&ctx.account_id, &ctx.region);
        if !store.as_ref().is_some_and(|s| s.bucket_exists(&bucket)) {
            return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
        }
        if !store.as_ref().is_some_and(|s| {
            s.is_bucket_empty(&bucket) && !s.has_incomplete_multipart_uploads(&bucket)
        }) {
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
        write!(
            xml,
            "<Bucket><Name>{}</Name><CreationDate>{}</CreationDate></Bucket>",
            xml_escape(&b.name),
            b.creation_date.format("%Y-%m-%dT%H:%M:%S.000Z")
        )
        .unwrap();
    }
    xml.push_str("</Buckets></ListAllMyBucketsResult>");
    xml_ok(xml)
}

fn handle_get_bucket_location(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };

    let b = match store.get_bucket(&bucket) {
        Some(b) => b,
        None => {
            return s3_bucket_error(
                "NoSuchBucket",
                "The specified bucket does not exist",
                &bucket,
                404,
            );
        }
    };

    // us-east-1 is represented as empty string in the XML
    let location = if b.region == "us-east-1" {
        String::new()
    } else {
        b.region.clone()
    };

    xml_ok(format!(
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

    // Preflight store lookup: validate bucket existence and read versioning
    // state under a *read* lock so that concurrent GETs and other PUTs can
    // proceed in parallel during this validation phase.
    let versioning_enabled = {
        let store = match store_bundle.get(&ctx.account_id, &ctx.region) {
            Some(store) => store,
            None => return s3_error("NoSuchBucket", "The specified bucket does not exist", 404),
        };

        let bucket_state = match store.get_bucket(&bucket) {
            Some(b) => b,
            None => return s3_error("NoSuchBucket", "The specified bucket does not exist", 404),
        };
        bucket_state.versioning.as_str() == "Enabled"
    };

    let content_type = ctx
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let metadata: HashMap<String, String> = ctx
        .headers
        .iter()
        .filter_map(|(k, v)| {
            k.as_str()
                .strip_prefix("x-amz-meta-")
                .and_then(|mk| v.to_str().ok().map(|mv| (mk.to_string(), mv.to_string())))
        })
        .collect();

    // Generate version_id now so we can use it for the file path
    let version_id = if versioning_enabled {
        uuid::Uuid::new_v4().to_string()
    } else {
        "null".to_string()
    };

    // Get body data — prefer body_reader (stream-through, single disk write),
    // then spooled_body (streaming, no full copy in memory),
    // fall back to raw_body for unit-test contexts where both are None.
    let (etag, size, object_data) = if let Some(reader_mutex) = ctx.body_reader.as_ref() {
        // Stream-through path: the gateway bypassed SpooledBody so we receive
        // the raw network stream.  Read it directly into memory or disk,
        // computing the MD5 hash on the fly with HashingReader.
        let content_length: Option<u64> = ctx
            .headers
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());

        let mut reader_guard = reader_mutex.lock().await;
        let threshold = inline_object_threshold();

        if content_length.is_some_and(|len| len <= threshold) {
            // Small object: read entirely into memory.
            let cap = content_length.unwrap_or(64 * 1024) as usize;
            let mut hashing_reader = HashingReader::<md5::Md5, _>::new(&mut *reader_guard);
            let mut body_bytes = Vec::with_capacity(cap);
            if let Err(e) =
                tokio::io::AsyncReadExt::read_to_end(&mut hashing_reader, &mut body_bytes).await
            {
                warn!(error = %e, "Failed to read body_reader (small path)");
                return s3_error("InternalError", "Failed to read request body", 500);
            }
            let digest = hashing_reader.finalize();
            let etag = format_etag(digest.as_slice());
            let size = body_bytes.len() as u64;
            (etag, size, ObjectDataRef::Inline(Bytes::from(body_bytes)))
        } else {
            // Large object (or unknown size): stream directly to disk via the
            // async write path.
            //
            // We MUST NOT use block_in_place + SyncIoBridge here.  The
            // body_reader is backed by the live hyper HTTP/1.1 connection.
            // axum::serve runs each connection as a single tokio task that
            // drives both socket I/O (reading TCP frames, pushing body data
            // through an internal channel to the handler) and the request
            // handler itself.  block_in_place freezes the entire task — the
            // socket-I/O half stops running, so body data can never arrive,
            // and SyncIoBridge::read() (which calls Handle::block_on) parks
            // the OS thread forever: a permanent deadlock.
            //
            // The async path uses tokio::fs::File with an adaptive BufWriter
            // (up to 16 MiB), requiring only ~12 spawn_blocking dispatches
            // for a 100 MiB file — overhead is negligible vs. actual I/O.
            let mut hashing_reader = HashingReader::<md5::Md5, _>::new(&mut *reader_guard);
            let result = file_store
                .write_object_from_reader(
                    &ctx.account_id,
                    &ctx.region,
                    &bucket,
                    &key,
                    &version_id,
                    &mut hashing_reader,
                    content_length,
                )
                .await;
            let (file_path, bytes_written) = match result {
                Ok(r) => r,
                Err(e) => {
                    warn!(error = %e, "Failed to stream body_reader to filesystem");
                    return s3_error("InternalError", "Failed to store object", 500);
                }
            };
            let digest = hashing_reader.finalize();
            let etag = format_etag(digest.as_slice());
            (etag, bytes_written, ObjectDataRef::FileRef(file_path))
        }
    } else if let Some(mutex) = ctx.spooled_body.as_ref() {
        // Take the SpooledBody out of the Mutex so we can consume it into a reader.
        // The guard must be dropped before any .await, so use a block scope.
        let (spooled, spooled_len) = {
            let mut guard = mutex.lock().expect("spooled_body mutex poisoned");
            let spooled = std::mem::replace(
                &mut *guard,
                openstack_service_framework::SpooledBody::new(0),
            );
            let spooled_len = spooled.len() as u64;
            (spooled, spooled_len)
        };

        if spooled_len <= inline_object_threshold() {
            // Small object: read into memory, hash on the way.
            let mut hashing_reader = HashingReader::<md5::Md5, _>::new(spooled.into_reader());
            let mut body_bytes = Vec::with_capacity(spooled_len as usize);
            if let Err(e) =
                tokio::io::AsyncReadExt::read_to_end(&mut hashing_reader, &mut body_bytes).await
            {
                warn!(error = %e, "Failed to read spooled body");
                return s3_error("InternalError", "Failed to read request body", 500);
            }
            let digest = hashing_reader.finalize();
            let etag = format_etag(digest.as_slice());
            let size = body_bytes.len() as u64;
            (etag, size, ObjectDataRef::Inline(Bytes::from(body_bytes)))
        } else {
            // Large object: run sync I/O in spawn_blocking to avoid blocking
            // tokio worker threads during the copy loop.
            let file_store_clone = file_store.clone();
            let account_id_c = ctx.account_id.clone();
            let region_c = ctx.region.clone();
            let bucket_c = bucket.clone();
            let key_c = key.clone();
            let version_id_c = version_id.clone();

            let result = tokio::task::spawn_blocking(move || {
                let mut md5_reader = Md5Read::new(spooled);
                let (file_path, bytes_written) = file_store_clone.write_object_from_sync_reader(
                    &account_id_c,
                    &region_c,
                    &bucket_c,
                    &key_c,
                    &version_id_c,
                    &mut md5_reader,
                    Some(spooled_len),
                )?;
                let digest = md5_reader.finalize();
                let etag = format_etag(digest.as_slice());
                io::Result::Ok((etag, bytes_written, file_path))
            })
            .await;

            match result {
                Ok(Ok((etag, bytes_written, file_path))) => {
                    (etag, bytes_written, ObjectDataRef::FileRef(file_path))
                }
                Ok(Err(e)) => {
                    warn!(error = %e, "Failed to stream object to filesystem");
                    return s3_error("InternalError", "Failed to store object", 500);
                }
                Err(_) => {
                    return s3_error("InternalError", "Object write task panicked", 500);
                }
            }
        }
    } else {
        // Fallback for unit-test contexts where spooled_body is None.
        let body_bytes = ctx.raw_body_bytes().to_vec();
        let etag = format_etag(&md5::Md5::digest(&body_bytes));
        let size = body_bytes.len() as u64;
        let object_data = if size <= inline_object_threshold() {
            ObjectDataRef::Inline(Bytes::from(body_bytes))
        } else {
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
            ObjectDataRef::FileRef(file_path)
        };
        (etag, size, object_data)
    };

    let new_file_path = match &object_data {
        ObjectDataRef::FileRef(path) => Some(path.clone()),
        _ => None,
    };

    // Build version and store in S3Store (short-lived guard)
    let version = crate::store::ObjectVersion {
        version_id: Arc::from(version_id.as_str()),
        last_modified: chrono::Utc::now(),
        etag: Arc::from(etag.as_str()),
        content_type: Arc::from(content_type.as_str()),
        content_encoding: None,
        content_disposition: None,
        cache_control: None,
        size,
        metadata: Arc::new(metadata),
        acl: std::borrow::Cow::Borrowed("private"),
        data: object_data,
        delete_marker: false,
    };

    let prev = {
        let mut store = match store_bundle.get_mut(&ctx.account_id, &ctx.region) {
            Some(store) => store,
            None => {
                if let Some(path) = new_file_path.as_ref() {
                    let _ = tokio::fs::remove_file(path).await;
                }
                return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
            }
        };

        if !store.bucket_exists(&bucket) {
            drop(store);
            if let Some(path) = new_file_path.as_ref() {
                let _ = tokio::fs::remove_file(path).await;
            }
            return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
        }

        store.put_object_version(&bucket, &key, version)
    };

    let replaced_file_path = if version_id == "null" {
        prev.and_then(|old| match old {
            ObjectDataRef::FileRef(path) => {
                if new_file_path.as_deref() == Some(path.as_path()) {
                    None
                } else {
                    Some(path)
                }
            }
            _ => None,
        })
    } else {
        None
    };

    if let Some(path) = replaced_file_path {
        let _ = tokio::fs::remove_file(path).await;
    }

    let response_headers = {
        let mut headers = vec![("ETag".to_string(), etag)];
        if version_id != "null" {
            headers.push(("x-amz-version-id".to_string(), version_id));
        }
        headers
    };

    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::new()),
        content_type: Cow::Borrowed(""),
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
        let Some(store) = store_bundle.get(&ctx.account_id, &ctx.region) else {
            return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
        };

        if !store.bucket_exists(&bucket) {
            return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
        }

        let version_id = ctx.query_params.get("versionId").cloned();
        let version = if let Some(ref vid) = version_id {
            store.get_object_version(&bucket, &key, vid)
        } else {
            store.get_object(&bucket, &key)
        };

        version.map(|v| {
            (
                v.data.clone(),
                v.etag.clone(),
                v.last_modified,
                v.size,
                v.version_id.clone(),
                v.content_type.clone(),
                v.content_encoding.clone(),
                v.metadata.clone(),
            )
        })
    };

    match version_info {
        None => s3_error("NoSuchKey", "The specified key does not exist", 404),
        Some((
            data,
            etag,
            last_modified,
            size,
            version_id,
            content_type,
            content_encoding,
            metadata,
        )) => {
            let mut headers = vec![
                ("ETag".to_string(), String::from(&*etag)),
                (
                    "Last-Modified".to_string(),
                    last_modified
                        .format("%a, %d %b %Y %H:%M:%S GMT")
                        .to_string(),
                ),
                ("Content-Length".to_string(), size.to_string()),
            ];
            if version_id.as_ref() != "null" {
                headers.push(("x-amz-version-id".to_string(), version_id.to_string()));
            }
            for (mk, mv) in metadata.iter() {
                headers.push((format!("x-amz-meta-{mk}"), mv.clone()));
            }
            // Always emit Content-Encoding so the gateway's CompressionLayer
            // does not gzip binary object data.  For objects stored without a
            // custom content-encoding we emit "identity" (RFC 9110 §8.4.1),
            // which is a no-op encoding that explicitly tells the layer to
            // leave the bytes untouched.  This mirrors what the Streaming path
            // already does via the gateway builder.
            headers.push((
                "Content-Encoding".to_string(),
                content_encoding
                    .as_deref()
                    .unwrap_or("identity")
                    .to_string(),
            ));

            let body = match data {
                ObjectDataRef::Inline(bytes) => ResponseBody::Buffered(bytes),
                ObjectDataRef::FileRef(path) => {
                    // For small objects read the entire file into memory and
                    // return a buffered response — this avoids spawn_blocking
                    // overhead for tiny payloads.
                    if size <= inline_object_threshold() {
                        match tokio::fs::read(&path).await {
                            Ok(bytes) => ResponseBody::Buffered(Bytes::from(bytes)),
                            Err(e) => {
                                warn!(error = %e, path = %path.display(), "Failed to read object file");
                                return s3_error("InternalError", "Failed to read object", 500);
                            }
                        }
                    } else {
                        // Keep the existing 1 MiB buffer for smaller streamed
                        // objects and only grow reads once objects reach the
                        // large benchmark tiers.
                        match ObjectFileStore::read_object_at(&path).await {
                            Ok(file) => {
                                let stream = ReaderStream::with_capacity(
                                    file,
                                    get_object_stream_read_buf(size),
                                );
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
                }
            };

            DispatchResponse {
                status_code: 200,
                body,
                content_type: Cow::Owned(String::from(&*content_type)),
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
                ("ETag".to_string(), String::from(&*v.etag)),
                (
                    "Last-Modified".to_string(),
                    v.last_modified
                        .format("%a, %d %b %Y %H:%M:%S GMT")
                        .to_string(),
                ),
                ("Content-Length".to_string(), v.size.to_string()),
            ];
            if v.version_id.as_ref() != "null" {
                headers.push(("x-amz-version-id".to_string(), v.version_id.to_string()));
            }
            for (mk, mv) in v.metadata.iter() {
                headers.push((format!("x-amz-meta-{mk}"), mv.clone()));
            }
            DispatchResponse {
                status_code: 200,
                body: ResponseBody::Buffered(Bytes::new()),
                content_type: Cow::Owned(String::from(&*v.content_type)),
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
        let store = store_bundle.get(&ctx.account_id, &ctx.region);
        if !store.as_ref().is_some_and(|s| s.bucket_exists(&bucket)) {
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
        if let Some(removed_version) = &removed
            && let ObjectDataRef::FileRef(path) = &removed_version.data
        {
            let _ = tokio::fs::remove_file(path).await;
        }
        headers.push(("x-amz-version-id".to_string(), vid));
    } else {
        let deleted = {
            let mut store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
            store.delete_object(&bucket, &key)
        };
        if let Some(deleted_version) = &deleted {
            // If not versioned, the actual object was removed — clean up file
            if !deleted_version.delete_marker
                && let ObjectDataRef::FileRef(path) = &deleted_version.data
            {
                let _ = tokio::fs::remove_file(path).await;
            }
            if deleted_version.delete_marker {
                headers.push(("x-amz-delete-marker".to_string(), "true".to_string()));
                headers.push((
                    "x-amz-version-id".to_string(),
                    deleted_version.version_id.to_string(),
                ));
            }
        }
    }

    DispatchResponse {
        status_code: 204,
        body: ResponseBody::Buffered(Bytes::new()),
        content_type: Cow::Borrowed(""),
        headers,
    }
}

/// Async DeleteObjects (batch) — deletes backing files for removed objects.
async fn handle_delete_objects_async(
    store_bundle: Arc<AccountRegionBundle<S3Store>>,
    dispatcher: Option<Arc<dyn CrossServiceDispatcher>>,
    ctx: &RequestContext,
) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };

    {
        let store = store_bundle.get(&ctx.account_id, &ctx.region);
        if !store.as_ref().is_some_and(|s| s.bucket_exists(&bucket)) {
            return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
        }
    }

    // Parse the XML body for object keys
    let body = std::str::from_utf8(ctx.raw_body_bytes()).unwrap_or("");

    let keys: Vec<(String, Option<String>)> = {
        let mut result = Vec::new();
        let mut remaining = body;
        while let Some(obj_start) = remaining.find("<Object>") {
            remaining = &remaining[obj_start + 8..];
            let obj_end = remaining.find("</Object>").unwrap_or(remaining.len());
            let obj_xml = &remaining[..obj_end];
            let key = extract_xml_text(obj_xml, "Key").unwrap_or_default();
            let version_id = extract_xml_text(obj_xml, "VersionId")
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
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
                if let Some(removed) = store.delete_object_version(&bucket, key, vid)
                    && let ObjectDataRef::FileRef(path) = &removed.data
                {
                    files_to_delete.push(path.clone());
                }
                write!(
                    deleted_xml,
                    "<Deleted><Key>{}</Key><VersionId>{}</VersionId></Deleted>",
                    xml_escape(key),
                    xml_escape(vid)
                )
                .unwrap();
            } else {
                if let Some(removed) = store.delete_object(&bucket, key)
                    && !removed.delete_marker
                    && let ObjectDataRef::FileRef(path) = &removed.data
                {
                    files_to_delete.push(path.clone());
                }
                write!(
                    deleted_xml,
                    "<Deleted><Key>{}</Key></Deleted>",
                    xml_escape(key)
                )
                .unwrap();
            }
        }
    }

    // Clean up files in parallel (async I/O — no store guard held)
    if !files_to_delete.is_empty() {
        let mut cleanup_set = tokio::task::JoinSet::new();
        for path in files_to_delete {
            cleanup_set.spawn(async move { tokio::fs::remove_file(path).await });
        }
        while cleanup_set.join_next().await.is_some() {}
    }

    // Emit notifications for each deleted key
    let dispatcher = dispatcher.clone();
    let account_id = ctx.account_id.clone();
    let region = ctx.region.clone();
    let bucket_for_notify = bucket.clone();
    let keys_for_notify = keys.clone();
    let store_bundle_clone = Arc::clone(&store_bundle);
    tokio::spawn(async move {
        for (key, _version_id) in keys_for_notify {
            emit_s3_notification(
                store_bundle_clone.clone(),
                dispatcher.clone(),
                account_id.clone(),
                region.clone(),
                bucket_for_notify.clone(),
                key,
                "s3:ObjectRemoved:Delete",
            )
            .await;
        }
    });

    xml_ok(format!(
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

    let copy_source = match ctx
        .headers
        .get("x-amz-copy-source")
        .and_then(|v| v.to_str().ok())
    {
        Some(s) => s.to_string(),
        None => return s3_error("InvalidRequest", "Missing x-amz-copy-source header", 400),
    };

    let copy_source = urlencoding_decode(&copy_source);
    let (src_bucket, src_key) = parse_copy_source(&copy_source);

    // Read source object metadata (short-lived guard)
    let src_info = {
        let Some(store) = store_bundle.get(&ctx.account_id, &ctx.region) else {
            return s3_error("NoSuchBucket", "Destination bucket does not exist", 404);
        };

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
        let store = store_bundle.get(&ctx.account_id, &ctx.region);
        store.as_ref().is_some_and(|s| {
            s.get_bucket(&dest_bucket)
                .map(|b| b.versioning.as_str() == "Enabled")
                .unwrap_or(false)
        })
    };

    let dest_version_id = if versioning_enabled {
        uuid::Uuid::new_v4().to_string()
    } else {
        "null".to_string()
    };

    // Copy data — keep small objects inline; use filesystem for large ones.
    let dest_data = match &src_data {
        ObjectDataRef::Inline(bytes) => {
            // Source is already in memory — keep it inline for the destination
            // if it's within the threshold; otherwise write to disk.
            if src_size <= inline_object_threshold() {
                ObjectDataRef::Inline(bytes.clone())
            } else {
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
        }
        ObjectDataRef::FileRef(path) => {
            if src_size <= inline_object_threshold() {
                // Small file-backed object — read into memory and keep inline.
                match tokio::fs::read(path).await {
                    Ok(bytes) => ObjectDataRef::Inline(Bytes::from(bytes)),
                    Err(e) => {
                        warn!(error = %e, path = %path.display(), "Failed to read source object file for copy");
                        return s3_error("InternalError", "Failed to copy object", 500);
                    }
                }
            } else {
                match file_store
                    .copy_object(
                        ObjectLocation {
                            account_id: &ctx.account_id,
                            region: &ctx.region,
                            bucket: &src_bucket,
                            key: &src_key,
                            version_id: &src_version_id,
                        },
                        ObjectLocation {
                            account_id: &ctx.account_id,
                            region: &ctx.region,
                            bucket: &dest_bucket,
                            key: &dest_key,
                            version_id: &dest_version_id,
                        },
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
        }
    };

    let new_file_path = match &dest_data {
        ObjectDataRef::FileRef(path) => Some(path.clone()),
        _ => None,
    };

    // Build version and store
    let version = crate::store::ObjectVersion {
        version_id: Arc::from(dest_version_id.as_str()),
        last_modified: chrono::Utc::now(),
        etag: src_etag.clone(),
        content_type: ct,
        content_encoding: None,
        content_disposition: None,
        cache_control: None,
        size: src_size,
        metadata: meta,
        acl: std::borrow::Cow::Borrowed("private"),
        data: dest_data,
        delete_marker: false,
    };

    let (etag, last_modified, replaced_file_path) = {
        let mut store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        let prev = store.put_object_version(&dest_bucket, &dest_key, version);
        let replaced = if dest_version_id == "null" {
            prev.and_then(|old| match old {
                ObjectDataRef::FileRef(path) => {
                    if new_file_path.as_deref() == Some(path.as_path()) {
                        None
                    } else {
                        Some(path)
                    }
                }
                _ => None,
            })
        } else {
            None
        };

        let v = store.get_object(&dest_bucket, &dest_key);
        match v {
            Some(v) => (v.etag.clone(), v.last_modified, replaced),
            None => (src_etag, src_last_modified, replaced),
        }
    };

    if let Some(path) = replaced_file_path {
        let _ = tokio::fs::remove_file(path).await;
    }

    xml_ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<CopyObjectResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<LastModified>{}</LastModified><ETag>{}</ETag></CopyObjectResult>",
        last_modified.format("%Y-%m-%dT%H:%M:%S.000Z"),
        xml_escape(&etag)
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

    // Apply start_after / continuation_token (used as an exclusive lower bound).
    let skip_after = continuation_token.as_deref().or(start_after.as_deref());

    // Use range-based listing with server-side delimiter grouping.
    // list_objects_paged() stops after max_keys + 1 **distinct** outputs
    // (Contents + unique CommonPrefixes), so we never scan more raw keys
    // than needed — even for delimiter requests with many keys per prefix.
    let ListPagedResult {
        contents: mut content_items,
        common_prefixes: cp_map,
    } = store.list_objects_paged(&bucket, &prefix, skip_after, max_keys + 1, &delimiter);

    let truncated = content_items.len() + cp_map.len() > max_keys;

    // Truncate: common prefixes first (BTreeMap is already sorted), then
    // give the remaining MaxKeys budget to content items.
    let cp_count_kept = cp_map.len().min(max_keys);
    let remaining = max_keys.saturating_sub(cp_count_kept);
    content_items.truncate(remaining);

    // Collect the kept common prefix strings (BTreeMap keys are sorted).
    let cp_vec: Vec<&str> = cp_map
        .keys()
        .take(cp_count_kept)
        .map(String::as_str)
        .collect();

    let next_token: Option<String> = if truncated {
        // Prefer the last kept content key as cursor.  When the page was
        // filled entirely by common prefixes, use the last raw key seen
        // under the last common prefix — this is stored in cp_map so the
        // next page starts strictly after all keys under that prefix,
        // preventing duplicate prefix entries across pages.
        if let Some((k, ..)) = content_items.last() {
            Some(k.clone())
        } else if let Some(last_cp) = cp_vec.last().copied() {
            cp_map.get(last_cp).cloned()
        } else {
            None
        }
    } else {
        None
    };

    let key_count = content_items.len() + cp_vec.len();

    // Pre-size the XML buffer to avoid repeated reallocations.
    // Rough estimate: ~250 bytes per Content entry + ~80 bytes per CommonPrefix
    // + ~400 bytes for the fixed header/footer.
    let cap = 400 + content_items.len() * 250 + cp_vec.len() * 80;
    let mut xml = String::with_capacity(cap);
    write!(
        xml,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Name>{}</Name><Prefix>{}</Prefix><MaxKeys>{}</MaxKeys>\
<KeyCount>{}</KeyCount><IsTruncated>{}</IsTruncated>",
        xml_escape(&bucket),
        xml_escape(&prefix),
        max_keys,
        key_count,
        truncated
    )
    .unwrap();

    if let Some(ref t) = next_token {
        write!(
            xml,
            "<NextContinuationToken>{}</NextContinuationToken>",
            xml_escape(t)
        )
        .unwrap();
    }

    for (key, lm, etag, size) in &content_items {
        write!(
            xml,
            "<Contents>\
<Key>{key}</Key>\
<LastModified>{lm}</LastModified>\
<ETag>{etag}</ETag>\
<Size>{size}</Size>\
<StorageClass>STANDARD</StorageClass>\
</Contents>",
            key = xml_escape(key),
            lm = lm.format("%Y-%m-%dT%H:%M:%S.000Z"),
            etag = xml_escape(etag),
            size = size,
        )
        .unwrap();
    }

    for cp in &cp_vec {
        write!(
            xml,
            "<CommonPrefixes><Prefix>{}</Prefix></CommonPrefixes>",
            xml_escape(cp)
        )
        .unwrap();
    }

    xml.push_str("</ListBucketResult>");
    xml_ok(xml)
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

    // Single-pass: collect (key, last_modified, etag, size) for all matching objects.
    // list_objects() returns in sorted key order (BTreeMap), so no sort needed.
    type ObjMeta1 = (String, chrono::DateTime<chrono::Utc>, Arc<str>, u64);
    let mut all_items: Vec<ObjMeta1> = store
        .list_objects(&bucket)
        .into_iter()
        .filter_map(|obj| {
            let v = obj.current()?;
            if !obj.key.starts_with(&prefix) {
                return None;
            }
            Some((
                obj.key.clone(),
                v.last_modified,
                Arc::clone(&v.etag),
                v.size,
            ))
        })
        .collect();
    if !marker.is_empty() {
        all_items.retain(|(k, ..)| k.as_str() > marker.as_str());
    }

    let mut common_prefixes: BTreeSet<String> = BTreeSet::new();
    let mut content_items: Vec<ObjMeta1> = Vec::new();
    if delimiter.is_empty() {
        content_items = std::mem::take(&mut all_items);
    } else {
        for item in &all_items {
            let suffix = &item.0[prefix.len()..];
            if let Some(pos) = suffix.find(&*delimiter) {
                common_prefixes.insert(format!("{}{}{}", prefix, &suffix[..pos], delimiter));
            } else {
                content_items.push(item.clone());
            }
        }
    }

    let truncated = content_items.len() + common_prefixes.len() > max_keys;
    // Truncate common_prefixes first, then give remaining budget to content_items.
    let mut cp_vec1: Vec<String> = common_prefixes.into_iter().collect();
    if cp_vec1.len() > max_keys {
        cp_vec1.truncate(max_keys);
    }
    let remaining1 = max_keys.saturating_sub(cp_vec1.len());
    content_items.truncate(remaining1);
    let next_marker = if truncated {
        // Same cursor-safety logic as ListObjectsV2: do not use the synthetic
        // common-prefix string as a marker — use the last raw key that fell
        // under the last kept prefix so the next request skips past it cleanly.
        if let Some((k, ..)) = content_items.last() {
            k.clone()
        } else if let Some(last_cp) = cp_vec1.last() {
            all_items
                .iter()
                .rfind(|(k, ..)| k.starts_with(last_cp.as_str()))
                .map(|(k, ..)| k.clone())
                .unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Name>{name}</Name><Prefix>{prefix}</Prefix>\
<MaxKeys>{max_keys}</MaxKeys><IsTruncated>{truncated}</IsTruncated>",
        name = xml_escape(&bucket),
        prefix = xml_escape(&prefix),
        max_keys = max_keys,
        truncated = truncated,
    );

    if truncated && !next_marker.is_empty() {
        write!(xml, "<NextMarker>{}</NextMarker>", xml_escape(&next_marker)).unwrap();
    }

    for (key, lm, etag, size) in &content_items {
        write!(
            xml,
            "<Contents>\
<Key>{key}</Key>\
<LastModified>{lm}</LastModified>\
<ETag>{etag}</ETag>\
<Size>{size}</Size>\
<StorageClass>STANDARD</StorageClass>\
</Contents>",
            key = xml_escape(key),
            lm = lm.format("%Y-%m-%dT%H:%M:%S.000Z"),
            etag = xml_escape(etag),
            size = size,
        )
        .unwrap();
    }

    for cp in &cp_vec1 {
        write!(
            xml,
            "<CommonPrefixes><Prefix>{}</Prefix></CommonPrefixes>",
            xml_escape(cp)
        )
        .unwrap();
    }

    xml.push_str("</ListBucketResult>");
    xml_ok(xml)
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
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let metadata: HashMap<String, String> = ctx
        .headers
        .iter()
        .filter_map(|(k, v)| {
            k.as_str()
                .strip_prefix("x-amz-meta-")
                .and_then(|mk| v.to_str().ok().map(|mv| (mk.to_string(), mv.to_string())))
        })
        .collect();

    let upload_id = store.create_multipart_upload(&bucket, &key, content_type, metadata);

    xml_ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<InitiateMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Bucket>{bucket}</Bucket><Key>{key}</Key><UploadId>{upload_id}</UploadId>\
</InitiateMultipartUploadResult>",
        bucket = xml_escape(&bucket),
        key = xml_escape(&key),
        upload_id = xml_escape(&upload_id)
    ))
}

/// Async UploadPart — writes part body to filesystem via ObjectFileStore.
async fn handle_upload_part_async(
    store_bundle: &AccountRegionBundle<S3Store>,
    file_store: &ObjectFileStore,
    ctx: &RequestContext,
) -> DispatchResponse {
    let request_bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    let request_key = key_from_path(&ctx.path);

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
        let Some(store) = store_bundle.get(&ctx.account_id, &ctx.region) else {
            return s3_error("NoSuchUpload", "The specified upload does not exist", 404);
        };
        match store.get_multipart_upload(&upload_id) {
            None => return s3_error("NoSuchUpload", "The specified upload does not exist", 404),
            Some(u) => {
                if u.bucket != request_bucket || u.key != request_key {
                    return s3_error("NoSuchUpload", "The specified upload does not exist", 404);
                }
                (u.bucket.clone(), u.key.clone())
            }
        }
    };

    // Get part data — prefer body_reader (stream-through, single disk write),
    // then spooled_body (streaming), fall back to raw_body for tests.
    let (etag, size, part_data) = if let Some(reader_mutex) = ctx.body_reader.as_ref() {
        let content_length: Option<u64> = ctx
            .headers
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());

        let mut reader_guard = reader_mutex.lock().await;
        let threshold = inline_object_threshold();

        if content_length.is_some_and(|len| len <= threshold) {
            let cap = content_length.unwrap_or(64 * 1024) as usize;
            let mut hashing_reader = HashingReader::<md5::Md5, _>::new(&mut *reader_guard);
            let mut data = Vec::with_capacity(cap);
            if let Err(e) =
                tokio::io::AsyncReadExt::read_to_end(&mut hashing_reader, &mut data).await
            {
                warn!(error = %e, "Failed to read body_reader for upload part (small path)");
                return s3_error("InternalError", "Failed to read request body", 500);
            }
            let digest = hashing_reader.finalize();
            let etag = format_etag(digest.as_slice());
            let size = data.len() as u64;
            (etag, size, ObjectDataRef::Inline(Bytes::from(data)))
        } else {
            // Large part (or unknown size): stream directly to disk via the
            // async write path.  See the PutObject comment above for why
            // block_in_place + SyncIoBridge cannot be used with body_reader.
            let part_version_id = format!("part-{}", part_number);
            let mut hashing_reader = HashingReader::<md5::Md5, _>::new(&mut *reader_guard);
            let multipart_key = format!("__multipart/{upload_id}/{key}");
            let result = file_store
                .write_object_from_reader(
                    &ctx.account_id,
                    &ctx.region,
                    &bucket,
                    &multipart_key,
                    &part_version_id,
                    &mut hashing_reader,
                    content_length,
                )
                .await;
            let (file_path, bytes_written) = match result {
                Ok(r) => r,
                Err(e) => {
                    warn!(error = %e, "Failed to stream body_reader upload part to filesystem");
                    return s3_error("InternalError", "Failed to store part", 500);
                }
            };
            let digest = hashing_reader.finalize();
            let etag = format_etag(digest.as_slice());
            (etag, bytes_written, ObjectDataRef::FileRef(file_path))
        }
    } else if let Some(mutex) = ctx.spooled_body.as_ref() {
        // Take the SpooledBody out of the Mutex so we can consume it into a reader.
        // The guard must be dropped before any .await, so use a block scope.
        let (spooled, spooled_len) = {
            let mut guard = mutex.lock().expect("spooled_body mutex poisoned");
            let spooled = std::mem::replace(
                &mut *guard,
                openstack_service_framework::SpooledBody::new(0),
            );
            let spooled_len = spooled.len() as u64;
            (spooled, spooled_len)
        };

        if spooled_len <= inline_object_threshold() {
            let mut hashing_reader = HashingReader::<md5::Md5, _>::new(spooled.into_reader());
            let mut data = Vec::with_capacity(spooled_len as usize);
            if let Err(e) =
                tokio::io::AsyncReadExt::read_to_end(&mut hashing_reader, &mut data).await
            {
                warn!(error = %e, "Failed to read spooled body for upload part");
                return s3_error("InternalError", "Failed to read request body", 500);
            }
            let digest = hashing_reader.finalize();
            let etag = format_etag(digest.as_slice());
            let size = data.len() as u64;
            (etag, size, ObjectDataRef::Inline(Bytes::from(data)))
        } else {
            let part_version_id = format!("part-{}", part_number);
            // SpooledBody implements std::io::Read directly — no SyncIoBridge needed.
            // Use spawn_blocking so the sync write loop doesn't block a worker thread.
            let file_store_clone = file_store.clone();
            let account_id_c = ctx.account_id.clone();
            let region_c = ctx.region.clone();
            let bucket_c = bucket.clone();
            let multipart_key = format!("__multipart/{upload_id}/{key}");

            let result = tokio::task::spawn_blocking(move || {
                let mut md5_reader = Md5Read::new(spooled);
                let (file_path, bytes_written) = file_store_clone.write_object_from_sync_reader(
                    &account_id_c,
                    &region_c,
                    &bucket_c,
                    &multipart_key,
                    &part_version_id,
                    &mut md5_reader,
                    Some(spooled_len),
                )?;
                let digest = md5_reader.finalize();
                let etag = format_etag(digest.as_slice());
                io::Result::Ok((etag, bytes_written, file_path))
            })
            .await;

            match result {
                Ok(Ok((etag, bytes_written, file_path))) => {
                    (etag, bytes_written, ObjectDataRef::FileRef(file_path))
                }
                Ok(Err(e)) => {
                    warn!(error = %e, "Failed to stream upload part to filesystem");
                    return s3_error("InternalError", "Failed to store part", 500);
                }
                Err(_) => {
                    return s3_error("InternalError", "Upload part write task panicked", 500);
                }
            }
        }
    } else {
        // Fallback for unit-test contexts where spooled_body is None.
        let data = ctx.raw_body_bytes().to_vec();
        let etag = format_etag(&md5::Md5::digest(&data));
        let size = data.len() as u64;
        let part_data = if size <= inline_object_threshold() {
            ObjectDataRef::Inline(Bytes::from(data))
        } else {
            let part_version_id = format!("part-{}", part_number);
            let file_path = match file_store
                .write_object(
                    &ctx.account_id,
                    &ctx.region,
                    &bucket,
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
            ObjectDataRef::FileRef(file_path)
        };
        (etag, size, part_data)
    };

    // Store part metadata in S3Store (short-lived guard)
    {
        let mut store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        store.upload_part_with_etag(&upload_id, part_number, part_data, etag.clone(), size);
    }

    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::new()),
        content_type: Cow::Borrowed(""),
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
    let body = std::str::from_utf8(ctx.raw_body_bytes()).unwrap_or("");
    let parts: Vec<(u32, String)> = {
        let mut result = Vec::new();
        let mut remaining = body;
        while let Some(start) = remaining.find("<Part>") {
            remaining = &remaining[start + 6..];
            let end = remaining.find("</Part>").unwrap_or(remaining.len());
            let part_xml = &remaining[..end];
            let pn: u32 = extract_xml_text(part_xml, "PartNumber")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            let etag = extract_xml_text(part_xml, "ETag")
                .map(|v| v.trim().to_string())
                .unwrap_or_default();
            if pn > 0 {
                result.push((pn, etag));
            }
        }
        result
    };

    // Gather part file paths and metadata from the store (short-lived guard)
    let upload_info = {
        let Some(store) = store_bundle.get(&ctx.account_id, &ctx.region) else {
            return s3_error("NoSuchUpload", "The specified upload does not exist", 404);
        };
        match store.get_multipart_upload(&upload_id) {
            None => return s3_error("NoSuchUpload", "The specified upload does not exist", 404),
            Some(u) => {
                if u.bucket != bucket || u.key != key {
                    return s3_error("NoSuchUpload", "The specified upload does not exist", 404);
                }
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
        let store = store_bundle.get(&ctx.account_id, &ctx.region);
        store.as_ref().is_some_and(|s| {
            s.get_bucket(&bucket)
                .map(|b| b.versioning.as_str() == "Enabled")
                .unwrap_or(false)
        })
    };

    let version_id = if versioning_enabled {
        uuid::Uuid::new_v4().to_string()
    } else {
        "null".to_string()
    };

    let estimated_size: u64 = part_data.iter().map(|(_, _, size)| *size).sum();
    let part_count = part_data.len();

    let multipart_etag = {
        let mut concat = Vec::with_capacity(part_count * 16);
        for (pn, _, _) in &part_data {
            if let Some((_expected_pn, supplied_etag)) = parts.iter().find(|(n, _)| n == pn)
                && let Ok(bytes) = hex::decode(supplied_etag.trim_matches('"'))
                && bytes.len() == 16
            {
                concat.extend_from_slice(&bytes);
            }
        }
        if concat.len() == part_count * 16 {
            format!(
                "\"{}-{}\"",
                hex::encode(md5::Md5::digest(&concat)),
                part_count
            )
        } else {
            String::new()
        }
    };

    // For small assembled objects, keep the existing inline path.
    let assembled_data = if estimated_size <= inline_object_threshold() {
        let mut combined = Vec::with_capacity(estimated_size as usize);
        for (_pn, data_ref, _size) in &part_data {
            match data_ref {
                ObjectDataRef::Inline(bytes) => {
                    combined.extend_from_slice(bytes);
                }
                ObjectDataRef::FileRef(path) => match tokio::fs::read(path).await {
                    Ok(bytes) => combined.extend_from_slice(&bytes),
                    Err(e) => {
                        warn!(error = %e, path = %path.display(), "Failed to read part file");
                        return s3_error("InternalError", "Failed to read part", 500);
                    }
                },
            }
        }

        let etag = if multipart_etag.is_empty() {
            format_etag(&md5::Md5::digest(&combined))
        } else {
            multipart_etag.clone()
        };
        let size = combined.len() as u64;

        // Clean up any file-backed parts before going inline.
        for (_pn, data_ref, _size) in &part_data {
            if let ObjectDataRef::FileRef(path) = data_ref {
                let _ = tokio::fs::remove_file(path).await;
            }
        }

        (etag, size, ObjectDataRef::Inline(Bytes::from(combined)))
    } else {
        // Stream all parts directly to the final object file with bounded buffering.
        let parts_for_reader: Vec<ObjectDataRef> = part_data
            .iter()
            .map(|(_, data_ref, _)| data_ref.clone())
            .collect();

        let fs_clone = file_store.clone();
        let account_id = ctx.account_id.clone();
        let region = ctx.region.clone();
        let bucket_cloned = bucket.clone();
        let key_cloned = key.clone();
        let version_id_cloned = version_id.clone();

        let join = tokio::task::spawn_blocking(move || {
            let mut multipart_reader = MultipartRead::from_data_refs(parts_for_reader)?;
            let mut hashing_reader = Md5Read::new(&mut multipart_reader);
            let (path, bytes_written) = fs_clone.write_object_from_sync_reader(
                &account_id,
                &region,
                &bucket_cloned,
                &key_cloned,
                &version_id_cloned,
                &mut hashing_reader,
                Some(estimated_size),
            )?;
            let digest = hashing_reader.finalize();
            Ok::<(PathBuf, u64, md5::digest::Output<md5::Md5>), io::Error>((
                path,
                bytes_written,
                digest,
            ))
        })
        .await;

        let (file_path, size, digest) = match join {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => {
                warn!(error = %e, "Failed to stream assembled object to filesystem");
                return s3_error("InternalError", "Failed to store object", 500);
            }
            Err(e) => {
                warn!(error = %e, "Assemble object task panicked");
                return s3_error("InternalError", "Failed to store object", 500);
            }
        };

        // Clean up part files after successful assembly — spawn all removals
        // concurrently so that many parts are deleted in parallel rather than
        // one-at-a-time.
        let mut cleanup_set = tokio::task::JoinSet::new();
        for (_pn, data_ref, _size) in &part_data {
            if let ObjectDataRef::FileRef(path) = data_ref {
                let path = path.clone();
                cleanup_set.spawn(async move { tokio::fs::remove_file(path).await });
            }
        }
        while cleanup_set.join_next().await.is_some() {}

        let etag = if multipart_etag.is_empty() {
            format_etag(digest.as_slice())
        } else {
            multipart_etag
        };
        (etag, size, ObjectDataRef::FileRef(file_path))
    };

    let (etag, size, assembled_data) = assembled_data;

    let new_file_path = match &assembled_data {
        ObjectDataRef::FileRef(path) => Some(path.clone()),
        _ => None,
    };

    // Build version and store in S3Store
    let version = crate::store::ObjectVersion {
        version_id: Arc::from(version_id.as_str()),
        last_modified: chrono::Utc::now(),
        etag: Arc::from(etag.as_str()),
        content_type: Arc::from(content_type.as_str()),
        content_encoding: None,
        content_disposition: None,
        cache_control: None,
        size,
        metadata: Arc::new(metadata),
        acl: std::borrow::Cow::Borrowed("private"),
        data: assembled_data,
        delete_marker: false,
    };

    let replaced_file_path = {
        let mut store = store_bundle.get_or_create(&ctx.account_id, &ctx.region);
        let prev = store.complete_multipart_upload_with_version(&upload_id, version);
        if version_id == "null" {
            prev.and_then(|old| match old {
                ObjectDataRef::FileRef(path) => {
                    if new_file_path.as_deref() == Some(path.as_path()) {
                        None
                    } else {
                        Some(path)
                    }
                }
                _ => None,
            })
        } else {
            None
        }
    };

    if let Some(path) = replaced_file_path {
        let _ = tokio::fs::remove_file(path).await;
    }

    let location = format!("http://localhost:4566/{bucket}/{key}");
    xml_ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<CompleteMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Location>{location}</Location>\
<Bucket>{bucket}</Bucket>\
<Key>{key}</Key>\
<ETag>{etag}</ETag>\
</CompleteMultipartUploadResult>",
        location = xml_escape(&location),
        bucket = xml_escape(&bucket),
        key = xml_escape(&key),
        etag = xml_escape(&etag)
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
        xml_escape(&bucket)
    );

    for u in uploads {
        write!(
            xml,
            "<Upload>\
<Key>{key}</Key>\
<UploadId>{id}</UploadId>\
<Initiated>{initiated}</Initiated>\
</Upload>",
            key = xml_escape(&u.key),
            id = xml_escape(&u.upload_id),
            initiated = u.initiated.format("%Y-%m-%dT%H:%M:%S.000Z"),
        )
        .unwrap();
    }

    xml.push_str("</ListMultipartUploadsResult>");
    xml_ok(xml)
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
    xml_ok(default_acl_xml(&ctx.account_id))
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
        .and_then(|v| v.to_str().ok())
        .unwrap_or("private")
        .to_string();
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
    xml_ok(default_acl_xml(&ctx.account_id))
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
                content_type: Cow::Borrowed("application/json"),
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
        let policy = String::from_utf8_lossy(ctx.raw_body_bytes()).to_string();
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
            xml_ok(format!(
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
    let body = std::str::from_utf8(ctx.raw_body_bytes()).unwrap_or("");
    let status = extract_xml_text(body, "Status")
        .map(|v| v.trim().to_string())
        .unwrap_or_default();

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
    // list_objects() returns objects in sorted key order (BTreeMap).
    let objects: Vec<_> = store
        .list_objects(&bucket)
        .into_iter()
        .filter(|o| o.key.starts_with(&prefix))
        .collect();

    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListVersionsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Name>{}</Name><Prefix>{}</Prefix><IsTruncated>false</IsTruncated>",
        xml_escape(&bucket),
        xml_escape(&prefix)
    );

    for obj in objects {
        for v in &obj.versions {
            let is_latest = obj
                .versions
                .first()
                .map(|fv| fv.version_id == v.version_id)
                .unwrap_or(false);
            if v.delete_marker {
                write!(
                    xml,
                    "<DeleteMarker>\
<Key>{key}</Key><VersionId>{vid}</VersionId>\
<IsLatest>{latest}</IsLatest>\
<LastModified>{lm}</LastModified>\
</DeleteMarker>",
                    key = xml_escape(&obj.key),
                    vid = xml_escape(&v.version_id),
                    latest = is_latest,
                    lm = v.last_modified.format("%Y-%m-%dT%H:%M:%S.000Z"),
                )
                .unwrap();
            } else {
                write!(
                    xml,
                    "<Version>\
<Key>{key}</Key><VersionId>{vid}</VersionId>\
<IsLatest>{latest}</IsLatest>\
<LastModified>{lm}</LastModified>\
<ETag>{etag}</ETag><Size>{size}</Size>\
<StorageClass>STANDARD</StorageClass>\
</Version>",
                    key = xml_escape(&obj.key),
                    vid = xml_escape(&v.version_id),
                    latest = is_latest,
                    lm = v.last_modified.format("%Y-%m-%dT%H:%M:%S.000Z"),
                    etag = xml_escape(&v.etag),
                    size = v.size,
                )
                .unwrap();
            }
        }
    }

    xml.push_str("</ListVersionsResult>");
    xml_ok(xml)
}

// ---------------------------------------------------------------------------
// Pre-signed URL (validation only — actual serving is done by treating a
// request with X-Amz-Signature query param as a valid GetObject)
// ---------------------------------------------------------------------------

// Pre-signed URL handling — uses the async GetObject path directly.

// ---------------------------------------------------------------------------
// Notification configuration
// ---------------------------------------------------------------------------

fn handle_get_bucket_notification(store: &S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    let b = match store.get_bucket(&bucket) {
        Some(b) => b,
        None => return s3_error("NoSuchBucket", "The specified bucket does not exist", 404),
    };

    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<NotificationConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">",
    );

    for nc in &b.notifications {
        let (tag, arn_tag) = match nc.destination_type.as_str() {
            "sns" => ("TopicConfiguration", "Topic"),
            "lambda" => ("CloudFunctionConfiguration", "CloudFunction"),
            _ => ("QueueConfiguration", "Queue"), // default = sqs
        };
        write!(
            xml,
            "<{tag}><Id>{}</Id><{arn_tag}>{}</{arn_tag}>",
            xml_escape(&nc.id),
            xml_escape(&nc.destination_arn)
        )
        .unwrap();
        for ev in &nc.events {
            write!(xml, "<Event>{}</Event>", xml_escape(ev)).unwrap();
        }
        if nc.prefix_filter.is_some() || nc.suffix_filter.is_some() {
            xml.push_str("<Filter><S3Key>");
            if let Some(ref prefix) = nc.prefix_filter {
                write!(
                    xml,
                    "<FilterRule><Name>prefix</Name><Value>{}</Value></FilterRule>",
                    xml_escape(prefix)
                )
                .unwrap();
            }
            if let Some(ref suffix) = nc.suffix_filter {
                write!(
                    xml,
                    "<FilterRule><Name>suffix</Name><Value>{}</Value></FilterRule>",
                    xml_escape(suffix)
                )
                .unwrap();
            }
            xml.push_str("</S3Key></Filter>");
        }
        write!(xml, "</{tag}>").unwrap();
    }

    xml.push_str("</NotificationConfiguration>");
    xml_ok(xml)
}

fn handle_put_bucket_notification(store: &mut S3Store, ctx: &RequestContext) -> DispatchResponse {
    let bucket = match bucket_from_path(&ctx.path) {
        Some(b) => b,
        None => return s3_error("InvalidBucketName", "Bucket name is required", 400),
    };
    if !store.bucket_exists(&bucket) {
        return s3_error("NoSuchBucket", "The specified bucket does not exist", 404);
    }

    let body = match std::str::from_utf8(ctx.raw_body_bytes()) {
        Ok(s) => s,
        Err(_) => {
            return s3_error(
                "MalformedXML",
                "The XML you provided was not valid UTF-8",
                400,
            );
        }
    };
    let mut configs: Vec<crate::store::NotificationConfig> = Vec::new();

    // Parse QueueConfiguration entries (SQS)
    parse_notification_configs(body, "QueueConfiguration", "Queue", "sqs", &mut configs);
    // Parse TopicConfiguration entries (SNS)
    parse_notification_configs(body, "TopicConfiguration", "Topic", "sns", &mut configs);
    // Parse CloudFunctionConfiguration entries (Lambda)
    parse_notification_configs(
        body,
        "CloudFunctionConfiguration",
        "CloudFunction",
        "lambda",
        &mut configs,
    );

    // Check if body contains notification config content but parsing failed
    let body_has_configs = body.contains("QueueConfiguration")
        || body.contains("TopicConfiguration")
        || body.contains("CloudFunctionConfiguration");
    if body_has_configs && configs.is_empty() {
        return s3_error(
            "MalformedXML",
            "The XML you provided was ill-formed or did not validate against our published schema",
            400,
        );
    }

    if let Some(b) = store.get_bucket_mut(&bucket) {
        b.notifications = configs;
    }
    debug!(bucket = %bucket, "PutBucketNotificationConfiguration stored");
    empty_200()
}

/// Parse one notification config element type from the XML body.
fn parse_notification_configs(
    body: &str,
    outer_tag: &str,
    arn_tag: &str,
    dest_type: &str,
    out: &mut Vec<crate::store::NotificationConfig>,
) {
    let open = format!("<{outer_tag}>");
    let close = format!("</{outer_tag}>");
    let mut remaining = body;
    while let Some(start) = remaining.find(&open) {
        remaining = &remaining[start + open.len()..];
        let end = remaining.find(&close).unwrap_or(remaining.len());
        let block = &remaining[..end];

        let destination_arn = extract_xml_text(block, arn_tag).unwrap_or_default();
        let id = extract_xml_text(block, "Id").unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        // Collect all <Event> tags within this block
        let mut events = Vec::new();
        let mut ev_rem = block;
        while let Some(ev_start) = ev_rem.find("<Event>") {
            ev_rem = &ev_rem[ev_start + 7..];
            if let Some(ev_end) = ev_rem.find("</Event>") {
                events.push(ev_rem[..ev_end].trim().to_string());
                ev_rem = &ev_rem[ev_end..];
            }
        }

        // Parse filter rules
        let prefix_filter = {
            if let Some(filter_block) = extract_xml_text(block, "Filter") {
                // Look for Name=prefix, Value=...
                find_filter_rule(&filter_block, "prefix")
            } else {
                None
            }
        };
        let suffix_filter = {
            if let Some(filter_block) = extract_xml_text(block, "Filter") {
                find_filter_rule(&filter_block, "suffix")
            } else {
                None
            }
        };

        if !destination_arn.is_empty() {
            out.push(crate::store::NotificationConfig {
                id,
                destination_arn,
                destination_type: dest_type.to_string(),
                events,
                prefix_filter,
                suffix_filter,
            });
        }
        remaining = &remaining[end..];
    }
}

/// Find a <FilterRule> with matching <Name> and return its <Value>.
fn find_filter_rule(block: &str, name: &str) -> Option<String> {
    let mut rem = block;
    while let Some(start) = rem.find("<FilterRule>") {
        rem = &rem[start + 12..];
        let end = rem.find("</FilterRule>").unwrap_or(rem.len());
        let rule = &rem[..end];
        if let Some(rule_name) = extract_xml_text(rule, "Name")
            && rule_name.trim().eq_ignore_ascii_case(name)
        {
            return extract_xml_text(rule, "Value");
        }
        rem = &rem[end..];
    }
    None
}

/// Emit an S3 bucket notification event to all matching configurations.
/// This function accepts owned values so it can be spawned as a background task.
async fn emit_s3_notification(
    store_bundle: Arc<AccountRegionBundle<S3Store>>,
    dispatcher: Option<Arc<dyn CrossServiceDispatcher>>,
    account_id: String,
    region: String,
    bucket: String,
    key: String,
    event_name: &'static str,
) {
    let dispatcher = match dispatcher {
        Some(d) => d,
        None => return,
    };

    let configs: Vec<crate::store::NotificationConfig> = {
        let Some(store) = store_bundle.get(&account_id, &region) else {
            return;
        };
        let Some(b) = store.get_bucket(&bucket) else {
            return;
        };
        b.notifications.clone()
    };

    for nc in &configs {
        // Check event filter
        let event_matches = nc.events.iter().any(|e| {
            e == event_name || e == "s3:*" || {
                // wildcard prefix like "s3:ObjectCreated:*"
                e.ends_with('*') && event_name.starts_with(&e[..e.len() - 1])
            }
        });
        if !event_matches {
            continue;
        }

        // Check key prefix/suffix filter
        if let Some(ref prefix) = nc.prefix_filter
            && !key.starts_with(prefix.as_str())
        {
            continue;
        }
        if let Some(ref suffix) = nc.suffix_filter
            && !key.ends_with(suffix.as_str())
        {
            continue;
        }

        // Build the S3 event notification JSON payload.
        // AWS S3 Record.eventName strips the "s3:" prefix (e.g. "ObjectCreated:Put").
        let record_event_name = event_name
            .strip_prefix("s3:")
            .unwrap_or(event_name)
            .replace(":*", ":Put");

        let payload = serde_json::json!({
            "Records": [{
                "eventVersion": "2.1",
                "eventSource": "aws:s3",
                "awsRegion": region,
                "eventTime": chrono::Utc::now().to_rfc3339(),
                "eventName": record_event_name,
                "s3": {
                    "s3SchemaVersion": "1.0",
                    "bucket": {
                        "name": bucket,
                        "arn": format!("arn:aws:s3:::{bucket}"),
                    },
                    "object": {
                        "key": key,
                    }
                }
            }]
        });

        let payload_bytes = serde_json::to_vec(&payload).unwrap_or_default();

        // Build a synthetic RequestContext targeting the right service
        let dispatch_ctx = build_notification_dispatch_ctx_owned(
            &account_id,
            &region,
            &nc.destination_type,
            &nc.destination_arn,
            payload_bytes,
        );
        if let Err(e) = dispatcher.dispatch_to(&dispatch_ctx).await {
            warn!(err = %e, destination = %nc.destination_arn, event = %event_name, "S3 notification dispatch failed");
        }
    }
}

/// Build a RequestContext that routes to SQS SendMessage / SNS Publish / Lambda InvokeFunction.
/// Takes owned account_id and region strings so it can be used from spawned tasks.
fn build_notification_dispatch_ctx_owned(
    account_id: &str,
    region: &str,
    dest_type: &str,
    dest_arn: &str,
    payload: Vec<u8>,
) -> RequestContext {
    match dest_type {
        "sqs" => {
            // Extract queue name from ARN: arn:aws:sqs:region:account:queue-name
            let queue_name = dest_arn
                .split(':')
                .next_back()
                .unwrap_or("unknown")
                .to_string();
            let body = format!(
                "Action=SendMessage&QueueUrl=http%3A%2F%2Flocalhost%3A4566%2F{account_id}%2F{queue_name}&MessageBody={}",
                url_encode(std::str::from_utf8(&payload).unwrap_or(""))
            );
            let mut ctx = RequestContext::new("sqs", "SendMessage", region, account_id);
            ctx.method = "POST".to_string();
            ctx.path = format!("/{account_id}/{queue_name}");
            ctx.raw_body = Some(Bytes::from(body.into_bytes()));
            ctx
        }
        "sns" => {
            let body = format!(
                "Action=Publish&TopicArn={}&Message={}",
                url_encode(dest_arn),
                url_encode(std::str::from_utf8(&payload).unwrap_or(""))
            );
            let mut ctx = RequestContext::new("sns", "Publish", region, account_id);
            ctx.method = "POST".to_string();
            ctx.path = "/".to_string();
            ctx.raw_body = Some(Bytes::from(body.into_bytes()));
            ctx
        }
        "lambda" => {
            // Extract function name from ARN
            let fn_name = dest_arn
                .split(':')
                .next_back()
                .unwrap_or("unknown")
                .to_string();
            // Lambda dispatch uses "Invoke" (not "InvokeFunction") to match LambdaProvider routing
            let mut ctx = RequestContext::new("lambda", "Invoke", region, account_id);
            ctx.method = "POST".to_string();
            ctx.path = format!("/2015-03-31/functions/{fn_name}/invocations");
            ctx.raw_body = Some(Bytes::from(payload));
            ctx
        }
        _ => {
            let mut ctx = RequestContext::new(dest_type, "Notify", region, account_id);
            ctx.raw_body = Some(Bytes::from(payload));
            ctx
        }
    }
}

// ---------------------------------------------------------------------------
// Utility helpers
// ---------------------------------------------------------------------------

/// Naive XML text extractor: finds first occurrence of <tag>text</tag>
fn unescape_xml(s: &str) -> String {
    // Replace the five predefined XML entities.  Order matters: &amp; must be
    // last so we don't double-expand entities like `&amp;lt;`.
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

fn extract_xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)?;
    Some(unescape_xml(&xml[start..start + end]))
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

    /// S3 uses rest-xml — there is no X-Amz-Target, so derive the operation
    /// name from the HTTP method + path shape + query params instead.
    fn derive_operation(&self, ctx: &RequestContext) -> Option<&str> {
        // derive_s3_operation returns a Cow<'static, str>.
        // We can't return a reference into it from this fn with lifetime 'a,
        // so we compute a &'static str via the same match logic and return that.
        Some(derive_s3_operation_static(ctx))
    }

    async fn start(&self) -> Result<(), anyhow::Error> {
        let dir = self.s3_objects_dir.clone();
        let store = self
            .object_store
            .get_or_try_init(|| async {
                ObjectFileStore::new(dir).await.map_err(anyhow::Error::from)
            })
            .await?;
        debug!(
            "S3 ObjectFileStore initialized at {:?}",
            self.s3_objects_dir
        );

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
        // S3 uses rest-xml. Derive the operation from method + path + query params.
        let op = derive_s3_operation(ctx);

        debug!(
            service = "s3",
            operation = %op,
            path = %ctx.path,
            method = %ctx.method,
            "S3 dispatch"
        );

        // For read operations we use get() (read lock); mutations use get_or_create() (write lock).
        let response = match op.as_ref() {
            // ---- Bucket ops ----
            "ListBuckets" => {
                if let Some(store) = self.store.get(&ctx.account_id, &ctx.region) {
                    handle_list_buckets(&store, ctx)
                } else {
                    // No state yet for this account — return empty bucket list.
                    xml_ok(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ListAllMyBucketsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
<Owner><ID>000000000000</ID><DisplayName>localstack</DisplayName></Owner>\
<Buckets></Buckets></ListAllMyBucketsResult>"
                            .to_string(),
                    )
                }
            }
            "CreateBucket" => {
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_create_bucket(&mut store, ctx)
            }
            "DeleteBucket" => handle_delete_bucket_async(&self.store, self.file_store(), ctx).await,
            "HeadBucket" => {
                let Some(store) = self.store.get(&ctx.account_id, &ctx.region) else {
                    let bucket = bucket_from_path(&ctx.path).unwrap_or_default();
                    return Ok(s3_bucket_error(
                        "NoSuchBucket",
                        "The specified bucket does not exist",
                        &bucket,
                        404,
                    ));
                };
                handle_head_bucket(&store, ctx)
            }
            "GetBucketLocation" => {
                let Some(store) = self.store.get(&ctx.account_id, &ctx.region) else {
                    let bucket = bucket_from_path(&ctx.path).unwrap_or_default();
                    return Ok(s3_bucket_error(
                        "NoSuchBucket",
                        "The specified bucket does not exist",
                        &bucket,
                        404,
                    ));
                };
                handle_get_bucket_location(&store, ctx)
            }
            // ---- Object ops ----
            "PutObject" => {
                let resp = handle_put_object_async(&self.store, self.file_store(), ctx).await;
                if resp.status_code == 200 {
                    let store = Arc::clone(&self.store);
                    let dispatcher = self.dispatcher.clone();
                    let account_id = ctx.account_id.clone();
                    let region = ctx.region.clone();
                    let bucket = bucket_from_path(&ctx.path).unwrap_or_default();
                    let key = key_from_path(&ctx.path);
                    tokio::spawn(async move {
                        emit_s3_notification(
                            store,
                            dispatcher,
                            account_id,
                            region,
                            bucket,
                            key,
                            "s3:ObjectCreated:Put",
                        )
                        .await;
                    });
                }
                resp
            }
            "GetObject" => handle_get_object_async(&self.store, ctx).await,
            "HeadObject" => {
                let Some(store) = self.store.get(&ctx.account_id, &ctx.region) else {
                    return Ok(s3_error(
                        "NoSuchBucket",
                        "The specified bucket does not exist",
                        404,
                    ));
                };
                handle_head_object(&store, ctx)
            }
            "DeleteObject" => {
                let bucket = bucket_from_path(&ctx.path).unwrap_or_default();
                let key = key_from_path(&ctx.path);
                let resp = handle_delete_object_async(&self.store, ctx).await;
                if resp.status_code == 204 {
                    let store = Arc::clone(&self.store);
                    let dispatcher = self.dispatcher.clone();
                    let account_id = ctx.account_id.clone();
                    let region = ctx.region.clone();
                    tokio::spawn(async move {
                        emit_s3_notification(
                            store,
                            dispatcher,
                            account_id,
                            region,
                            bucket,
                            key,
                            "s3:ObjectRemoved:Delete",
                        )
                        .await;
                    });
                }
                resp
            }
            "DeleteObjects" => {
                // Notifications for batch deletes are emitted per-key inside the handler
                handle_delete_objects_async(Arc::clone(&self.store), self.dispatcher.clone(), ctx)
                    .await
            }
            "CopyObject" => {
                let resp = handle_copy_object_async(&self.store, self.file_store(), ctx).await;
                if resp.status_code == 200 {
                    let store = Arc::clone(&self.store);
                    let dispatcher = self.dispatcher.clone();
                    let account_id = ctx.account_id.clone();
                    let region = ctx.region.clone();
                    let bucket = bucket_from_path(&ctx.path).unwrap_or_default();
                    let key = key_from_path(&ctx.path);
                    tokio::spawn(async move {
                        emit_s3_notification(
                            store,
                            dispatcher,
                            account_id,
                            region,
                            bucket,
                            key,
                            "s3:ObjectCreated:Copy",
                        )
                        .await;
                    });
                }
                resp
            }
            // ---- Listing ----
            "ListObjectsV2" => {
                let Some(store) = self.store.get(&ctx.account_id, &ctx.region) else {
                    return Ok(s3_error(
                        "NoSuchBucket",
                        "The specified bucket does not exist",
                        404,
                    ));
                };
                handle_list_objects_v2(&store, ctx)
            }
            "ListObjects" => {
                let Some(store) = self.store.get(&ctx.account_id, &ctx.region) else {
                    return Ok(s3_error(
                        "NoSuchBucket",
                        "The specified bucket does not exist",
                        404,
                    ));
                };
                handle_list_objects(&store, ctx)
            }
            // ---- Multipart ----
            "CreateMultipartUpload" => {
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_create_multipart_upload(&mut store, ctx)
            }
            "UploadPart" => handle_upload_part_async(&self.store, self.file_store(), ctx).await,
            "CompleteMultipartUpload" => {
                let bucket = bucket_from_path(&ctx.path).unwrap_or_default();
                let key = key_from_path(&ctx.path);
                let resp =
                    handle_complete_multipart_upload_async(&self.store, self.file_store(), ctx)
                        .await;
                if resp.status_code == 200 {
                    let store = Arc::clone(&self.store);
                    let dispatcher = self.dispatcher.clone();
                    let account_id = ctx.account_id.clone();
                    let region = ctx.region.clone();
                    let bucket2 = bucket.clone();
                    let key2 = key.clone();
                    tokio::spawn(async move {
                        emit_s3_notification(
                            store,
                            dispatcher,
                            account_id,
                            region,
                            bucket2,
                            key2,
                            "s3:ObjectCreated:CompleteMultipartUpload",
                        )
                        .await;
                    });
                }
                resp
            }
            "AbortMultipartUpload" => {
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_abort_multipart_upload(&mut store, ctx)
            }
            "ListMultipartUploads" => {
                let Some(store) = self.store.get(&ctx.account_id, &ctx.region) else {
                    return Ok(s3_error(
                        "NoSuchBucket",
                        "The specified bucket does not exist",
                        404,
                    ));
                };
                handle_list_multipart_uploads(&store, ctx)
            }
            // ---- ACL ----
            "GetBucketAcl" => {
                let Some(store) = self.store.get(&ctx.account_id, &ctx.region) else {
                    return Ok(s3_error(
                        "NoSuchBucket",
                        "The specified bucket does not exist",
                        404,
                    ));
                };
                handle_get_bucket_acl(&store, ctx)
            }
            "PutBucketAcl" => {
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_put_bucket_acl(&mut store, ctx)
            }
            "GetObjectAcl" => {
                let Some(store) = self.store.get(&ctx.account_id, &ctx.region) else {
                    return Ok(s3_error(
                        "NoSuchBucket",
                        "The specified bucket does not exist",
                        404,
                    ));
                };
                handle_get_object_acl(&store, ctx)
            }
            "PutObjectAcl" => {
                let Some(store) = self.store.get(&ctx.account_id, &ctx.region) else {
                    return Ok(s3_error(
                        "NoSuchBucket",
                        "The specified bucket does not exist",
                        404,
                    ));
                };
                handle_put_object_acl(&store, ctx)
            }
            // ---- Policy ----
            "GetBucketPolicy" => {
                let Some(store) = self.store.get(&ctx.account_id, &ctx.region) else {
                    return Ok(s3_error(
                        "NoSuchBucket",
                        "The specified bucket does not exist",
                        404,
                    ));
                };
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
                let Some(store) = self.store.get(&ctx.account_id, &ctx.region) else {
                    return Ok(s3_error(
                        "NoSuchBucket",
                        "The specified bucket does not exist",
                        404,
                    ));
                };
                handle_get_bucket_versioning(&store, ctx)
            }
            "PutBucketVersioning" => {
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_put_bucket_versioning(&mut store, ctx)
            }
            "ListObjectVersions" => {
                let Some(store) = self.store.get(&ctx.account_id, &ctx.region) else {
                    return Ok(s3_error(
                        "NoSuchBucket",
                        "The specified bucket does not exist",
                        404,
                    ));
                };
                handle_list_object_versions(&store, ctx)
            }
            // ---- Notifications ----
            "GetBucketNotificationConfiguration" => {
                let Some(store) = self.store.get(&ctx.account_id, &ctx.region) else {
                    return Ok(s3_error(
                        "NoSuchBucket",
                        "The specified bucket does not exist",
                        404,
                    ));
                };
                handle_get_bucket_notification(&store, ctx)
            }
            "PutBucketNotificationConfiguration" => {
                let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);
                handle_put_bucket_notification(&mut store, ctx)
            }
            // ---- Pre-signed ----
            "PresignedGetObject" => handle_get_object_async(&self.store, ctx).await,
            _ => {
                warn!(service = "s3", operation = %op, "S3 operation not implemented");
                return Err(DispatchError::NotImplemented(op.into_owned()));
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

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut buckets = Vec::new();
        for entry in self.store.iter() {
            let key = entry.key();
            let account = key.account_id().to_string();
            let region = key.region().to_string();
            let store = entry.value();
            for (name, bucket) in &store.buckets {
                let object_count = store
                    .objects
                    .get(name)
                    .map(|objects| {
                        objects
                            .values()
                            .filter(|obj| obj.current().is_some_and(|v| !v.delete_marker))
                            .count()
                    })
                    .unwrap_or(0);
                let bucket_id = format!("{account}:{region}:{name}");
                buckets.push(json!({
                    "id": bucket_id,
                    "kind": "bucket",
                    "created_at": bucket.creation_date.to_rfc3339(),
                    "attributes": [
                        {"key": "name", "value": name.clone()},
                        {"key": "account", "value": account.clone()},
                        {"key": "region", "value": region.clone()},
                        {"key": "object_count", "value": object_count.to_string()},
                        {"key": "versioning", "value": bucket.versioning.clone()},
                    ]
                }));
            }
        }
        Some(json!({ "kind": "s3", "buckets": buckets }))
    }
}

// ---------------------------------------------------------------------------
// Operation derivation from HTTP method + path + query params
// ---------------------------------------------------------------------------

/// Same logic as `derive_s3_operation` but returns `&'static str` so callers
/// with a lifetime bound (e.g. `ServiceProvider::derive_operation`) can use it.
fn derive_s3_operation_static(ctx: &RequestContext) -> &'static str {
    match derive_s3_operation(ctx) {
        Cow::Borrowed(s) => s,
        // Fallback for unrecognized method/path/query combinations.
        Cow::Owned(_) => "Unknown",
    }
}

fn derive_s3_operation(ctx: &RequestContext) -> Cow<'static, str> {
    let method = ctx.method.as_str();
    let has_key = path_has_key(&ctx.path);
    let has_bucket = path_has_bucket(&ctx.path);

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

    match (method, has_bucket, has_key) {
        ("GET", false, _) => Cow::Borrowed("ListBuckets"),
        ("GET", true, false) => {
            if has_location {
                Cow::Borrowed("GetBucketLocation")
            } else if has_acl {
                Cow::Borrowed("GetBucketAcl")
            } else if has_policy {
                Cow::Borrowed("GetBucketPolicy")
            } else if has_versioning {
                Cow::Borrowed("GetBucketVersioning")
            } else if has_versions {
                Cow::Borrowed("ListObjectVersions")
            } else if has_notification {
                Cow::Borrowed("GetBucketNotificationConfiguration")
            } else if has_uploads {
                Cow::Borrowed("ListMultipartUploads")
            } else if has_list_type_2 {
                Cow::Borrowed("ListObjectsV2")
            } else {
                Cow::Borrowed("ListObjects")
            }
        }
        ("GET", true, true) => {
            if has_acl {
                Cow::Borrowed("GetObjectAcl")
            } else if has_x_amz_sig {
                Cow::Borrowed("PresignedGetObject")
            } else {
                Cow::Borrowed("GetObject")
            }
        }
        ("HEAD", true, false) => Cow::Borrowed("HeadBucket"),
        ("HEAD", true, true) => Cow::Borrowed("HeadObject"),
        ("PUT", true, false) => {
            if has_acl {
                Cow::Borrowed("PutBucketAcl")
            } else if has_policy {
                Cow::Borrowed("PutBucketPolicy")
            } else if has_versioning {
                Cow::Borrowed("PutBucketVersioning")
            } else if has_notification {
                Cow::Borrowed("PutBucketNotificationConfiguration")
            } else {
                Cow::Borrowed("CreateBucket")
            }
        }
        ("PUT", true, true) => {
            if has_copy_source {
                Cow::Borrowed("CopyObject")
            } else if has_upload_id && has_part_number {
                Cow::Borrowed("UploadPart")
            } else if has_acl {
                Cow::Borrowed("PutObjectAcl")
            } else {
                Cow::Borrowed("PutObject")
            }
        }
        ("DELETE", true, false) => {
            if has_policy {
                Cow::Borrowed("DeleteBucketPolicy")
            } else {
                Cow::Borrowed("DeleteBucket")
            }
        }
        ("DELETE", true, true) => {
            if has_upload_id {
                Cow::Borrowed("AbortMultipartUpload")
            } else {
                Cow::Borrowed("DeleteObject")
            }
        }
        ("POST", true, false) => {
            if has_delete {
                Cow::Borrowed("DeleteObjects")
            } else if has_uploads {
                Cow::Borrowed("CreateMultipartUpload")
            } else {
                Cow::Borrowed("PostObject")
            }
        }
        ("POST", true, true) => {
            if has_upload_id {
                Cow::Borrowed("CompleteMultipartUpload")
            } else if has_uploads {
                Cow::Borrowed("CreateMultipartUpload")
            } else {
                Cow::Borrowed("PostObject")
            }
        }
        _ => Cow::Owned(format!("Unknown({method})")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GET_OBJECT_STREAM_LARGE_CUTOFF, GET_OBJECT_STREAM_READ_BUF_LARGE,
        GET_OBJECT_STREAM_READ_BUF_SMALL, get_object_stream_read_buf,
    };

    #[test]
    fn get_object_stream_read_buf_keeps_existing_size_through_cutoff() {
        assert_eq!(
            get_object_stream_read_buf(GET_OBJECT_STREAM_LARGE_CUTOFF),
            GET_OBJECT_STREAM_READ_BUF_SMALL
        );
    }

    #[test]
    fn get_object_stream_read_buf_grows_above_cutoff() {
        assert_eq!(
            get_object_stream_read_buf(GET_OBJECT_STREAM_LARGE_CUTOFF + 1),
            GET_OBJECT_STREAM_READ_BUF_LARGE
        );
    }
}
