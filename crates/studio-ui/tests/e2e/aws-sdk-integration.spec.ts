import { test, expect } from '@playwright/test';
import { S3Client, CreateBucketCommand, PutObjectCommand } from '@aws-sdk/client-s3';
import { SQSClient, CreateQueueCommand, SendMessageCommand } from '@aws-sdk/client-sqs';
import { DynamoDBClient, CreateTableCommand, ListTablesCommand } from '@aws-sdk/client-dynamodb';

/**
 * These tests use the actual AWS SDK for JavaScript to interact with openstack
 * while the Studio UI is running, then verify the Studio UI correctly shows
 * the resulting state and transactions.
 */
test.describe('Studio UI Verification with AWS SDK', () => {
  const STUDIO_URL = process.env.STUDIO_URL || 'http://localhost:4566/_localstack/studio';
  const AWS_ENDPOINT = 'http://localhost:4566';
  const REGION = 'us-east-1';

  // Helper to create SDK clients pointing to the local gateway
  // forcePathStyle is required — the gateway expects /bucket/key paths, not
  // virtual-hosted style (bucket.localhost:4566).
  const s3 = new S3Client({
    endpoint: AWS_ENDPOINT,
    region: REGION,
    credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
    forcePathStyle: true,
  });
  const sqs = new SQSClient({ endpoint: AWS_ENDPOINT, region: REGION, credentials: { accessKeyId: 'test', secretAccessKey: 'test' } });
  const ddb = new DynamoDBClient({ endpoint: AWS_ENDPOINT, region: REGION, credentials: { accessKeyId: 'test', secretAccessKey: 'test' } });

  test('should reflect S3 bucket creation in Studio explorer', async ({ page }) => {
    const bucketName = `sdk-bucket-${Date.now()}`;
    
    // 1. Create bucket via SDK
    await s3.send(new CreateBucketCommand({ Bucket: bucketName }));
    
    // 2. Open Studio
    await page.goto(STUDIO_URL);
    await page.click('.svc-card:has-text("S3")');
    
    // 3. Check Storage tab
    await page.click('.tab:has-text("Storage")');
    await expect(page.locator('.resource-row .resource-id', { hasText: bucketName })).toBeVisible({ timeout: 10000 });
    
    // 4. Check Transactions tab — CreateBucket should be recorded
    await page.click('.tab:has-text("Transactions")');
    await expect(page.locator('code.tx-op:has-text("CreateBucket")').first()).toBeVisible();
  });

  test('should reflect SQS message operations in Studio history', async ({ page }) => {
    const queueName = `sdk-queue-${Date.now()}`;
    
    // 1. SDK interactions
    const { QueueUrl } = await sqs.send(new CreateQueueCommand({ QueueName: queueName }));
    await sqs.send(new SendMessageCommand({ QueueUrl, MessageBody: 'hello from SDK' }));
    
    // 2. Open Studio
    await page.goto(STUDIO_URL);
    await page.click('.svc-card:has-text("SQS")');
    
    // 3. Verify transactions recorded
    await page.click('.tab:has-text("Transactions")');
    await expect(page.locator('code.tx-op:has-text("CreateQueue")').first()).toBeVisible();
    await expect(page.locator('code.tx-op:has-text("SendMessage")').first()).toBeVisible();
  });

  test('should reflect DynamoDB table structure in Storage tab', async ({ page }) => {
    const tableName = `sdk-table-${Date.now()}`;
    
    // 1. Create table via SDK
    await ddb.send(new CreateTableCommand({
      TableName: tableName,
      AttributeDefinitions: [{ AttributeName: 'id', AttributeType: 'S' }],
      KeySchema: [{ AttributeName: 'id', KeyType: 'HASH' }],
      ProvisionedThroughput: { ReadCapacityUnits: 1, WriteCapacityUnits: 1 }
    }));
    
    // 2. Open Studio
    await page.goto(STUDIO_URL);
    await page.click('.svc-card:has-text("DynamoDB")');
    
    // 3. Verify Storage shows the table (match on attr-val span which has the exact name)
    await page.click('.tab:has-text("Storage")');
    await expect(page.locator('span.attr-val', { hasText: tableName })).toBeVisible({ timeout: 10000 });
  });
});
