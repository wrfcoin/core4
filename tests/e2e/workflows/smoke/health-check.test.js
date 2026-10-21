// @smoke
const { test, expect } = require('@playwright/test');

test.describe('Health Check @smoke', () => {
  test('should respond to health check', async ({ request }) => {
    const response = await request.get('/health');

    expect(response.status()).toBe(200);

    const body = await response.json();
    expect(body).toHaveProperty('status', 'ok');
    expect(body).toHaveProperty('server', 'wrfcoin-backend');
  });

  test('should respond to API base endpoint', async ({ request }) => {
    const response = await request.get('/api/');

    expect(response.status()).toBe(200);

    const body = await response.json();
    expect(body).toHaveProperty('status', 'success');
    expect(body).toHaveProperty('message');
  });
});
