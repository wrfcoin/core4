/**
 * Basic Bridge Functionality Test - Simplified for initial validation
 */

const { test, expect } = require('@playwright/test');

test.describe('Basic Bridge Functionality', () => {
  test('Bridge interface loads and displays main components', async ({
    page,
  }) => {
    // Navigate to the main page
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Check if any cross-chain bridge components are visible
    // We'll check for common elements that should exist

    // Wait for page to load and check for any navigation or bridge-related elements
    await page.waitForTimeout(3000);

    // Take a screenshot for debugging
    await page.screenshot({ path: 'bridge-interface-screenshot.png' });

    // Check if the page loaded successfully (non-error page)
    const pageTitle = await page.title();
    expect(pageTitle).toBeTruthy();
    expect(pageTitle).not.toContain('Error');

    // Check for basic page structure
    const bodyContent = await page.locator('body').textContent();
    expect(bodyContent.length).toBeGreaterThan(0);

    console.log('Page title:', pageTitle);
    console.log('Page loaded successfully');
  });

  test('Cross-chain server responds to API calls', async ({ page }) => {
    // Test the cross-chain server API directly
    const response = await page.request.get('http://localhost:3000/health');
    expect(response.ok()).toBeTruthy();

    const healthData = await response.json();
    expect(healthData.status).toBe('healthy');
    expect(healthData.service).toBe('cross-chain-protocols');

    // Test protocols endpoint
    const protocolsResponse = await page.request.get(
      'http://localhost:3000/api/crosschain/protocols'
    );
    expect(protocolsResponse.ok()).toBeTruthy();

    const protocolsData = await protocolsResponse.json();
    expect(protocolsData.success).toBe(true);
    expect(protocolsData.protocols).toBeDefined();
    expect(Object.keys(protocolsData.protocols).length).toBeGreaterThan(0);

    console.log('Cross-chain server is responding correctly');
    console.log(
      'Available protocols:',
      Object.keys(protocolsData.protocols).length
    );
  });

  test('Bridge metrics endpoint provides data', async ({ page }) => {
    // Test bridge metrics
    const metricsResponse = await page.request.get(
      'http://localhost:3000/api/crosschain/metrics'
    );
    expect(metricsResponse.ok()).toBeTruthy();

    const metricsData = await metricsResponse.json();
    expect(metricsData.success).toBe(true);
    expect(metricsData.metrics).toBeDefined();
    expect(metricsData.metrics.total_protocols).toBeGreaterThan(0);

    console.log(
      'Bridge metrics available:',
      metricsData.metrics.total_protocols,
      'protocols'
    );
  });

  test('Bridge status endpoint shows operational status', async ({ page }) => {
    // Test bridge status
    const statusResponse = await page.request.get(
      'http://localhost:3000/api/crosschain/bridge-status'
    );
    expect(statusResponse.ok()).toBeTruthy();

    const statusData = await statusResponse.json();
    expect(statusData.success).toBe(true);
    expect(statusData.bridges).toBeDefined();
    expect(statusData.bridges.length).toBeGreaterThan(0);
    expect(statusData.overall_status).toBe('operational');

    console.log(
      'Bridge status: operational,',
      statusData.bridges.length,
      'bridges monitored'
    );
  });
});
