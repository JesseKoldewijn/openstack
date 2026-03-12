## ADDED Requirements

### Requirement: S3 PutObject SHALL stream request bodies to disk
The S3 PutObject operation SHALL stream the incoming request body directly to disk via the `SpooledBody` async reader, computing the MD5 ETag incrementally during the write. The full body SHALL NOT be buffered in memory.

#### Scenario: Large PutObject streams to disk
- **WHEN** a PutObject request arrives with a 500 MiB body
- **THEN** the body is streamed to a temporary file on disk with constant memory usage (proportional to the stream buffer size, not the body size), and the ETag is computed during streaming

#### Scenario: ETag is computed incrementally
- **WHEN** a PutObject streams data to disk
- **THEN** the MD5 hash is computed incrementally as chunks are written, and the resulting ETag matches what would be produced by hashing the complete body

#### Scenario: Small PutObject uses buffered path
- **WHEN** a PutObject request arrives with a 100 byte body
- **THEN** the body MAY be processed via the in-memory buffer path of the `SpooledBody` without filesystem streaming overhead

### Requirement: S3 UploadPart SHALL stream request bodies to disk
The S3 UploadPart operation SHALL stream the incoming request body directly to a part file on disk, computing the ETag incrementally.

#### Scenario: UploadPart streams to disk
- **WHEN** an UploadPart request arrives with a 100 MiB part body
- **THEN** the part body is streamed to disk with constant memory overhead

### Requirement: S3 GetObject SHALL stream response bodies from disk
The S3 GetObject operation SHALL return a streaming response body that reads the object file from disk in chunks, rather than loading the entire file into memory.

#### Scenario: Large GetObject streams from disk
- **WHEN** a GetObject request is processed for a 1 GiB object
- **THEN** the response body is streamed from disk in chunks (default 64 KiB), and peak memory usage for the response is proportional to the chunk size, not the object size

#### Scenario: Content-Length header is set from stored metadata
- **WHEN** a GetObject streams a response
- **THEN** the `Content-Length` header is set to the stored `ObjectVersion.size` value, enabling clients to track download progress

#### Scenario: Streaming GetObject returns correct data
- **WHEN** a GetObject response is streamed for an object that was previously stored via PutObject
- **THEN** the complete streamed body is byte-for-byte identical to the original PutObject body

### Requirement: S3 CopyObject SHALL copy between filesystem paths
The S3 CopyObject operation SHALL copy the source object's file to the destination path on disk without loading the full object into memory.

#### Scenario: CopyObject copies file on disk
- **WHEN** CopyObject copies a 500 MiB object from bucket-a/key-1 to bucket-b/key-2
- **THEN** the file is copied on disk (using filesystem copy, not memory buffering), and the destination object has the correct metadata and a new version ID if versioning is enabled
