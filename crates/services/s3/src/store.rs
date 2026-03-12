use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
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
    /// Data stored inline in memory.
    Inline(Vec<u8>),
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
        ObjectDataRef::Inline(Vec::new())
    }
}

/// Custom serde: serialize as either a base64 string (Inline) or a
/// `{"file_ref": "path"}` object (FileRef).  Deserialization is
/// backward-compatible: a plain base64 string is decoded to Inline,
/// an object with `file_ref` is decoded to FileRef.
mod serde_object_data_ref {
    use std::path::PathBuf;

    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde::de::{self, Deserializer};
    use serde::ser::Serializer;
    use serde::Deserialize;

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
                Ok(ObjectDataRef::Inline(bytes))
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
    pub etag: String,
    pub content_type: String,
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
        data: Vec<u8>,
        content_type: impl Into<String>,
        metadata: HashMap<String, String>,
        versioning_enabled: bool,
    ) -> Self {
        let etag = format!("\"{}\"", hex::encode(md5_bytes(&data)));
        let size = data.len() as u64;
        let version_id = if versioning_enabled {
            Uuid::new_v4().to_string()
        } else {
            "null".to_string()
        };
        Self {
            version_id,
            last_modified: Utc::now(),
            etag,
            content_type: content_type.into(),
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
            etag,
            content_type: content_type.into(),
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
    md5::compute(data).0
}

// ---------------------------------------------------------------------------
// S3 Object (collection of versions)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Object {
    pub key: String,
    /// Versions ordered newest-first. versions[0] is the current version.
    pub versions: Vec<ObjectVersion>,
}

impl S3Object {
    pub fn new(key: impl Into<String>, version: ObjectVersion) -> Self {
        Self {
            key: key.into(),
            versions: vec![version],
        }
    }

    /// Returns the current (latest) non-delete-marker version, if any.
    pub fn current(&self) -> Option<&ObjectVersion> {
        self.versions.first().filter(|v| !v.delete_marker)
    }

    /// Returns the latest version regardless of delete-marker status.
    pub fn latest(&self) -> Option<&ObjectVersion> {
        self.versions.first()
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

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct S3Store {
    /// bucket_name → Bucket
    pub buckets: HashMap<String, Bucket>,
    /// bucket_name → key → S3Object
    pub objects: HashMap<String, HashMap<String, S3Object>>,
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

    // --- object helpers ----------------------------------------------------

    pub fn put_object(
        &mut self,
        bucket: &str,
        key: &str,
        data: ObjectDataRef,
        content_type: impl Into<String>,
        metadata: HashMap<String, String>,
    ) -> Option<ObjectVersion> {
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

        let objects = self.objects.entry(bucket.to_string()).or_default();

        if let Some(obj) = objects.get_mut(key) {
            let prev = obj.versions.first().cloned();
            obj.versions.insert(0, version.clone());
            // Keep at most 100 non-current versions to avoid unbounded growth
            obj.versions.truncate(100);
            prev
        } else {
            objects.insert(key.to_string(), S3Object::new(key, version.clone()));
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
    ) -> Option<ObjectVersion> {
        let objects = self.objects.entry(bucket.to_string()).or_default();
        if let Some(obj) = objects.get_mut(key) {
            let prev = obj.versions.first().cloned();
            obj.versions.insert(0, version);
            obj.versions.truncate(100);
            prev
        } else {
            objects.insert(key.to_string(), S3Object::new(key, version));
            None
        }
    }

    pub fn get_object(&self, bucket: &str, key: &str) -> Option<&ObjectVersion> {
        self.objects
            .get(bucket)
            .and_then(|objs| objs.get(key))
            .and_then(|obj| obj.current())
    }

    pub fn get_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Option<&ObjectVersion> {
        self.objects
            .get(bucket)
            .and_then(|objs| objs.get(key))
            .and_then(|obj| obj.versions.iter().find(|v| v.version_id == version_id))
    }

    pub fn delete_object(&mut self, bucket: &str, key: &str) -> Option<ObjectVersion> {
        let versioning = self
            .buckets
            .get(bucket)
            .map(|b| b.versioning.as_str() == "Enabled")
            .unwrap_or(false);

        let objects = self.objects.get_mut(bucket)?;

        if versioning {
            // Insert a delete marker
            let marker = ObjectVersion {
                version_id: Uuid::new_v4().to_string(),
                last_modified: Utc::now(),
                etag: String::new(),
                content_type: String::new(),
                content_encoding: None,
                content_disposition: None,
                cache_control: None,
                size: 0,
                metadata: HashMap::new(),
                acl: String::new(),
                data: ObjectDataRef::Inline(Vec::new()),
                delete_marker: true,
            };
            let obj = objects.entry(key.to_string()).or_insert_with(|| S3Object {
                key: key.to_string(),
                versions: Vec::new(),
            });
            obj.versions.insert(0, marker.clone());
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
    ) -> Option<ObjectVersion> {
        let objects = self.objects.get_mut(bucket)?;
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

    pub fn list_objects(&self, bucket: &str) -> Vec<&S3Object> {
        self.objects
            .get(bucket)
            .map(|m| m.values().collect())
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
    ) -> Option<ObjectVersion> {
        let upload = self.multipart_uploads.remove(upload_id)?;

        // Concatenate parts in order (inline only)
        let mut combined = Vec::new();
        let mut sorted_parts: Vec<u32> = parts.iter().map(|(n, _)| *n).collect();
        sorted_parts.sort_unstable();
        for part_num in sorted_parts {
            if let Some(part) = upload.parts.get(&part_num) {
                if let ObjectDataRef::Inline(bytes) = &part.data {
                    combined.extend_from_slice(bytes);
                }
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
            combined,
            upload.content_type.clone(),
            upload.metadata.clone(),
            versioning,
        );

        let objects = self.objects.entry(upload.bucket.clone()).or_default();
        if let Some(obj) = objects.get_mut(&upload.key) {
            obj.versions.insert(0, version.clone());
        } else {
            objects.insert(
                upload.key.clone(),
                S3Object::new(upload.key.clone(), version.clone()),
            );
        }

        Some(version)
    }

    /// Complete a multipart upload with a pre-assembled `ObjectVersion`.
    ///
    /// Used when parts are file-backed and the caller has already
    /// concatenated them on disk.
    pub fn complete_multipart_upload_with_version(
        &mut self,
        upload_id: &str,
        version: ObjectVersion,
    ) -> Option<ObjectVersion> {
        let upload = self.multipart_uploads.remove(upload_id)?;

        let objects = self.objects.entry(upload.bucket.clone()).or_default();
        if let Some(obj) = objects.get_mut(&upload.key) {
            obj.versions.insert(0, version.clone());
        } else {
            objects.insert(
                upload.key.clone(),
                S3Object::new(upload.key.clone(), version.clone()),
            );
        }

        Some(version)
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
