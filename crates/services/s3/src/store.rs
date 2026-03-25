use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use digest::Digest as _;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// ObjectDataRef — where object bytes live
// ---------------------------------------------------------------------------

/// Reference to the actual bytes of an S3 object or upload part.
///
/// `Inline` keeps the data in memory (small objects, delete markers).
/// `FileRef` points to a file on disk managed by [`ObjectFileStore`].
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectDataRef {
    /// Data stored inline in memory.  `Bytes` is reference-counted so
    /// clone is O(1) — no deep copy under the DashMap lock.
    Inline(Bytes),
    /// Data stored on disk at the given path.
    FileRef(PathBuf),
}

impl ObjectDataRef {
    /// Returns the inline data, if available.
    pub fn as_inline(&self) -> Option<&[u8]> {
        match self {
            ObjectDataRef::Inline(v) => Some(v),
            ObjectDataRef::FileRef(_) => None,
        }
    }

    /// Returns the file path, if this is a file-backed reference.
    pub fn as_file_ref(&self) -> Option<&PathBuf> {
        match self {
            ObjectDataRef::Inline(_) => None,
            ObjectDataRef::FileRef(p) => Some(p),
        }
    }

    /// Returns `true` if data is stored on disk.
    pub fn is_file_ref(&self) -> bool {
        matches!(self, ObjectDataRef::FileRef(_))
    }
}

impl Default for ObjectDataRef {
    fn default() -> Self {
        ObjectDataRef::Inline(Bytes::new())
    }
}

/// Custom serde: serialize as either a base64 string (Inline) or a
/// `{"file_ref": "path"}` object (FileRef).  Deserialization is
/// backward-compatible: a plain base64 string is decoded to Inline,
/// an object with `file_ref` is decoded to FileRef.
mod serde_object_data_ref {
    use std::path::PathBuf;

    use base64::{Engine, engine::general_purpose::STANDARD};
    use bytes::Bytes;
    use serde::Deserialize;
    use serde::de::{self, Deserializer};
    use serde::ser::Serializer;

    use super::ObjectDataRef;

    pub fn serialize<S: Serializer>(data: &ObjectDataRef, s: S) -> Result<S::Ok, S::Error> {
        match data {
            ObjectDataRef::Inline(bytes) => s.serialize_str(&STANDARD.encode(bytes)),
            ObjectDataRef::FileRef(path) => {
                use serde::Serialize;
                #[derive(Serialize)]
                struct Ref<'a> {
                    file_ref: &'a str,
                }
                Ref {
                    file_ref: path.to_str().unwrap_or(""),
                }
                .serialize(s)
            }
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<ObjectDataRef, D::Error> {
        // We accept either a string (base64-encoded inline data) or an
        // object with a "file_ref" field.
        let value = serde_json::Value::deserialize(d)?;
        match &value {
            serde_json::Value::String(b64) => {
                let bytes = STANDARD.decode(b64).map_err(de::Error::custom)?;
                Ok(ObjectDataRef::Inline(Bytes::from(bytes)))
            }
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::String(path)) = map.get("file_ref") {
                    Ok(ObjectDataRef::FileRef(PathBuf::from(path)))
                } else {
                    Err(de::Error::custom(
                        "expected object with 'file_ref' string field",
                    ))
                }
            }
            _ => Err(de::Error::custom(
                "expected base64 string or {file_ref: ...} object",
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Bucket
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    pub name: String,
    pub creation_date: DateTime<Utc>,
    pub region: String,
    /// Versioning state: "Enabled" | "Suspended" | "" (disabled)
    pub versioning: String,
    /// JSON-encoded bucket policy (None = no policy)
    pub policy: Option<String>,
    /// Canned ACL string (e.g. "private", "public-read")
    pub acl: String,
    /// Notification configuration
    pub notifications: Vec<NotificationConfig>,
}

impl Bucket {
    pub fn new(name: impl Into<String>, region: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            creation_date: Utc::now(),
            region: region.into(),
            versioning: String::new(),
            policy: None,
            acl: "private".to_string(),
            notifications: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Object version
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectVersion {
    /// version-id string; "null" when versioning is disabled
    pub version_id: String,
    pub last_modified: DateTime<Utc>,
    /// ETag stored as a shared string — clone is O(1) (atomic refcount).
    pub etag: Arc<str>,
    /// Content-Type stored as a shared string — clone is O(1).
    pub content_type: Arc<str>,
    pub content_encoding: Option<String>,
    pub content_disposition: Option<String>,
    pub cache_control: Option<String>,
    pub size: u64,
    /// User-defined metadata (x-amz-meta-* headers, stored without the prefix)
    pub metadata: HashMap<String, String>,
    /// ACL canned string
    pub acl: String,
    /// The actual object data (inline or file-backed)
    #[serde(with = "serde_object_data_ref")]
    pub data: ObjectDataRef,
    /// True when this is a delete marker
    pub delete_marker: bool,
}

impl ObjectVersion {
    /// Create a new object version from inline data.
    pub fn new(
        data: Bytes,
        content_type: impl Into<String>,
        metadata: HashMap<String, String>,
        versioning_enabled: bool,
    ) -> Self {
        let etag_str = format!("\"{}\"", hex::encode(md5_bytes(&data)));
        let size = data.len() as u64;
        let version_id = if versioning_enabled {
            Uuid::new_v4().to_string()
        } else {
            "null".to_string()
        };
        Self {
            version_id,
            last_modified: Utc::now(),
            etag: Arc::from(etag_str.as_str()),
            content_type: Arc::from(content_type.into().as_str()),
            content_encoding: None,
            content_disposition: None,
            cache_control: None,
            size,
            metadata,
            acl: "private".to_string(),
            data: ObjectDataRef::Inline(data),
            delete_marker: false,
        }
    }

    /// Create a new object version with a pre-computed ETag and
    /// file-backed data reference.
    pub fn new_with_file_ref(
        file_path: PathBuf,
        size: u64,
        etag: String,
        content_type: impl Into<String>,
        metadata: HashMap<String, String>,
        versioning_enabled: bool,
    ) -> Self {
        let version_id = if versioning_enabled {
            Uuid::new_v4().to_string()
        } else {
            "null".to_string()
        };
        Self {
            version_id,
            last_modified: Utc::now(),
            etag: Arc::from(etag.as_str()),
            content_type: Arc::from(content_type.into().as_str()),
            content_encoding: None,
            content_disposition: None,
            cache_control: None,
            size,
            metadata,
            acl: "private".to_string(),
            data: ObjectDataRef::FileRef(file_path),
            delete_marker: false,
        }
    }
}

fn md5_bytes(data: &[u8]) -> [u8; 16] {
    md5::Md5::digest(data).into()
}

// ---------------------------------------------------------------------------
// S3 Object (collection of versions)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Object {
    pub key: String,
    /// Versions ordered newest-first.  `versions[0]` is the current version.
    ///
    /// `Arc<ObjectVersion>` makes GET metadata extraction O(1) — cloning an
    /// `Arc` is a single atomic increment rather than a deep copy of all
    /// fields (etag string, content_type string, metadata HashMap, etc.).
    pub versions: Vec<Arc<ObjectVersion>>,
}

impl S3Object {
    pub fn new(key: impl Into<String>, version: ObjectVersion) -> Self {
        Self {
            key: key.into(),
            versions: vec![Arc::new(version)],
        }
    }

    /// Returns the current (latest) non-delete-marker version, if any.
    pub fn current(&self) -> Option<&ObjectVersion> {
        self.versions
            .first()
            .filter(|v| !v.delete_marker)
            .map(Arc::as_ref)
    }

    /// Returns an `Arc` to the current (latest) non-delete-marker version, if any.
    ///
    /// Prefer this over `current()` when you need to retain the version
    /// beyond the lifetime of the store guard — cloning an `Arc` is O(1).
    pub fn current_arc(&self) -> Option<Arc<ObjectVersion>> {
        self.versions.first().filter(|v| !v.delete_marker).cloned()
    }

    /// Returns the latest version regardless of delete-marker status.
    pub fn latest(&self) -> Option<&ObjectVersion> {
        self.versions.first().map(Arc::as_ref)
    }

    /// Returns an `Arc` to the latest version regardless of delete-marker status.
    pub fn latest_arc(&self) -> Option<Arc<ObjectVersion>> {
        self.versions.first().cloned()
    }
}

// ---------------------------------------------------------------------------
// Multipart upload
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultipartUpload {
    pub upload_id: String,
    pub bucket: String,
    pub key: String,
    pub initiated: DateTime<Utc>,
    pub content_type: String,
    pub metadata: HashMap<String, String>,
    /// Parts indexed by part number (1-based)
    pub parts: HashMap<u32, UploadPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadPart {
    pub part_number: u32,
    pub etag: String,
    pub size: u64,
    #[serde(with = "serde_object_data_ref")]
    pub data: ObjectDataRef,
    pub last_modified: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Notification config (stub)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub id: String,
    pub destination_arn: String,
    pub events: Vec<String>,
    pub prefix_filter: Option<String>,
    pub suffix_filter: Option<String>,
}

// ---------------------------------------------------------------------------
// S3Store  (per-account-region shard)
// ---------------------------------------------------------------------------

/// Per-account-region S3 data store.
///
/// `objects` uses a [`DashMap`] keyed by bucket name so that concurrent
/// PutObject / GetObject requests targeting **different buckets** within the
/// same account-region shard can proceed in parallel without contending on a
/// single write lock.  Each bucket's key→object mapping is a `BTreeMap`
/// (sorted iteration for `ListObjects`), guarded by the DashMap shard lock.
///
/// The outer `AccountRegionBundle<S3Store>` provides coarse account+region
/// sharding; `DashMap` provides fine-grained per-bucket sharding within that.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct S3Store {
    /// bucket_name → Bucket
    pub buckets: HashMap<String, Bucket>,
    /// bucket_name → key → S3Object  (BTreeMap keeps keys sorted for listing)
    ///
    /// DashMap provides per-bucket-shard locking so concurrent PUTs/GETs to
    /// different buckets don't contend.
    pub objects: DashMap<String, BTreeMap<String, S3Object>>,
    /// upload_id → MultipartUpload
    pub multipart_uploads: HashMap<String, MultipartUpload>,
}

impl S3Store {
    pub fn new() -> Self {
        Self::default()
    }

    // --- bucket helpers ----------------------------------------------------

    pub fn get_bucket(&self, name: &str) -> Option<&Bucket> {
        self.buckets.get(name)
    }

    pub fn get_bucket_mut(&mut self, name: &str) -> Option<&mut Bucket> {
        self.buckets.get_mut(name)
    }

    pub fn bucket_exists(&self, name: &str) -> bool {
        self.buckets.contains_key(name)
    }

    pub fn create_bucket(&mut self, name: impl Into<String>, region: impl Into<String>) -> &Bucket {
        let name = name.into();
        let bucket = Bucket::new(name.clone(), region);
        self.buckets.insert(name.clone(), bucket);
        self.objects.entry(name.clone()).or_default();
        self.buckets.get(&name).unwrap()
    }

    pub fn delete_bucket(&mut self, name: &str) -> bool {
        if self.buckets.remove(name).is_some() {
            self.objects.remove(name);
            true
        } else {
            false
        }
    }

    pub fn is_bucket_empty(&self, name: &str) -> bool {
        self.objects.get(name).map(|m| m.is_empty()).unwrap_or(true)
    }

    pub fn has_incomplete_multipart_uploads(&self, bucket: &str) -> bool {
        self.multipart_uploads
            .values()
            .any(|upload| upload.bucket == bucket)
    }

    // --- object helpers ----------------------------------------------------

    pub fn put_object(
        &mut self,
        bucket: &str,
        key: &str,
        data: ObjectDataRef,
        content_type: impl Into<String>,
        metadata: HashMap<String, String>,
    ) -> Option<ObjectDataRef> {
        let versioning = self
            .buckets
            .get(bucket)
            .map(|b| b.versioning.as_str() == "Enabled")
            .unwrap_or(false);

        let version = match &data {
            ObjectDataRef::Inline(bytes) => {
                ObjectVersion::new(bytes.clone(), content_type, metadata, versioning)
            }
            ObjectDataRef::FileRef(path) => {
                // For file-backed data we cannot compute etag here without
                // reading the file.  The caller is expected to provide a
                // pre-built ObjectVersion via `put_object_version` instead.
                // Fallback: empty etag, size 0 — but this path should not
                // be hit in practice for file-backed objects.
                ObjectVersion::new_with_file_ref(
                    path.clone(),
                    0,
                    String::new(),
                    content_type,
                    metadata,
                    versioning,
                )
            }
        };

        let mut objects = self.objects.entry(bucket.to_string()).or_default();

        if let Some(obj) = objects.get_mut(key) {
            let prev = obj.versions.first().map(|v| v.data.clone());
            if version.version_id == "null" {
                // Non-versioned bucket: overwrite current object in place.
                obj.versions.clear();
                obj.versions.push(Arc::new(version));
            } else {
                obj.versions.insert(0, Arc::new(version));
            }
            prev
        } else {
            objects.insert(key.to_string(), S3Object::new(key, version));
            None
        }
    }

    /// Insert a fully-constructed `ObjectVersion` into the store.
    ///
    /// This is the preferred path for file-backed objects where the
    /// caller has already computed the ETag and size.
    pub fn put_object_version(
        &mut self,
        bucket: &str,
        key: &str,
        version: ObjectVersion,
    ) -> Option<ObjectDataRef> {
        let mut objects = self.objects.entry(bucket.to_string()).or_default();
        if let Some(obj) = objects.get_mut(key) {
            let prev = obj.versions.first().map(|v| v.data.clone());
            if version.version_id == "null" {
                // Non-versioned bucket: overwrite current object in place.
                obj.versions.clear();
                obj.versions.push(Arc::new(version));
            } else {
                obj.versions.insert(0, Arc::new(version));
            }
            prev
        } else {
            objects.insert(key.to_string(), S3Object::new(key, version));
            None
        }
    }

    pub fn get_object(&self, bucket: &str, key: &str) -> Option<Arc<ObjectVersion>> {
        self.objects
            .get(bucket)
            .and_then(|objs| objs.get(key).and_then(|obj| obj.current_arc()))
    }

    pub fn get_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Option<Arc<ObjectVersion>> {
        self.objects.get(bucket).and_then(|objs| {
            objs.get(key).and_then(|obj| {
                obj.versions
                    .iter()
                    .find(|v| v.version_id == version_id)
                    .cloned()
            })
        })
    }

    pub fn delete_object(&mut self, bucket: &str, key: &str) -> Option<Arc<ObjectVersion>> {
        let versioning = self
            .buckets
            .get(bucket)
            .map(|b| b.versioning.as_str() == "Enabled")
            .unwrap_or(false);

        let mut objects = self.objects.get_mut(bucket)?;

        if versioning {
            // Insert a delete marker
            let marker = ObjectVersion {
                version_id: Uuid::new_v4().to_string(),
                last_modified: Utc::now(),
                etag: Arc::from(""),
                content_type: Arc::from(""),
                content_encoding: None,
                content_disposition: None,
                cache_control: None,
                size: 0,
                metadata: HashMap::new(),
                acl: String::new(),
                data: ObjectDataRef::Inline(Bytes::new()),
                delete_marker: true,
            };
            let marker = Arc::new(marker);
            let obj = objects.entry(key.to_string()).or_insert_with(|| S3Object {
                key: key.to_string(),
                versions: Vec::new(),
            });
            obj.versions.insert(0, Arc::clone(&marker));
            Some(marker)
        } else {
            objects
                .remove(key)
                .and_then(|obj| obj.versions.into_iter().next())
        }
    }

    pub fn delete_object_version(
        &mut self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Option<Arc<ObjectVersion>> {
        let mut objects = self.objects.get_mut(bucket)?;
        let obj = objects.get_mut(key)?;
        let pos = obj
            .versions
            .iter()
            .position(|v| v.version_id == version_id)?;
        let removed = obj.versions.remove(pos);
        if obj.versions.is_empty() {
            objects.remove(key);
        }
        Some(removed)
    }

    pub fn list_objects(&self, bucket: &str) -> Vec<S3Object> {
        self.objects
            .get(bucket)
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default()
    }

    // --- multipart helpers -------------------------------------------------

    pub fn create_multipart_upload(
        &mut self,
        bucket: impl Into<String>,
        key: impl Into<String>,
        content_type: impl Into<String>,
        metadata: HashMap<String, String>,
    ) -> String {
        let upload_id = Uuid::new_v4().to_string();
        let upload = MultipartUpload {
            upload_id: upload_id.clone(),
            bucket: bucket.into(),
            key: key.into(),
            initiated: Utc::now(),
            content_type: content_type.into(),
            metadata,
            parts: HashMap::new(),
        };
        self.multipart_uploads.insert(upload_id.clone(), upload);
        upload_id
    }

    pub fn upload_part(
        &mut self,
        upload_id: &str,
        part_number: u32,
        data: ObjectDataRef,
    ) -> Option<String> {
        let upload = self.multipart_uploads.get_mut(upload_id)?;
        let (etag, size) = match &data {
            ObjectDataRef::Inline(bytes) => {
                let etag = format!("\"{}\"", hex::encode(md5_bytes(bytes)));
                let size = bytes.len() as u64;
                (etag, size)
            }
            ObjectDataRef::FileRef(_) => {
                // For file-backed parts the caller should use
                // `upload_part_with_etag` instead.
                (String::new(), 0)
            }
        };
        let part = UploadPart {
            part_number,
            etag: etag.clone(),
            size,
            data,
            last_modified: Utc::now(),
        };
        upload.parts.insert(part_number, part);
        Some(etag)
    }

    /// Upload a part with a pre-computed ETag and size (for file-backed parts).
    pub fn upload_part_with_etag(
        &mut self,
        upload_id: &str,
        part_number: u32,
        data: ObjectDataRef,
        etag: String,
        size: u64,
    ) -> Option<String> {
        let upload = self.multipart_uploads.get_mut(upload_id)?;
        let part = UploadPart {
            part_number,
            etag: etag.clone(),
            size,
            data,
            last_modified: Utc::now(),
        };
        upload.parts.insert(part_number, part);
        Some(etag)
    }

    /// Complete a multipart upload by concatenating inline parts.
    ///
    /// For file-backed parts, the caller should use
    /// `complete_multipart_upload_with_version` instead, providing a
    /// pre-assembled `ObjectVersion` that references the concatenated
    /// file.
    pub fn complete_multipart_upload(
        &mut self,
        upload_id: &str,
        parts: &[(u32, String)], // (part_number, etag)
    ) -> Option<Arc<ObjectVersion>> {
        let upload = self.multipart_uploads.remove(upload_id)?;

        // Concatenate parts in order (inline only)
        let mut combined = Vec::new();
        let mut sorted_parts: Vec<u32> = parts.iter().map(|(n, _)| *n).collect();
        sorted_parts.sort_unstable();
        for part_num in &sorted_parts {
            if let Some(part) = upload.parts.get(part_num)
                && let ObjectDataRef::Inline(bytes) = &part.data
            {
                combined.extend_from_slice(bytes);
                // File-backed parts are skipped here — caller should
                // use the file-aware path.
            }
        }

        let versioning = self
            .buckets
            .get(&upload.bucket)
            .map(|b| b.versioning.as_str() == "Enabled")
            .unwrap_or(false);

        let version = ObjectVersion::new(
            Bytes::from(combined),
            upload.content_type.clone(),
            upload.metadata.clone(),
            versioning,
        );

        let multipart_etag = {
            let mut concat = Vec::with_capacity(sorted_parts.len() * 16);
            let mut count = 0usize;
            for part_num in &sorted_parts {
                if let Some(part) = upload.parts.get(part_num)
                    && let Ok(bytes) = hex::decode(part.etag.trim_matches('"'))
                    && bytes.len() == 16
                {
                    concat.extend_from_slice(&bytes);
                    count += 1;
                }
            }
            if count > 0 {
                Some(format!("\"{}-{}\"", hex::encode(md5_bytes(&concat)), count))
            } else {
                None
            }
        };

        let mut version = version;
        if let Some(etag) = multipart_etag {
            version.etag = Arc::from(etag.as_str());
        }

        let mut objects = self.objects.entry(upload.bucket.clone()).or_default();
        if let Some(obj) = objects.get_mut(&upload.key) {
            if version.version_id == "null" {
                obj.versions.clear();
                obj.versions.push(Arc::new(version));
            } else {
                obj.versions.insert(0, Arc::new(version));
            }
            obj.versions.first().cloned()
        } else {
            let s3obj = S3Object::new(upload.key.clone(), version);
            let ret = s3obj.versions.first().cloned();
            objects.insert(upload.key.clone(), s3obj);
            ret
        }
    }

    /// Complete a multipart upload with a pre-assembled `ObjectVersion`.
    ///
    /// Used when parts are file-backed and the caller has already
    /// concatenated them on disk.
    ///
    /// Returns the previous current version's data ref when an existing key
    /// is overwritten; otherwise returns `None`.
    pub fn complete_multipart_upload_with_version(
        &mut self,
        upload_id: &str,
        version: ObjectVersion,
    ) -> Option<ObjectDataRef> {
        let upload = self.multipart_uploads.remove(upload_id)?;

        let mut objects = self.objects.entry(upload.bucket.clone()).or_default();
        if let Some(obj) = objects.get_mut(&upload.key) {
            let prev = obj.versions.first().map(|v| v.data.clone());
            if version.version_id == "null" {
                obj.versions.clear();
                obj.versions.push(Arc::new(version));
            } else {
                obj.versions.insert(0, Arc::new(version));
            }
            prev
        } else {
            objects.insert(
                upload.key.clone(),
                S3Object::new(upload.key.clone(), version),
            );
            None
        }
    }

    /// Get the `MultipartUpload` metadata for a given upload_id.
    pub fn get_multipart_upload(&self, upload_id: &str) -> Option<&MultipartUpload> {
        self.multipart_uploads.get(upload_id)
    }

    pub fn abort_multipart_upload(&mut self, upload_id: &str) -> bool {
        self.multipart_uploads.remove(upload_id).is_some()
    }

    pub fn list_multipart_uploads(&self, bucket: &str) -> Vec<&MultipartUpload> {
        self.multipart_uploads
            .values()
            .filter(|u| u.bucket == bucket)
            .collect()
    }
}

// The old `serde_bytes_base64` module has been replaced by
// `serde_object_data_ref` which supports both inline (base64) and
// file-ref serialization.
