## ADDED Requirements

### Requirement: S3 object storage emulation
The system SHALL emulate the Amazon S3 API including bucket operations (CreateBucket, DeleteBucket, ListBuckets, HeadBucket), object operations (PutObject, GetObject, DeleteObject, HeadObject, CopyObject, ListObjectsV2), multipart uploads (CreateMultipartUpload, UploadPart, CompleteMultipartUpload, AbortMultipartUpload), and bucket policies/ACLs.

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

### Requirement: SQS message queue emulation
The system SHALL emulate the Amazon SQS API including queue operations (CreateQueue, DeleteQueue, ListQueues, GetQueueUrl, GetQueueAttributes, SetQueueAttributes), message operations (SendMessage, ReceiveMessage, DeleteMessage, SendMessageBatch, ChangeMessageVisibility), and dead-letter queue support.

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

### Requirement: SNS notification emulation
The system SHALL emulate the Amazon SNS API including topic operations (CreateTopic, DeleteTopic, ListTopics), subscription operations (Subscribe, Unsubscribe, ListSubscriptions), and message publishing (Publish, PublishBatch). The system SHALL support SQS, HTTP/HTTPS, and Lambda subscription protocols.

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
The system SHALL emulate the Amazon DynamoDB API including table operations (CreateTable, DeleteTable, DescribeTable, ListTables, UpdateTable), item operations (PutItem, GetItem, DeleteItem, UpdateItem, Query, Scan, BatchGetItem, BatchWriteItem, TransactGetItems, TransactWriteItems), and secondary indexes (GSI, LSI).

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

### Requirement: DynamoDB Streams emulation
The system SHALL emulate DynamoDB Streams, providing a change data capture stream for DynamoDB table modifications. The system SHALL support `DescribeStream`, `GetRecords`, `GetShardIterator`, and `ListStreams` operations.

#### Scenario: Stream captures insert
- **WHEN** a table has streams enabled with `NEW_AND_OLD_IMAGES` and an item is inserted
- **THEN** `GetRecords` SHALL return a record with `eventName=INSERT` and the new item image

### Requirement: Lambda function emulation
The system SHALL emulate the AWS Lambda API including function management (CreateFunction, DeleteFunction, UpdateFunctionCode, UpdateFunctionConfiguration, GetFunction, ListFunctions) and invocation (Invoke synchronous and asynchronous). Lambda functions SHALL execute in Docker containers using AWS-compatible runtime images.

#### Scenario: Create and invoke a function
- **WHEN** a Lambda function is created with a Python 3.12 runtime and a zip deployment package, then invoked with payload `{"key": "value"}`
- **THEN** the function SHALL execute in a Docker container and return the function's response

#### Scenario: Environment variables
- **WHEN** a function is created with environment variables `{"FOO": "bar"}`
- **THEN** the function's execution environment SHALL have `FOO=bar` available

#### Scenario: Function timeout
- **WHEN** a function is invoked and exceeds its configured timeout
- **THEN** the invocation SHALL fail with a timeout error

#### Scenario: Hot reload
- **WHEN** a function's code package references the magic S3 bucket `hot-reload` (configurable via `BUCKET_MARKER_LOCAL`)
- **THEN** the function SHALL use the local file path for code, reflecting changes without redeployment

### Requirement: Kinesis data stream emulation
The system SHALL emulate the Amazon Kinesis API including stream management (CreateStream, DeleteStream, DescribeStream, ListStreams), shard operations (SplitShard, MergeShard), and data operations (PutRecord, PutRecords, GetRecords, GetShardIterator).

#### Scenario: Put and get records
- **WHEN** records are put into a Kinesis stream and a shard iterator is obtained with type `TRIM_HORIZON`
- **THEN** `GetRecords` SHALL return the records in the order they were put

### Requirement: Kinesis Firehose emulation
The system SHALL emulate the Amazon Kinesis Data Firehose API including delivery stream management (CreateDeliveryStream, DeleteDeliveryStream, DescribeDeliveryStream) and data ingestion (PutRecord, PutRecordBatch). The system SHALL support S3 as a destination.

#### Scenario: Deliver records to S3
- **WHEN** a Firehose delivery stream with S3 destination is created and records are put
- **THEN** the records SHALL be buffered and delivered to the configured S3 bucket
