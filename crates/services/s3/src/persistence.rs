//! S3 persistence adapter.
//!
//! Implements [`PersistableStore`] for S3, serializing metadata and
//! `ObjectDataRef::FileRef` paths as **relative** to the S3 objects
//! directory.  Object body files are already durable on disk and are
//! not re-written during save.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use openstack_state::{AccountRegionBundle, PersistableStore};
use tracing::warn;

use crate::store::{ObjectDataRef, S3Store};

/// Persistence adapter for S3 that shares the in-memory store with
/// [`S3Provider`](crate::provider::S3Provider).
pub struct S3PersistableStore {
    bundle: Arc<AccountRegionBundle<S3Store>>,
    /// Root directory for S3 object files (used to relativize/resolve paths).
    s3_objects_dir: PathBuf,
}

impl S3PersistableStore {
    pub fn new(bundle: Arc<AccountRegionBundle<S3Store>>, s3_objects_dir: PathBuf) -> Self {
        Self {
            bundle,
            s3_objects_dir,
        }
    }
}

#[async_trait::async_trait]
impl PersistableStore for S3PersistableStore {
    fn service_name(&self) -> &str {
        "s3"
    }

    async fn save(&self, data_dir: &Path) -> Result<(), anyhow::Error> {
        for entry in self.bundle.iter() {
            let key = entry.key();
            let path = openstack_state::state_path(data_dir, "s3", &key.account_id, &key.region);

            // Clone the store so we can relativize FileRef paths without
            // holding the DashMap guard or mutating the live store.
            let mut snapshot = entry.value().clone();
            relativize_paths(&mut snapshot, &self.s3_objects_dir);

            openstack_state::save_store(&snapshot, &path).await?;
        }
        Ok(())
    }

    async fn load(&self, data_dir: &Path) -> Result<(), anyhow::Error> {
        let base = data_dir.join("state").join("s3");
        if !base.exists() {
            return Ok(());
        }

        let mut rd = tokio::fs::read_dir(&base).await?;
        while let Some(account_entry) = rd.next_entry().await? {
            if !account_entry.file_type().await?.is_dir() {
                continue;
            }
            let account_id = account_entry.file_name().to_string_lossy().to_string();

            let mut rd2 = tokio::fs::read_dir(account_entry.path()).await?;
            while let Some(region_entry) = rd2.next_entry().await? {
                if !region_entry.file_type().await?.is_dir() {
                    continue;
                }
                let region = region_entry.file_name().to_string_lossy().to_string();
                let path = region_entry.path().join("store.json");

                let mut store: S3Store = openstack_state::load_store(&path).await?;
                resolve_and_verify_paths(&mut store, &self.s3_objects_dir);

                *self.bundle.get_or_create(&account_id, &region) = store;
            }
        }
        Ok(())
    }

    fn reset(&self) {
        self.bundle.clear();
    }
}

// ---------------------------------------------------------------------------
// Path relativization (save) and resolution (load)
// ---------------------------------------------------------------------------

/// Convert all `FileRef` absolute paths to paths relative to `base`.
fn relativize_paths(store: &mut S3Store, base: &Path) {
    // Object versions
    for objects_by_key in store.objects.values_mut() {
        for s3_obj in objects_by_key.values_mut() {
            for version in &mut s3_obj.versions {
                relativize_data_ref(&mut version.data, base);
            }
        }
    }

    // Multipart upload parts
    for upload in store.multipart_uploads.values_mut() {
        for part in upload.parts.values_mut() {
            relativize_data_ref(&mut part.data, base);
        }
    }
}

/// Convert all relative `FileRef` paths back to absolute paths and verify
/// that the referenced files exist.  Missing files are logged as warnings
/// and the data ref is left as-is (the object metadata is retained but
/// reads will fail gracefully).
fn resolve_and_verify_paths(store: &mut S3Store, base: &Path) {
    // Object versions
    for (bucket_name, objects_by_key) in store.objects.iter_mut() {
        for (key, s3_obj) in objects_by_key.iter_mut() {
            for version in &mut s3_obj.versions {
                resolve_data_ref(&mut version.data, base, bucket_name, key);
            }
        }
    }

    // Multipart upload parts
    for upload in store.multipart_uploads.values_mut() {
        for part in upload.parts.values_mut() {
            resolve_data_ref(
                &mut part.data,
                base,
                &upload.bucket,
                &format!("__multipart/{}/part-{}", upload.upload_id, part.part_number),
            );
        }
    }
}

fn relativize_data_ref(data: &mut ObjectDataRef, base: &Path) {
    if let ObjectDataRef::FileRef(abs_path) = data
        && let Ok(rel) = abs_path.strip_prefix(base)
    {
        *data = ObjectDataRef::FileRef(rel.to_path_buf());
        // If strip_prefix fails, the path is already relative or points
        // elsewhere — leave it as-is.
    }
}

fn resolve_data_ref(data: &mut ObjectDataRef, base: &Path, bucket: &str, key: &str) {
    if let ObjectDataRef::FileRef(rel_path) = data {
        if rel_path.is_relative() {
            let abs_path = base.join(&*rel_path);
            if !abs_path.exists() {
                warn!(
                    bucket = bucket,
                    key = key,
                    path = %abs_path.display(),
                    "S3 object file missing on disk — object metadata retained but data unavailable"
                );
            }
            *data = ObjectDataRef::FileRef(abs_path);
        }
        // If the path is already absolute (legacy snapshot), leave it and
        // just verify existence.
        else if !rel_path.exists() {
            warn!(
                bucket = bucket,
                key = key,
                path = %rel_path.display(),
                "S3 object file missing on disk — object metadata retained but data unavailable"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::store::{ObjectVersion, S3Object, S3Store};

    #[test]
    fn relativize_and_resolve_roundtrip() {
        let base = PathBuf::from("/data/s3/objects");
        let mut store = S3Store::default();

        // Insert an object with an absolute FileRef path
        let abs_path = PathBuf::from("/data/s3/objects/acct/us-east-1/mybucket/abc123/null");
        let version = ObjectVersion::new_with_file_ref(
            abs_path.clone(),
            42,
            "\"etag\"".to_string(),
            "application/octet-stream",
            HashMap::new(),
            false,
        );
        let s3_obj = S3Object::new("my-key", version);
        store
            .objects
            .entry("mybucket".to_string())
            .or_default()
            .insert("my-key".to_string(), s3_obj);

        // Relativize
        relativize_paths(&mut store, &base);
        let data = &store.objects["mybucket"]["my-key"].versions[0].data;
        assert_eq!(
            data,
            &ObjectDataRef::FileRef(PathBuf::from("acct/us-east-1/mybucket/abc123/null"))
        );

        // Resolve (note: file won't exist on disk in test, but path should be absolute)
        resolve_and_verify_paths(&mut store, &base);
        let data = &store.objects["mybucket"]["my-key"].versions[0].data;
        assert_eq!(data, &ObjectDataRef::FileRef(abs_path));
    }

    #[test]
    fn inline_data_untouched() {
        let base = PathBuf::from("/data/s3/objects");
        let mut store = S3Store::default();

        let version = ObjectVersion::new(b"hello".to_vec(), "text/plain", HashMap::new(), false);
        let s3_obj = S3Object::new("inline-key", version);
        store
            .objects
            .entry("mybucket".to_string())
            .or_default()
            .insert("inline-key".to_string(), s3_obj);

        // Relativize should not touch Inline data
        relativize_paths(&mut store, &base);
        let data = &store.objects["mybucket"]["inline-key"].versions[0].data;
        assert_eq!(data, &ObjectDataRef::Inline(b"hello".to_vec()));

        // Resolve should not touch Inline data
        resolve_and_verify_paths(&mut store, &base);
        let data = &store.objects["mybucket"]["inline-key"].versions[0].data;
        assert_eq!(data, &ObjectDataRef::Inline(b"hello".to_vec()));
    }
}
