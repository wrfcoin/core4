import { test, expect } from '@playwright/test';

test('debug dashboard loading', async ({ page }) => {
  console.log('Navigating to dashboard...');
  await page.goto('http://localhost:5000');

  console.log('Taking screenshot...');
  await page.screenshot({ path: 'debug-dashboard.png' });

  console.log('Getting page content...');
  const content = await page.content();
  console.log('Page content length:', content.length);

  console.log('Waiting for React app to load...');
  await page.waitForSelector('#root', { timeout: 30000 });

  console.log('Checking for any elements...');
  const elements = await page.locator('*').count();
  console.log('Total elements on page:', elements);

  console.log('Looking for dashboard class...');
  const dashboardElements = await page.locator('.dashboard').count();
  console.log('Dashboard elements found:', dashboardElements);

  console.log('Looking for any text containing "Hash"...');
  const hashElements = await page.locator('text=/Hash/').count();
  console.log('Hash-related elements found:', hashElements);

  console.log('Taking final screenshot...');
  await page.screenshot({ path: 'debug-dashboard-final.png' });
});
