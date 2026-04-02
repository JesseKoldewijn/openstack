import { test, expect } from '@playwright/test';

test.describe('Studio UI Service Explorer', () => {
  const STUDIO_URL = process.env.STUDIO_URL || 'http://localhost:4566/_localstack/studio';

  test('should load the service list', async ({ page }) => {
    await page.goto(STUDIO_URL);
    
    // Check for common services in the sidebar/explorer
    // Using class selectors based on the revealed HTML template
    await expect(page.locator('.svc-card:has-text("S3")')).toBeVisible();
    await expect(page.locator('.svc-card:has-text("SQS")')).toBeVisible();
    await expect(page.locator('.svc-card:has-text("DynamoDB")')).toBeVisible();
  });

  test('should navigate to S3 and show bucket operations', async ({ page }) => {
    await page.goto(STUDIO_URL);
    
    // Click S3 service card
    await page.click('.svc-card:has-text("S3")');
    
    // Click Operations tab
    await page.click('.tab:has-text("Operations")');
    
    // Should show operations list (it has a search input with placeholder)
    await expect(page.locator('input[placeholder="Search operations…"]')).toBeVisible();
  });

  test('should show transaction log history', async ({ page }) => {
    await page.goto(STUDIO_URL);
    
    // Select a service first to see its detail
    await page.click('.svc-card:has-text("S3")');
    
    // Click Transactions tab
    await page.click('.tab:has-text("Transactions")');
    
    // Should show the transactions panel
    await expect(page.locator('.tab--active:has-text("Transactions")')).toBeVisible();
  });
});
