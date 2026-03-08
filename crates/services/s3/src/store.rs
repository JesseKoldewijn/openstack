use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    /// The actual object data
    #[serde(with = "serde_bytes_base64")]
    pub data: Vec<u8>,
    /// True when this is a delete marker
    pub delete_marker: bool,
}

impl ObjectVersion {
    pub fn new(
        data: Vec<u8>,
        content_type: impl Into<String>,
        metadata: HashMap<String, String>,
        versioning_enabled: bool,
    ) -> Self {
        let etag = format!("\"{}\"", hex::encode(md5_bytes(&data)));
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
            size: data.len() as u64,
            metadata,
            acl: "private".to_string(),
            data,
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
    #[serde(with = "serde_bytes_base64")]
    pub data: Vec<u8>,
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
        data: Vec<u8>,
        content_type: impl Into<String>,
        metadata: HashMap<String, String>,
    ) -> Option<ObjectVersion> {
        let versioning = self
            .buckets
            .get(bucket)
            .map(|b| b.versioning.as_str() == "Enabled")
            .unwrap_or(false);

        let version = ObjectVersion::new(data, content_type, metadata, versioning);

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
                data: Vec::new(),
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
        data: Vec<u8>,
    ) -> Option<String> {
        let upload = self.multipart_uploads.get_mut(upload_id)?;
        let etag = format!("\"{}\"", hex::encode(md5_bytes(&data)));
        let part = UploadPart {
            part_number,
            etag: etag.clone(),
            size: data.len() as u64,
            data,
            last_modified: Utc::now(),
        };
        upload.parts.insert(part_number, part);
        Some(etag)
    }

    pub fn complete_multipart_upload(
        &mut self,
        upload_id: &str,
        parts: &[(u32, String)], // (part_number, etag)
    ) -> Option<ObjectVersion> {
        let upload = self.multipart_uploads.remove(upload_id)?;

        // Concatenate parts in order
        let mut combined = Vec::new();
        let mut sorted_parts: Vec<u32> = parts.iter().map(|(n, _)| *n).collect();
        sorted_parts.sort_unstable();
        for part_num in sorted_parts {
            if let Some(part) = upload.parts.get(&part_num) {
                combined.extend_from_slice(&part.data);
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

// ---------------------------------------------------------------------------
// Custom serde helper: serialize Vec<u8> as base64 string
// ---------------------------------------------------------------------------

mod serde_bytes_base64 {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(data: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        STANDARD.encode(data).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        STANDARD.decode(&s).map_err(serde::de::Error::custom)
    }
}
