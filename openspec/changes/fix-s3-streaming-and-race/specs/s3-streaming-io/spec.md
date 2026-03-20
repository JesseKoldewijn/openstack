## ADDED Requirements

### Requirement: S3 PutObject and UploadPart SHALL use a HashingReader to compute ETag incrementally
The S3 PutObject and UploadPart operations SHALL compute the MD5 ETag by wrapping the `SpooledBody` async reader in a `HashingReader` adapter that feeds each chunk to an MD5 digest as it is written to disk. The full body SHALL NOT be buffered in memory solely for the purpose of hashing. After the streaming write completes, the finalized digest SHALL be used as the object's ETag.

#### Scenario: ETag matches full-body hash
- **WHEN** a PutObject streams a 50 MiB body to disk via HashingReader
- **THEN** the ETag returned in the response is identical to the hex-encoded MD5 of the complete body, and matches what a client would compute by hashing the body before sending

#### Scenario: HashingReader adds negligible memory overhead
- **WHEN** a PutObject streams a 100 MiB body
- **THEN** the `HashingReader` holds only the current digest state (a fixed-size struct, independent of body size) plus the stream buffer (default 64 KiB)

#### Scenario: UploadPart ETag is used in CompleteMultipartUpload validation
- **WHEN** an UploadPart streams its body via HashingReader and CompleteMultipartUpload references that part
- **THEN** the stored part ETag matches the incrementally computed hash, and CompleteMultipartUpload validation succeeds
