## ADDED Requirements

### Requirement: S3 object data SHALL be stored on the filesystem
The S3 service SHALL store object body data as individual files on the filesystem under `{DATA_DIR}/objects/s3/{account_id}/{region}/{bucket}/{key_hash}/{version_id}` rather than holding `Vec<u8>` in memory. The `ObjectVersion` data field SHALL use an `ObjectDataRef` enum that supports both inline data (for delete markers and empty bodies) and filesystem path references.

#### Scenario: PutObject stores data on disk
- **WHEN** a PutObject request is processed with a 50 MiB body
- **THEN** the object body is written to a file at the canonical filesystem path, and the `ObjectVersion` holds a `FileRef` pointing to that path, not the raw bytes

#### Scenario: Delete markers use inline storage
- **WHEN** a delete marker is created for a versioned object
- **THEN** the `ObjectVersion` uses `ObjectDataRef::Inline` with an empty byte vector, and no file is created on disk

#### Scenario: GetObject reads data from disk
- **WHEN** a GetObject request is processed for a filesystem-backed object
- **THEN** the object body is read from the file at the stored path, not from process memory

#### Scenario: Object files use content-safe paths
- **WHEN** an S3 key contains characters that are invalid in filesystem paths (e.g., `/`, `\0`, or very long names)
- **THEN** the key is hashed to produce a filesystem-safe directory name, preventing path injection or invalid path errors

### Requirement: Object files SHALL be written atomically
The S3 service SHALL write object data to a temporary file first and then atomically rename it to the final path. No partially-written object file SHALL be visible at the canonical path.

#### Scenario: Atomic write via rename
- **WHEN** a PutObject writes object data to disk
- **THEN** the data is first written to a `.tmp`-suffixed file in the same directory, then renamed to the final path upon completion

#### Scenario: Incomplete write on crash leaves no corrupt file
- **WHEN** the process crashes during a PutObject write
- **THEN** no file exists at the canonical object path (only a `.tmp` file may remain, cleaned up on next startup)

### Requirement: Object files SHALL be deleted when objects are removed
The S3 service SHALL delete the backing file on disk when an object version is deleted or a bucket is deleted.

#### Scenario: DeleteObject removes the file
- **WHEN** a non-versioned object is deleted via DeleteObject
- **THEN** the backing file for that object is deleted from disk

#### Scenario: DeleteBucket removes all object files
- **WHEN** a bucket is deleted (after confirming it is empty per S3 semantics)
- **THEN** the bucket's object directory tree is removed from disk

#### Scenario: Version truncation cleans up old files
- **WHEN** a versioned object exceeds the 100-version cap
- **THEN** the backing files for truncated versions are deleted from disk

### Requirement: S3 multipart upload parts SHALL be stored on the filesystem
Multipart upload part data SHALL be stored on disk (not in memory) using the same filesystem storage pattern as objects. CompleteMultipartUpload SHALL concatenate parts by reading from disk and writing a single combined object file.

#### Scenario: UploadPart stores part data on disk
- **WHEN** an UploadPart request is processed
- **THEN** the part data is written to a file under the multipart upload's directory, not held as `Vec<u8>` in memory

#### Scenario: CompleteMultipartUpload concatenates from disk
- **WHEN** CompleteMultipartUpload is called with 5 parts
- **THEN** the parts are read sequentially from disk and written to the final object file, without loading all parts into memory simultaneously

#### Scenario: AbortMultipartUpload cleans up part files
- **WHEN** AbortMultipartUpload is called
- **THEN** all part files for that upload are deleted from disk
