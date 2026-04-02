import { test, expect } from '@playwright/test';

test.describe('Studio Guided Workflows', () => {
  const STUDIO_URL = process.env.STUDIO_URL || 'http://localhost:4566/_localstack/studio';

  test('should execute S3 basic bucket creation and upload', async ({ page }) => {
    await page.goto(STUDIO_URL);
    
    // Select S3
    await page.click('.svc-card:has-text("S3")');
    
    // Should see guided flows on Overview tab
    await expect(page.locator('.panel-section-title:has-text("Guided interaction")')).toBeVisible();

    // Select the flow (it uses sub-tabs with flow ID)
    await page.click('.sub-tab:has-text("l1-basic")');
    
    // Fill in a unique bucket name so this test never conflicts with a prior run
    await page.fill('input[data-input="bucket_name"]', `e2e-bucket-${Date.now()}`);
    
    // Execute flow
    await page.click('button:has-text("Run flow")');
    // Expect some visual feedback of progress/success
    await expect(page.locator('.step-ok:has-text("Create bucket")')).toBeVisible({ timeout: 15000 });
  });

  test('should execute SQS basic queue flow', async ({ page }) => {
    await page.goto(STUDIO_URL);
    
    // Select SQS
    await page.click('.svc-card:has-text("SQS")');
    
    // Select the guided flow
    await page.click('.sub-tab:has-text("l1-basic")');
    
    // Set queue name
    await page.fill('input[data-input="queue_name"]', 'test-e2e-queue');
    
    // Execute flow
    await page.click('button:has-text("Run flow")');
    await expect(page.locator('.step-ok:has-text("Create queue")')).toBeVisible({ timeout: 15000 });
  });
});
