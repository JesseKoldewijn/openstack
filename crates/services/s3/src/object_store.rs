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
//! key is hashed (XXH3-128, hex-encoded) to produce a filesystem-safe
//! directory name.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashSet;
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, warn};
use xxhash_rust::xxh3::xxh3_128;

/// Process-wide counter used to generate unique temporary file names.
///
/// An atomic counter is used instead of `Uuid::new_v4()` because it
/// requires no OS-RNG syscall and is significantly faster under load.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Manages object data on the filesystem.
#[derive(Debug, Clone)]
pub struct ObjectFileStore {
    base_dir: PathBuf,
    known_dirs: Arc<DashSet<PathBuf>>,
}

#[derive(Debug, Clone, Copy)]
pub struct ObjectLocation<'a> {
    pub account_id: &'a str,
    pub region: &'a str,
    pub bucket: &'a str,
    pub key: &'a str,
    pub version_id: &'a str,
}

impl ObjectFileStore {
    /// Create a new `ObjectFileStore` rooted at `base_dir`.
    ///
    /// The directory is created if it does not exist.
    pub async fn new(base_dir: impl Into<PathBuf>) -> io::Result<Self> {
        let base_dir = base_dir.into();
        fs::create_dir_all(&base_dir).await?;
        Ok(Self {
            base_dir,
            known_dirs: Arc::new(DashSet::new()),
        })
    }

    // ── Path helpers ────────────────────────────────────────────────

    /// Hash an S3 key to a filesystem-safe hex string.
    pub fn key_hash(key: &str) -> String {
        format!("{:032x}", xxh3_128(key.as_bytes()))
    }

    /// Legacy SHA-256 hash used by older object path layouts.
    fn key_hash_legacy_sha256(key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        hex::encode(hasher.finalize())
    }

    async fn ensure_dir_exists(&self, dir: &Path) -> io::Result<()> {
        if self.known_dirs.contains(dir) {
            return Ok(());
        }
        fs::create_dir_all(dir).await?;
        self.known_dirs.insert(dir.to_path_buf());
        Ok(())
    }

    fn ensure_dir_exists_sync(&self, dir: &Path) -> io::Result<()> {
        if self.known_dirs.contains(dir) {
            return Ok(());
        }
        std::fs::create_dir_all(dir)?;
        self.known_dirs.insert(dir.to_path_buf());
        Ok(())
    }

    /// Build the directory path for a given object key within a bucket.
    fn object_dir(&self, account_id: &str, region: &str, bucket: &str, key: &str) -> PathBuf {
        self.base_dir
            .join(account_id)
            .join(region)
            .join(bucket)
            .join(Self::key_hash(key))
    }

    fn object_dir_legacy(
        &self,
        account_id: &str,
        region: &str,
        bucket: &str,
        key: &str,
    ) -> PathBuf {
        self.base_dir
            .join(account_id)
            .join(region)
            .join(bucket)
            .join(Self::key_hash_legacy_sha256(key))
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

    fn object_path_legacy(
        &self,
        account_id: &str,
        region: &str,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> PathBuf {
        self.object_dir_legacy(account_id, region, bucket, key)
            .join(version_id)
    }

    async fn resolve_existing_object_path(
        &self,
        account_id: &str,
        region: &str,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> io::Result<PathBuf> {
        let current = self.object_path(account_id, region, bucket, key, version_id);
        if fs::metadata(&current).await.is_ok() {
            return Ok(current);
        }

        let legacy = self.object_path_legacy(account_id, region, bucket, key, version_id);
        if fs::metadata(&legacy).await.is_ok() {
            return Ok(legacy);
        }

        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "object file not found in current or legacy layout",
        ))
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
        self.ensure_dir_exists(&dir).await?;

        let final_path = dir.join(version_id);
        let tmp_id = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_path = dir.join(format!("{version_id}-{tmp_id}.tmp"));

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
    /// All objects use [`write_via_copy`]: an async read loop with a 512 KiB
    /// `BufReader` feeding an adaptive `BufWriter` (2–8 MiB depending on
    /// `content_length`).  Pre-allocates the file with `set_len` when size
    /// is known to avoid block-level fragmentation.
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
        content_length: Option<u64>,
    ) -> io::Result<(PathBuf, u64)>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let dir = self.object_dir(account_id, region, bucket, key);
        self.ensure_dir_exists(&dir).await?;

        let final_path = dir.join(version_id);
        let tmp_id = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_path = dir.join(format!("{version_id}-{tmp_id}.tmp"));

        let bytes_written = write_via_copy(reader, &tmp_path, content_length).await?;

        fs::rename(&tmp_path, &final_path).await?;

        debug!(
            path = %final_path.display(),
            size = bytes_written,
            "Object written to filesystem (streamed)"
        );

        Ok((final_path, bytes_written))
    }

    /// Write object data from a synchronous reader, streaming to disk.
    ///
    /// Intended for use inside [`tokio::task::spawn_blocking`] or
    /// [`tokio::task::block_in_place`] closures so that disk I/O does not
    /// block tokio worker threads.
    ///
    /// When `content_length` is known the file is pre-allocated with
    /// `set_len` to avoid filesystem block re-allocation, and the write
    /// buffer is scaled to match the object size:
    ///
    /// | Content-Length    | Write buffer |
    /// |------------------|--------------|
    /// | < 4 MiB or None  | 2 MiB        |
    /// | 4 MiB – 50 MiB   | 4 MiB        |
    /// | 50 MiB – 128 MiB | 8 MiB        |
    /// | > 128 MiB        | 16 MiB       |
    ///
    /// Returns `(final_path, bytes_written)`.
    pub fn write_object_from_sync_reader<R>(
        &self,
        account_id: &str,
        region: &str,
        bucket: &str,
        key: &str,
        version_id: &str,
        reader: &mut R,
        content_length: Option<u64>,
    ) -> io::Result<(PathBuf, u64)>
    where
        R: std::io::Read,
    {
        use std::io::Write;

        let dir = self.object_dir(account_id, region, bucket, key);
        self.ensure_dir_exists_sync(&dir)?;

        let final_path = dir.join(version_id);
        let tmp_id = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_path = dir.join(format!("{version_id}-{tmp_id}.tmp"));

        let file = std::fs::File::create(&tmp_path)?;

        // Pre-allocate to avoid block-level fragmentation on ext4/xfs.
        if let Some(len) = content_length
            && len > 0
        {
            file.set_len(len)?;
        }

        // Adaptive write buffer: scale with expected object size to reduce
        // write(2) syscall overhead. Read buffer stays at 512 KiB.
        const READ_BUF: usize = 512 * 1024; // 512 KiB
        let write_buf: usize = match content_length {
            Some(len) if len > 128 * 1024 * 1024 => 16 * 1024 * 1024, // 16 MiB for > 128 MiB
            Some(len) if len > 50 * 1024 * 1024 => 8 * 1024 * 1024,   //  8 MiB for > 50 MiB
            Some(len) if len > 4 * 1024 * 1024 => 4 * 1024 * 1024,    //  4 MiB for > 4 MiB
            _ => 2 * 1024 * 1024,                                     //  2 MiB default
        };
        let mut buf_writer = std::io::BufWriter::with_capacity(write_buf, file);
        let mut buf = vec![0u8; READ_BUF];
        let mut bytes_written = 0u64;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            buf_writer.write_all(&buf[..n])?;
            bytes_written += n as u64;
        }
        buf_writer.flush()?;
        // Ensure the file is closed before rename (important on Windows;
        // harmless on Linux).
        drop(buf_writer);

        std::fs::rename(&tmp_path, &final_path)?;

        debug!(
            path = %final_path.display(),
            size = bytes_written,
            "Object written to filesystem (sync streamed)"
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
        let path = self
            .resolve_existing_object_path(account_id, region, bucket, key, version_id)
            .await?;
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
        let current_path = self.object_path(account_id, region, bucket, key, version_id);
        let legacy_path = self.object_path_legacy(account_id, region, bucket, key, version_id);

        let mut deleted = false;
        match fs::remove_file(&current_path).await {
            Ok(()) => {
                deleted = true;
                debug!(path = %current_path.display(), "Object file deleted");
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                match fs::remove_file(&legacy_path).await {
                    Ok(()) => {
                        deleted = true;
                        debug!(path = %legacy_path.display(), "Legacy object file deleted");
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        // Already gone — not an error.
                    }
                    Err(e) => return Err(e),
                }
            }
            Err(e) => return Err(e),
        }

        if !deleted {
            debug!(
                current_path = %current_path.display(),
                legacy_path = %legacy_path.display(),
                "Object file already absent"
            );
        }

        // Clean up empty parent directories in both current and legacy layouts.
        let current_key_dir = self.object_dir(account_id, region, bucket, key);
        let legacy_key_dir = self.object_dir_legacy(account_id, region, bucket, key);

        Self::remove_dir_if_empty(&current_key_dir).await;
        if fs::metadata(&current_key_dir).await.is_err() {
            self.known_dirs.remove(&current_key_dir);
        }

        if legacy_key_dir != current_key_dir {
            Self::remove_dir_if_empty(&legacy_key_dir).await;
            if fs::metadata(&legacy_key_dir).await.is_err() {
                self.known_dirs.remove(&legacy_key_dir);
            }
        }

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
                let stale_dirs: Vec<PathBuf> = self
                    .known_dirs
                    .iter()
                    .filter_map(|known| {
                        if known.as_path().starts_with(&dir) {
                            Some(known.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                for stale in stale_dirs {
                    self.known_dirs.remove(&stale);
                }
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
        src: ObjectLocation<'_>,
        dst: ObjectLocation<'_>,
    ) -> io::Result<PathBuf> {
        let src = self
            .resolve_existing_object_path(
                src.account_id,
                src.region,
                src.bucket,
                src.key,
                src.version_id,
            )
            .await?;
        let dst_dir = self.object_dir(dst.account_id, dst.region, dst.bucket, dst.key);
        self.ensure_dir_exists(&dst_dir).await?;
        let dst = dst_dir.join(dst.version_id);

        // Short-circuit when copying an object to itself (same-key copy in a
        // non-versioned bucket produces identical src/dst paths).  Both
        // hard_link and fs::copy would either fail or truncate the file.
        if src == dst {
            return Ok(dst);
        }

        // Prefer a hard link (O(1), no data copy) and fall back to a full
        // byte copy if the source and destination are on different
        // filesystems or the filesystem does not support hard links.
        if fs::hard_link(&src, &dst).await.is_err() {
            fs::copy(&src, &dst).await?;
        }

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
                } else if ft.is_file()
                    && let Some(name) = entry.file_name().to_str()
                    && name.ends_with(".tmp")
                {
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

/// Simple write path for all objects.
///
/// Wraps the reader in a `BufReader` so `copy_buf` issues large reads
/// (vs. the 8 KiB internal buffer of `tokio::io::copy`).  Writes go
/// through an adaptive `BufWriter` whose size scales with the expected
/// object size:
///
/// | Content-Length    | Write buffer | Approx. `spawn_blocking` dispatches |
/// |------------------|--------------|-------------------------------------|
/// | < 4 MiB or None  | 2 MiB        | ≤ 2                                 |
/// | 4 MiB – 50 MiB   | 4 MiB        | ≤ 13                                |
/// | 50 MiB – 128 MiB | 8 MiB        | ≤ 16                                |
/// | > 128 MiB        | 16 MiB       | ≤ 8 per 128 MiB                    |
///
/// Fewer dispatches → fewer blocking-pool round-trips → lower p95 latency
/// for large objects without extra heap allocation for small objects.
///
/// When `content_length` is known the file is pre-allocated with
/// `set_len` to avoid filesystem block re-allocation during sequential writes.
async fn write_via_copy<R>(
    reader: &mut R,
    tmp_path: &Path,
    content_length: Option<u64>,
) -> io::Result<u64>
where
    R: tokio::io::AsyncRead + Unpin,
{
    // Adaptive buffer sizing: scale the write buffer with expected object size
    // to reduce the number of spawn_blocking dispatches per object.
    // Read buffer stays at 512 KiB (fits L2/L3 cache well).
    const READ_BUF: usize = 512 * 1024; // 512 KiB
    let write_buf: usize = match content_length {
        Some(len) if len > 128 * 1024 * 1024 => 16 * 1024 * 1024, // 16 MiB for > 128 MiB
        Some(len) if len >= 50 * 1024 * 1024 => 8 * 1024 * 1024,  //  8 MiB for >= 50 MiB
        Some(len) if len > 4 * 1024 * 1024 => 4 * 1024 * 1024,    //  4 MiB for > 4 MiB
        _ => 2 * 1024 * 1024,                                     //  2 MiB default
    };

    let file = fs::File::create(tmp_path).await?;

    // Pre-allocate to avoid block-level fragmentation on ext4/xfs.
    if let Some(len) = content_length
        && len > 0
    {
        file.set_len(len).await?;
    }

    let mut bw = tokio::io::BufWriter::with_capacity(write_buf, file);
    let mut br = tokio::io::BufReader::with_capacity(READ_BUF, reader);
    let bytes_written = tokio::io::copy_buf(&mut br, &mut bw).await?;
    bw.flush().await?;
    drop(bw); // close fd before caller renames

    Ok(bytes_written)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

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
            .write_object_from_reader("acct1", "us-east-1", "bkt", "key1", "v1", &mut cursor, None)
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
                ObjectLocation {
                    account_id: "acct1",
                    region: "us-east-1",
                    bucket: "src-bkt",
                    key: "srckey",
                    version_id: "v1",
                },
                ObjectLocation {
                    account_id: "acct1",
                    region: "us-east-1",
                    bucket: "dst-bkt",
                    key: "dstkey",
                    version_id: "v2",
                },
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

        // Should be a valid hex string of length 32 (XXH3-128).
        assert_eq!(h1.len(), 32);
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
    async fn concurrent_writes_same_key_no_race() {
        // Spawning 10 concurrent tasks writing to the same version_id
        // ("null") used to race on the shared `null.tmp` temp file.
        // With UUID-suffixed temp files each task gets its own temp path
        // and all writes should complete successfully.
        let (store, _tmp) = make_store().await;
        let store = std::sync::Arc::new(store);

        // Write 10 different data payloads concurrently to the same key.
        let handles: Vec<_> = (0u8..10)
            .map(|i| {
                let store = std::sync::Arc::clone(&store);
                tokio::spawn(async move {
                    store
                        .write_object("acct1", "us-east-1", "bkt", "same-key", "null", &[i; 64])
                        .await
                })
            })
            .collect();

        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await);
        }
        for join_result in results {
            // Every task must have finished without a filesystem error.
            join_result
                .expect("task panicked")
                .expect("write_object failed");
        }

        // The final object file must exist (last writer wins via rename).
        let key_dir = store.object_dir("acct1", "us-east-1", "bkt", "same-key");
        let final_path = key_dir.join("null");
        assert!(final_path.exists(), "final object file should exist");
        let final_bytes = tokio::fs::read(&final_path).await.unwrap();
        assert_eq!(final_bytes.len(), 64);
        assert!(
            (0u8..10).any(|i| final_bytes == vec![i; 64]),
            "final object should match one complete writer payload"
        );

        // No .tmp files should remain.
        let mut entries = tokio::fs::read_dir(&key_dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            assert!(!name.ends_with(".tmp"), "orphaned temp file found: {name}");
        }
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

    #[tokio::test]
    async fn read_object_falls_back_to_legacy_sha256_layout() {
        let (store, _tmp) = make_store().await;
        let legacy_dir = store.object_dir_legacy("acct1", "us-east-1", "bkt", "legacy-key");
        tokio::fs::create_dir_all(&legacy_dir).await.unwrap();
        let legacy_path = legacy_dir.join("v1");
        tokio::fs::write(&legacy_path, b"legacy-data")
            .await
            .unwrap();

        let mut file = store
            .read_object("acct1", "us-east-1", "bkt", "legacy-key", "v1")
            .await
            .expect("legacy object should be readable");
        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"legacy-data");
    }
}
