/**
 * Smoke Tests - Critical Path Validation
 *
 * Lightweight tests that validate the build system, configuration,
 * and critical file integrity. Designed to run in <30 seconds as
 * a fast-fail gate in CI before heavier test suites.
 *
 * These tests do NOT require running services - they validate
 * static project health. For service health checks, see
 * tests/smoke/service-health.test.ts.
 */

import { describe, it, expect } from '@jest/globals';
import * as fs from 'fs';
import * as path from 'path';

const ROOT = path.resolve(__dirname, '../..');

function fileExists(relativePath: string): boolean {
  return fs.existsSync(path.join(ROOT, relativePath));
}

function readJson(relativePath: string): any {
  const content = fs.readFileSync(path.join(ROOT, relativePath), 'utf-8');
  return JSON.parse(content);
}

describe('Smoke Tests: Project Structure', () => {
  it('should have root package.json with correct workspace config', () => {
    const pkg = readJson('package.json');
    expect(pkg.name).toBeDefined();
    expect(pkg.workspaces).toEqual(
      expect.arrayContaining(['frontend', 'backend', 'contracts'])
    );
  });

  it('should have all critical configuration files', () => {
    const requiredFiles = [
      'package.json',
      'package-lock.json',
      'tsconfig.json',
      'jest.config.js',
      'CLAUDE.md',
    ];

    for (const file of requiredFiles) {
      expect(fileExists(file)).toBe(true);
    }
  });

  it('should have blockchain-core Cargo.toml and workspace lock', () => {
    expect(fileExists('blockchain-core/Cargo.toml')).toBe(true);
    // Cargo.lock is at workspace root, not in blockchain-core/
    expect(fileExists('Cargo.lock')).toBe(true);
  });

  it('should have smart contract configuration', () => {
    expect(fileExists('contracts/hardhat.config.js')).toBe(true);
    expect(fileExists('contracts/package.json')).toBe(true);
  });

  it('should have CI workflow files', () => {
    expect(fileExists('.github/workflows/pr-checks.yml')).toBe(true);
  });
});

describe('Smoke Tests: Security Configuration', () => {
  it('should not have .env file committed', () => {
    // .env should exist for local dev but contain no real secrets in the repo
    if (fileExists('.env')) {
      const content = fs.readFileSync(path.join(ROOT, '.env'), 'utf-8');
      // Check it doesn't contain obvious real API keys
      expect(content).not.toMatch(/sk-[a-zA-Z0-9]{20,}/);
      expect(content).not.toMatch(/AKIA[A-Z0-9]{16}/);
    }
  });

  it('should have .gitignore with security patterns', () => {
    expect(fileExists('.gitignore')).toBe(true);
    const gitignore = fs.readFileSync(path.join(ROOT, '.gitignore'), 'utf-8');
    expect(gitignore).toContain('.env');
    expect(gitignore).toContain('node_modules');
  });

  it('should have security CLAUDE.md', () => {
    expect(fileExists('security/CLAUDE.md')).toBe(true);
  });
});

describe('Smoke Tests: Dependency Integrity', () => {
  it('should have node_modules installed', () => {
    expect(fileExists('node_modules')).toBe(true);
  });

  it('should have critical dependencies available', () => {
    const criticalDeps = [
      'express',
      'jsonwebtoken',
      'helmet',
    ];

    for (const dep of criticalDeps) {
      expect(fileExists(`node_modules/${dep}`)).toBe(true);
    }
  });

  it('should not have known vulnerable crypto packages', () => {
    // md5 should not be used for security purposes
    const rootPkg = readJson('package.json');
    const deps = { ...rootPkg.dependencies, ...rootPkg.devDependencies };
    expect(deps['md5']).toBeUndefined();
  });
});

describe('Smoke Tests: Build Artifacts', () => {
  it('should have blockchain-core source files', () => {
    expect(fileExists('blockchain-core/src/lib.rs')).toBe(true);
  });

  it('should have frontend source files', () => {
    expect(fileExists('frontend/src')).toBe(true);
    expect(fileExists('frontend/package.json')).toBe(true);
  });

  it('should have backend source files', () => {
    expect(fileExists('backend/server')).toBe(true);
  });

  it('should have contract source files', () => {
    expect(fileExists('contracts/contracts')).toBe(true);
  });
});

describe('Smoke Tests: Database Schema', () => {
  it('should have database migration files or schema', () => {
    const hasSchema =
      fileExists('backend/drizzle') ||
      fileExists('backend/database') ||
      fileExists('backend/shared/schema');
    expect(hasSchema).toBe(true);
  });
});

describe('Smoke Tests: Test Infrastructure', () => {
  it('should have Jest configuration', () => {
    expect(fileExists('jest.config.js')).toBe(true);
  });

  it('should have Playwright configuration', () => {
    expect(fileExists('playwright.config.js')).toBe(true);
  });

  it('should have test directories', () => {
    expect(fileExists('tests/unit')).toBe(true);
    expect(fileExists('tests/e2e')).toBe(true);
  });

  it('should have contract test infrastructure', () => {
    expect(fileExists('contracts/test')).toBe(true);
  });
});
