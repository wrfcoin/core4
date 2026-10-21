/**
 * Smoke Tests - Service Health Checks
 *
 * Tests that validate running services are responding correctly.
 * These require services to be running and are skipped in CI
 * unless SMOKE_TEST_SERVICES=true is set.
 *
 * Run locally: SMOKE_TEST_SERVICES=true npx jest tests/smoke/service-health.test.ts
 */

import { describe, it, expect, beforeAll } from '@jest/globals';
import http from 'http';

const SKIP_SERVICES = process.env.SMOKE_TEST_SERVICES !== 'true';

interface HealthCheckResult {
  service: string;
  url: string;
  healthy: boolean;
  statusCode?: number;
  latencyMs: number;
  error?: string;
}

async function checkHealth(
  service: string,
  host: string,
  port: number,
  path: string = '/health',
  timeoutMs: number = 5000
): Promise<HealthCheckResult> {
  const url = `http://${host}:${port}${path}`;
  const start = Date.now();

  return new Promise((resolve) => {
    const req = http.get(url, { timeout: timeoutMs }, (res) => {
      const latencyMs = Date.now() - start;
      let body = '';
      res.on('data', (chunk) => (body += chunk));
      res.on('end', () => {
        resolve({
          service,
          url,
          healthy: res.statusCode !== undefined && res.statusCode >= 200 && res.statusCode < 500,
          statusCode: res.statusCode,
          latencyMs,
        });
      });
    });

    req.on('error', (err) => {
      resolve({
        service,
        url,
        healthy: false,
        latencyMs: Date.now() - start,
        error: err.message,
      });
    });

    req.on('timeout', () => {
      req.destroy();
      resolve({
        service,
        url,
        healthy: false,
        latencyMs: timeoutMs,
        error: 'Timeout',
      });
    });
  });
}

const describeServices = SKIP_SERVICES ? describe.skip : describe;

describeServices('Smoke Tests: Service Health (requires running services)', () => {
  const results: HealthCheckResult[] = [];

  beforeAll(async () => {
    const checks = [
      checkHealth('Main API', 'localhost', 4001),
      checkHealth('Wallet Server', 'localhost', 4000),
      checkHealth('Cross-Chain', 'localhost', 3000),
      checkHealth('Governance', 'localhost', 8000),
      checkHealth('Weather Earnings', 'localhost', 8001, '/health'),
    ];

    const settled = await Promise.all(checks);
    results.push(...settled);
  });

  it('should have Main API responding', () => {
    const api = results.find((r) => r.service === 'Main API');
    expect(api?.healthy).toBe(true);
    expect(api?.latencyMs).toBeLessThan(5000);
  });

  it('should have Wallet Server responding', () => {
    const wallet = results.find((r) => r.service === 'Wallet Server');
    expect(wallet?.healthy).toBe(true);
  });

  it('should have at least 3 core services healthy', () => {
    const healthyCount = results.filter((r) => r.healthy).length;
    expect(healthyCount).toBeGreaterThanOrEqual(3);
  });

  it('should have all services respond within 5 seconds', () => {
    for (const result of results.filter((r) => r.healthy)) {
      expect(result.latencyMs).toBeLessThan(5000);
    }
  });
});

describeServices('Smoke Tests: Database Connectivity (requires running services)', () => {
  it('should have PostgreSQL accessible', async () => {
    const result = await checkHealth('PostgreSQL', 'localhost', 5432, '/', 3000);
    // PostgreSQL doesn't have an HTTP health endpoint, so we check if the port is open
    // A connection error with "socket hang up" means the port is open (PG doesn't speak HTTP)
    const portOpen = result.healthy || result.error === 'socket hang up' || result.error?.includes('Parse Error');
    expect(portOpen).toBe(true);
  });

  it('should have Redis accessible', async () => {
    const result = await checkHealth('Redis', 'localhost', 6379, '/', 3000);
    // Similar to PostgreSQL - Redis on raw TCP will give a protocol error
    const portOpen = result.healthy || result.error === 'socket hang up' || result.error?.includes('Parse Error');
    expect(portOpen).toBe(true);
  });
});
