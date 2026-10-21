/**
 * Global Test Teardown
 * Runs once after all tests
 */

const { getTestDatabase } = require('./test-database');
const { getMockServiceManager } = require('./mock-services');
const { TEST_ENV, CI_MODE, REPORTING } = require('../test.config');
const fs = require('fs').promises;
const path = require('path');

module.exports = async function globalTeardown() {
  console.log('\n🛑 Starting Global Test Teardown...\n');

  try {
    // 1. Stop mock services
    if (TEST_ENV === 'local' || TEST_ENV === 'test') {
      const mockManager = getMockServiceManager();
      await mockManager.stopAll();
    }

    // 2. Clean up test database
    if (TEST_ENV === 'local' || TEST_ENV === 'test') {
      const db = getTestDatabase();

      // Only drop schema in CI mode or if explicitly requested
      if (CI_MODE || process.env.CLEAN_TEST_DB === 'true') {
        await db.dropSchema();
      } else {
        // Just clean data for local development
        await db.cleanData();
      }

      await db.disconnect();
    }

    // 3. Generate test summary report
    await generateTestSummary();

    // 4. Archive test artifacts if needed
    if (CI_MODE) {
      await archiveTestArtifacts();
    }

    // 5. Clean up temporary files
    await cleanupTempFiles();

    console.log('\n✅ Global Test Teardown Complete\n');
  } catch (error) {
    console.error('❌ Global Test Teardown Failed:', error);
    // Don't throw in teardown to avoid masking test failures
  }
};

/**
 * Generate test summary report
 */
async function generateTestSummary() {
  const reportDir = path.join(__dirname, '../reports');
  const summaryPath = path.join(reportDir, 'test-summary.json');

  try {
    // Collect all test results
    const results = {
      timestamp: new Date().toISOString(),
      environment: TEST_ENV,
      duration: process.env.TEST_DURATION || 'unknown',
      summary: {
        total: 0,
        passed: 0,
        failed: 0,
        skipped: 0,
      },
      testSuites: [],
      coverage: null,
    };

    // Read Jest results if available
    try {
      const jestResults = await fs.readFile(
        path.join(reportDir, 'jest-results.json'),
        'utf8'
      );
      const jestData = JSON.parse(jestResults);
      results.summary.total += jestData.numTotalTests;
      results.summary.passed += jestData.numPassedTests;
      results.summary.failed += jestData.numFailedTests;
      results.summary.skipped += jestData.numPendingTests;
      results.testSuites.push({
        name: 'Jest Tests',
        ...jestData,
      });
    } catch (error) {
      // Jest results not available
    }

    // Read Playwright results if available
    try {
      const playwrightResults = await fs.readFile(
        path.join(reportDir, 'playwright-results.json'),
        'utf8'
      );
      const playwrightData = JSON.parse(playwrightResults);
      results.testSuites.push({
        name: 'Playwright Tests',
        ...playwrightData,
      });
    } catch (error) {
      // Playwright results not available
    }

    // Read coverage summary if available
    try {
      const coverageSummary = await fs.readFile(
        path.join(reportDir, 'coverage/coverage-summary.json'),
        'utf8'
      );
      results.coverage = JSON.parse(coverageSummary);
    } catch (error) {
      // Coverage not available
    }

    // Write summary
    await fs.writeFile(summaryPath, JSON.stringify(results, null, 2));

    // Generate markdown summary
    await generateMarkdownSummary(results);

    console.log('✅ Test summary generated');
  } catch (error) {
    console.error('Failed to generate test summary:', error);
  }
}

/**
 * Generate markdown summary report
 */
async function generateMarkdownSummary(results) {
  const reportDir = path.join(__dirname, '../reports');
  const mdPath = path.join(reportDir, 'test-summary.md');

  let markdown = `# Test Summary Report

**Date**: ${new Date(results.timestamp).toLocaleString()}  
**Environment**: ${results.environment}  
**Duration**: ${results.duration}  

## Overall Results

| Metric | Count |
|--------|-------|
| Total Tests | ${results.summary.total} |
| Passed | ${results.summary.passed} |
| Failed | ${results.summary.failed} |
| Skipped | ${results.summary.skipped} |
| Pass Rate | ${results.summary.total > 0 ? ((results.summary.passed / results.summary.total) * 100).toFixed(2) : 0}% |

`;

  if (results.coverage) {
    markdown += `
## Code Coverage

| Type | Coverage |
|------|----------|
| Statements | ${results.coverage.total.statements.pct}% |
| Branches | ${results.coverage.total.branches.pct}% |
| Functions | ${results.coverage.total.functions.pct}% |
| Lines | ${results.coverage.total.lines.pct}% |

`;
  }

  if (results.testSuites.length > 0) {
    markdown += `## Test Suites

`;
    for (const suite of results.testSuites) {
      markdown += `### ${suite.name}

`;
      // Add suite-specific details
    }
  }

  await fs.writeFile(mdPath, markdown);
}

/**
 * Archive test artifacts for CI
 */
async function archiveTestArtifacts() {
  const artifactsDir = path.join(__dirname, '../artifacts');
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
  const archiveDir = path.join(artifactsDir, `archive-${timestamp}`);

  await fs.mkdir(archiveDir, { recursive: true });

  // Archive screenshots
  try {
    const screenshots = await fs.readdir(
      path.join(artifactsDir, 'screenshots')
    );
    if (screenshots.length > 0) {
      await fs.mkdir(path.join(archiveDir, 'screenshots'), { recursive: true });
      for (const file of screenshots) {
        await fs.rename(
          path.join(artifactsDir, 'screenshots', file),
          path.join(archiveDir, 'screenshots', file)
        );
      }
    }
  } catch (error) {
    // No screenshots to archive
  }

  // Archive videos
  try {
    const videos = await fs.readdir(path.join(artifactsDir, 'videos'));
    if (videos.length > 0) {
      await fs.mkdir(path.join(archiveDir, 'videos'), { recursive: true });
      for (const file of videos) {
        await fs.rename(
          path.join(artifactsDir, 'videos', file),
          path.join(archiveDir, 'videos', file)
        );
      }
    }
  } catch (error) {
    // No videos to archive
  }

  console.log(`✅ Test artifacts archived to ${archiveDir}`);
}

/**
 * Clean up temporary files
 */
async function cleanupTempFiles() {
  const tempDirs = ['/tmp/wrfcoin-test-*', path.join(__dirname, '../.tmp')];

  for (const pattern of tempDirs) {
    try {
      // Use glob pattern to find and remove temp files
      const { exec } = require('child_process');
      await new Promise((resolve, reject) => {
        exec(`rm -rf ${pattern}`, error => {
          if (error && error.code !== 1) reject(error);
          else resolve();
        });
      });
    } catch (error) {
      // Ignore cleanup errors
    }
  }

  console.log('✅ Temporary files cleaned up');
}
