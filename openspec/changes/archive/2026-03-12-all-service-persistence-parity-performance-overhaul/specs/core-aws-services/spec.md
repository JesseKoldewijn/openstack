## MODIFIED Requirements

### Requirement: S3 object storage emulation
The system SHALL emulate the Amazon S3 API including bucket operations (CreateBucket, DeleteBucket, ListBuckets, HeadBucket), object operations (PutObject, GetObject, DeleteObject, HeadObject, CopyObject, ListObjectsV2), multipart uploads (CreateMultipartUpload, UploadPart, CompleteMultipartUpload, AbortMultipartUpload), and bucket policies/ACLs. S3 emulation SHALL meet required-lane performance and resource envelopes while preserving functional parity, and SHALL support parity-declared persistence behavior in durable modes.

#### Scenario: Create and use a bucket
- **WHEN** a client calls `CreateBucket` with bucket name `my-bucket`, then `PutObject` with key `test.txt` and body `hello`
- **THEN** a subsequent `GetObject` for `my-bucket/test.txt` SHALL return body `hello`

#### Scenario: List objects with prefix
- **WHEN** a bucket contains keys `a/1.txt`, `a/2.txt`, `b/1.txt` and `ListObjectsV2` is called with prefix `a/`
- **THEN** the response SHALL contain exactly `a/1.txt` and `a/2.txt`

#### Scenario: Multipart upload
- **WHEN** a client creates a multipart upload, uploads 2 parts, and completes the upload
- **THEN** `GetObject` SHALL return the concatenated content of both parts

#### Scenario: Pre-signed URL access
- **WHEN** a client generates a pre-signed URL for `GetObject` and makes an HTTP GET to that URL
- **THEN** the gateway SHALL serve the object without requiring an Authorization header

#### Scenario: Durable mode survives restart
- **WHEN** S3 state is created in a declared durable mode and the runtime restarts
- **THEN** bucket and object visibility after restart SHALL remain parity-consistent with declared durability semantics

### Requirement: SQS message queue emulation
The system SHALL emulate the Amazon SQS API including queue operations (CreateQueue, DeleteQueue, ListQueues, GetQueueUrl, GetQueueAttributes, SetQueueAttributes), message operations (SendMessage, ReceiveMessage, DeleteMessage, SendMessageBatch, ChangeMessageVisibility), and dead-letter queue support. SQS emulation SHALL satisfy required-lane latency/throughput/resource envelopes and persistence parity semantics in declared durable modes.

#### Scenario: Send and receive a message
- **WHEN** a client creates a queue, sends a message with body `test`, then calls `ReceiveMessage`
- **THEN** the received message SHALL have body `test` and a valid receipt handle

#### Scenario: Message visibility timeout
- **WHEN** a message is received with visibility timeout 5 seconds and not deleted
- **THEN** the message SHALL become visible again after 5 seconds

#### Scenario: Dead-letter queue
- **WHEN** a queue has a redrive policy with `maxReceiveCount=2` and a message is received 3 times without deletion
- **THEN** the message SHALL be moved to the configured dead-letter queue

#### Scenario: FIFO queue ordering
- **WHEN** messages are sent to a FIFO queue (`.fifo` suffix) with the same message group ID
- **THEN** `ReceiveMessage` SHALL return them in the order they were sent

#### Scenario: Queue state survives durable restart
- **WHEN** queue and message state exist in a declared durable mode and the runtime restarts
- **THEN** queue existence and eligible message visibility SHALL remain parity-consistent after restart

### Requirement: SNS notification emulation
The system SHALL emulate the Amazon SNS API including topic operations (CreateTopic, DeleteTopic, ListTopics), subscription operations (Subscribe, Unsubscribe, ListSubscriptions), and message publishing (Publish, PublishBatch). The system SHALL support SQS, HTTP/HTTPS, and Lambda subscription protocols. SNS emulation SHALL maintain parity behavior while meeting class-specific performance/resource envelopes.

#### Scenario: Publish to SQS subscriber
- **WHEN** an SQS queue is subscribed to an SNS topic and a message is published to the topic
- **THEN** the message SHALL be delivered to the SQS queue as an SNS notification envelope

#### Scenario: Message filtering
- **WHEN** a subscription has a filter policy on message attributes and a message is published that does not match
- **THEN** the message SHALL NOT be delivered to that subscription

#### Scenario: HTTP endpoint subscription
- **WHEN** an HTTP endpoint is subscribed to a topic and a message is published
- **THEN** the system SHALL POST the notification to the HTTP endpoint

### Requirement: DynamoDB table emulation
The system SHALL emulate the Amazon DynamoDB API including table operations (CreateTable, DeleteTable, DescribeTable, ListTables, UpdateTable), item operations (PutItem, GetItem, DeleteItem, UpdateItem, Query, Scan, BatchGetItem, BatchWriteItem, TransactGetItems, TransactWriteItems), and secondary indexes (GSI, LSI). DynamoDB emulation SHALL preserve parity semantics while satisfying class-specific performance/resource envelopes and declared persistence durability semantics.

#### Scenario: CRUD operations
- **WHEN** a table with partition key `pk` is created and an item `{pk: "1", data: "hello"}` is put
- **THEN** `GetItem` with key `{pk: "1"}` SHALL return `{pk: "1", data: "hello"}`

#### Scenario: Query with key condition
- **WHEN** a table has partition key `pk` and sort key `sk`, and items exist with `pk=A,sk=1`, `pk=A,sk=2`, `pk=B,sk=1`
- **THEN** `Query` with `pk=A` SHALL return exactly the two items with `pk=A`

#### Scenario: Global secondary index
- **WHEN** a table has a GSI on attribute `email` and a query is run against the GSI
- **THEN** the query SHALL return items matching the GSI key condition

#### Scenario: Conditional write failure
- **WHEN** `PutItem` is called with a condition expression `attribute_not_exists(pk)` and the item already exists
- **THEN** the operation SHALL fail with `ConditionalCheckFailedException`

#### Scenario: Durable table state survives restart
- **WHEN** table and item state are created in a declared durable mode and the runtime restarts
- **THEN** table metadata and item visibility SHALL remain parity-consistent after restart
