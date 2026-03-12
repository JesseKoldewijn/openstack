//! Filesystem-backed object storage for S3.
//!
//! Objects are stored as individual files under a configurable base
//! directory with the layout:
//!
//! ```text
//! {base_dir}/{account_id}/{region}/{bucket}/{key_hash}/{version_id}
//! ```
//!
//! S3 keys can contain characters that are invalid in file paths, so the
//! key is hashed (SHA-256, hex-encoded) to produce a filesystem-safe
//! directory name.

use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, warn};

/// Manages object data on the filesystem.
#[derive(Debug, Clone)]
pub struct ObjectFileStore {
    base_dir: PathBuf,
}

impl ObjectFileStore {
    /// Create a new `ObjectFileStore` rooted at `base_dir`.
    ///
    /// The directory is created if it does not exist.
    pub async fn new(base_dir: impl Into<PathBuf>) -> io::Result<Self> {
        let base_dir = base_dir.into();
        fs::create_dir_all(&base_dir).await?;
        Ok(Self { base_dir })
    }

    // ── Path helpers ────────────────────────────────────────────────

    /// Hash an S3 key to a filesystem-safe hex string.
    pub fn key_hash(key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Build the directory path for a given object key within a bucket.
    fn object_dir(&self, account_id: &str, region: &str, bucket: &str, key: &str) -> PathBuf {
        self.base_dir
            .join(account_id)
            .join(region)
            .join(bucket)
            .join(Self::key_hash(key))
    }

    /// Build the full file path for a specific object version.
    fn object_path(
        &self,
        account_id: &str,
        region: &str,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> PathBuf {
        self.object_dir(account_id, region, bucket, key)
            .join(version_id)
    }

    /// Build the bucket-level directory path.
    fn bucket_dir(&self, account_id: &str, region: &str, bucket: &str) -> PathBuf {
        self.base_dir.join(account_id).join(region).join(bucket)
    }

    // ── Write ───────────────────────────────────────────────────────

    /// Write object data to the filesystem.
    ///
    /// Data is first written to a temporary file in the same directory
    /// and then atomically renamed to its final path to prevent partial
    /// writes from being visible.
    ///
    /// Returns the final `PathBuf` on success.
    pub async fn write_object(
        &self,
        account_id: &str,
        region: &str,
        bucket: &str,
        key: &str,
        version_id: &str,
        data: &[u8],
    ) -> io::Result<PathBuf> {
        let dir = self.object_dir(account_id, region, bucket, key);
        fs::create_dir_all(&dir).await?;

        let final_path = dir.join(version_id);
        let tmp_path = dir.join(format!("{}.tmp", version_id));

        let mut file = fs::File::create(&tmp_path).await?;
        file.write_all(data).await?;
        file.flush().await?;
        // Ensure data is flushed to the OS (not necessarily disk, but
        // good enough for our use case).
        drop(file);

        fs::rename(&tmp_path, &final_path).await?;

        debug!(
            path = %final_path.display(),
            size = data.len(),
            "Object written to filesystem"
        );

        Ok(final_path)
    }

    /// Write object data from a reader (async), streaming to disk.
    ///
    /// Returns `(final_path, bytes_written)`.
    pub async fn write_object_from_reader<R>(
        &self,
        account_id: &str,
        region: &str,
        bucket: &str,
        key: &str,
        version_id: &str,
        reader: &mut R,
    ) -> io::Result<(PathBuf, u64)>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let dir = self.object_dir(account_id, region, bucket, key);
        fs::create_dir_all(&dir).await?;

        let final_path = dir.join(version_id);
        let tmp_path = dir.join(format!("{}.tmp", version_id));

        let mut file = fs::File::create(&tmp_path).await?;
        let bytes_written = tokio::io::copy(reader, &mut file).await?;
        file.flush().await?;
        drop(file);

        fs::rename(&tmp_path, &final_path).await?;

        debug!(
            path = %final_path.display(),
            size = bytes_written,
            "Object written to filesystem (streamed)"
        );

        Ok((final_path, bytes_written))
    }

    // ── Read ────────────────────────────────────────────────────────

    /// Open an object file for reading.
    ///
    /// Returns a `tokio::fs::File` that the caller can wrap in a
    /// `ReaderStream` for streaming responses.
    pub async fn read_object(
        &self,
        account_id: &str,
        region: &str,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> io::Result<fs::File> {
        let path = self.object_path(account_id, region, bucket, key, version_id);
        fs::File::open(&path).await
    }

    /// Open an object file by its stored path.
    pub async fn read_object_at(path: &Path) -> io::Result<fs::File> {
        fs::File::open(path).await
    }

    // ── Delete ──────────────────────────────────────────────────────

    /// Delete a specific object version from the filesystem.
    ///
    /// After removing the file, empty parent directories up to the
    /// bucket level are cleaned up.
    pub async fn delete_object(
        &self,
        account_id: &str,
        region: &str,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> io::Result<()> {
        let path = self.object_path(account_id, region, bucket, key, version_id);
        match fs::remove_file(&path).await {
            Ok(()) => {
                debug!(path = %path.display(), "Object file deleted");
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Already gone — not an error.
                debug!(path = %path.display(), "Object file already absent");
            }
            Err(e) => return Err(e),
        }

        // Clean up empty parent directories (key_hash dir, then bucket dir).
        let key_dir = self.object_dir(account_id, region, bucket, key);
        Self::remove_dir_if_empty(&key_dir).await;

        Ok(())
    }

    /// Remove the entire bucket directory tree.
    pub async fn delete_bucket_dir(
        &self,
        account_id: &str,
        region: &str,
        bucket: &str,
    ) -> io::Result<()> {
        let dir = self.bucket_dir(account_id, region, bucket);
        match fs::remove_dir_all(&dir).await {
            Ok(()) => {
                debug!(path = %dir.display(), "Bucket directory deleted");
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    // ── Copy ────────────────────────────────────────────────────────

    /// Copy an object file from one location to another.
    ///
    /// Returns the destination `PathBuf`.
    pub async fn copy_object(
        &self,
        src_account_id: &str,
        src_region: &str,
        src_bucket: &str,
        src_key: &str,
        src_version_id: &str,
        dst_account_id: &str,
        dst_region: &str,
        dst_bucket: &str,
        dst_key: &str,
        dst_version_id: &str,
    ) -> io::Result<PathBuf> {
        let src = self.object_path(
            src_account_id,
            src_region,
            src_bucket,
            src_key,
            src_version_id,
        );
        let dst_dir = self.object_dir(dst_account_id, dst_region, dst_bucket, dst_key);
        fs::create_dir_all(&dst_dir).await?;
        let dst = dst_dir.join(dst_version_id);

        fs::copy(&src, &dst).await?;

        debug!(
            src = %src.display(),
            dst = %dst.display(),
            "Object file copied"
        );

        Ok(dst)
    }

    // ── Cleanup ─────────────────────────────────────────────────────

    /// Scan the base directory for `.tmp` files left over from
    /// incomplete writes (e.g. after a crash) and remove them.
    pub async fn cleanup_orphaned_temps(&self) -> io::Result<usize> {
        let mut count = 0usize;
        let mut stack = vec![self.base_dir.clone()];

        while let Some(dir) = stack.pop() {
            let mut entries = match fs::read_dir(&dir).await {
                Ok(e) => e,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };

            while let Some(entry) = entries.next_entry().await? {
                let ft = entry.file_type().await?;
                if ft.is_dir() {
                    stack.push(entry.path());
                } else if ft.is_file() {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.ends_with(".tmp") {
                            match fs::remove_file(entry.path()).await {
                                Ok(()) => {
                                    count += 1;
                                    debug!(path = %entry.path().display(), "Removed orphaned temp file");
                                }
                                Err(e) => {
                                    warn!(
                                        path = %entry.path().display(),
                                        error = %e,
                                        "Failed to remove orphaned temp file"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        if count > 0 {
            warn!(count, "Cleaned up orphaned temp files");
        }

        Ok(count)
    }

    // ── Utilities ───────────────────────────────────────────────────

    /// Remove a directory only if it is empty.  Silently ignores errors
    /// (non-empty, not-found, permission, etc.).
    async fn remove_dir_if_empty(path: &Path) {
        // `remove_dir` fails if the directory is non-empty, which is
        // exactly the check we want.
        let _ = fs::remove_dir(path).await;
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    async fn make_store() -> (ObjectFileStore, TempDir) {
        let tmp = TempDir::new().unwrap();
        let store = ObjectFileStore::new(tmp.path().join("objects"))
            .await
            .unwrap();
        (store, tmp)
    }

    #[tokio::test]
    async fn write_and_read_object() {
        let (store, _tmp) = make_store().await;
        let data = b"hello world";
        let path = store
            .write_object("acct1", "us-east-1", "mybucket", "mykey", "v1", data)
            .await
            .unwrap();

        assert!(path.exists());

        let mut file = store
            .read_object("acct1", "us-east-1", "mybucket", "mykey", "v1")
            .await
            .unwrap();

        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await.unwrap();
        assert_eq!(&buf, data);
    }

    #[tokio::test]
    async fn write_from_reader() {
        let (store, _tmp) = make_store().await;
        let data = b"streamed data here";
        let mut cursor = tokio::io::BufReader::new(&data[..]);

        let (path, n) = store
            .write_object_from_reader("acct1", "us-east-1", "bkt", "key1", "v1", &mut cursor)
            .await
            .unwrap();

        assert_eq!(n, data.len() as u64);
        assert!(path.exists());

        let contents = tokio::fs::read(&path).await.unwrap();
        assert_eq!(&contents, data);
    }

    #[tokio::test]
    async fn delete_object_cleans_up() {
        let (store, _tmp) = make_store().await;
        store
            .write_object("acct1", "us-east-1", "bkt", "key1", "v1", b"data")
            .await
            .unwrap();

        store
            .delete_object("acct1", "us-east-1", "bkt", "key1", "v1")
            .await
            .unwrap();

        // File should be gone.
        let result = store
            .read_object("acct1", "us-east-1", "bkt", "key1", "v1")
            .await;
        assert!(result.is_err());

        // The key hash directory should have been cleaned up (it was empty).
        let key_dir = store.object_dir("acct1", "us-east-1", "bkt", "key1");
        assert!(!key_dir.exists());
    }

    #[tokio::test]
    async fn delete_nonexistent_is_ok() {
        let (store, _tmp) = make_store().await;
        // Deleting a file that doesn't exist should succeed.
        store
            .delete_object("acct1", "us-east-1", "bkt", "nokey", "v1")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn delete_bucket_dir_removes_tree() {
        let (store, _tmp) = make_store().await;
        store
            .write_object("acct1", "us-east-1", "bkt", "k1", "v1", b"a")
            .await
            .unwrap();
        store
            .write_object("acct1", "us-east-1", "bkt", "k2", "v1", b"b")
            .await
            .unwrap();

        store
            .delete_bucket_dir("acct1", "us-east-1", "bkt")
            .await
            .unwrap();

        let dir = store.bucket_dir("acct1", "us-east-1", "bkt");
        assert!(!dir.exists());
    }

    #[tokio::test]
    async fn copy_object_works() {
        let (store, _tmp) = make_store().await;
        let data = b"copy me";
        store
            .write_object("acct1", "us-east-1", "src-bkt", "srckey", "v1", data)
            .await
            .unwrap();

        let dst = store
            .copy_object(
                "acct1",
                "us-east-1",
                "src-bkt",
                "srckey",
                "v1",
                "acct1",
                "us-east-1",
                "dst-bkt",
                "dstkey",
                "v2",
            )
            .await
            .unwrap();

        let contents = tokio::fs::read(&dst).await.unwrap();
        assert_eq!(&contents, data);
    }

    #[tokio::test]
    async fn key_hash_is_deterministic() {
        let h1 = ObjectFileStore::key_hash("photos/vacation/2024/img.jpg");
        let h2 = ObjectFileStore::key_hash("photos/vacation/2024/img.jpg");
        assert_eq!(h1, h2);

        let h3 = ObjectFileStore::key_hash("different/key");
        assert_ne!(h1, h3);

        // Should be a valid hex string of length 64 (SHA-256).
        assert_eq!(h1.len(), 64);
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn cleanup_orphaned_temps() {
        let (store, _tmp) = make_store().await;
        // Write a normal object.
        store
            .write_object("acct1", "us-east-1", "bkt", "k", "v1", b"good")
            .await
            .unwrap();

        // Manually create a .tmp file (simulating a crash mid-write).
        let dir = store.object_dir("acct1", "us-east-1", "bkt", "k");
        let tmp_file = dir.join("v2.tmp");
        tokio::fs::write(&tmp_file, b"partial").await.unwrap();
        assert!(tmp_file.exists());

        let cleaned = store.cleanup_orphaned_temps().await.unwrap();
        assert_eq!(cleaned, 1);
        assert!(!tmp_file.exists());

        // Normal file should still be there.
        let real_path = store.object_path("acct1", "us-east-1", "bkt", "k", "v1");
        assert!(real_path.exists());
    }

    #[tokio::test]
    async fn atomic_write_no_partial_visible() {
        let (store, _tmp) = make_store().await;
        let path = store.object_path("acct1", "us-east-1", "bkt", "k", "v1");

        // Before write, the final path should not exist.
        assert!(!path.exists());

        store
            .write_object("acct1", "us-east-1", "bkt", "k", "v1", b"atomic data")
            .await
            .unwrap();

        // After write, it should exist with correct content.
        let contents = tokio::fs::read(&path).await.unwrap();
        assert_eq!(&contents, b"atomic data");

        // No .tmp file should remain.
        let dir = path.parent().unwrap();
        let tmp_path = dir.join("v1.tmp");
        assert!(!tmp_path.exists());
    }
}
