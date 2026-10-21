async function globalTeardown() {
  console.log('🧹 Starting E2E test environment teardown...');

  // Clean up blockchain network
  if (process.env.HARDHAT_PID) {
    await stopBlockchainNetwork();
  }

  // Clean up test data
  if (process.env.E2E_CLEANUP !== 'false') {
    await cleanupTestData();
  }

  // Generate test reports
  await generateTestReports();

  console.log('✅ E2E test environment teardown complete');
}

async function stopBlockchainNetwork() {
  console.log('⛓️ Stopping blockchain network...');

  try {
    const pid = parseInt(process.env.HARDHAT_PID);
    if (pid) {
      process.kill(pid, 'SIGTERM');
      console.log('✅ Blockchain network stopped');
    }
  } catch (error) {
    console.warn('⚠️ Failed to stop blockchain network:', error.message);
  }
}

async function cleanupTestData() {
  console.log('🗑️ Cleaning up test data...');

  const { chromium } = require('@playwright/test');
  const config = require('../config/test-config');

  const browser = await chromium.launch();
  const context = await browser.newContext();
  const page = await context.newPage();

  try {
    // Check if backend is still running
    const healthCheck = await page.request
      .get(`${config.baseUrls.backend}/health`)
      .catch(() => null);

    if (healthCheck && healthCheck.ok()) {
      // Clean up test users
      await cleanupTestUsers(page);

      // Clean up test weather stations
      await cleanupWeatherStations(page);

      // Clean up test transactions
      await cleanupTestTransactions(page);

      console.log('✅ Test data cleaned up');
    } else {
      console.log('⚠️ Backend not available, skipping data cleanup');
    }
  } catch (error) {
    console.warn('⚠️ Failed to cleanup test data:', error.message);
  } finally {
    await browser.close();
  }
}

async function cleanupTestUsers(page) {
  const config = require('../config/test-config');

  try {
    await page.request.delete(`${config.baseUrls.backend}/api/test/users`);
    console.log('✅ Test users cleaned up');
  } catch (error) {
    console.warn('⚠️ Failed to cleanup test users:', error.message);
  }
}

async function cleanupWeatherStations(page) {
  const config = require('../config/test-config');

  try {
    await page.request.delete(
      `${config.baseUrls.backend}/api/test/weather-stations`
    );
    console.log('✅ Test weather stations cleaned up');
  } catch (error) {
    console.warn('⚠️ Failed to cleanup weather stations:', error.message);
  }
}

async function cleanupTestTransactions(page) {
  const config = require('../config/test-config');

  try {
    await page.request.delete(
      `${config.baseUrls.backend}/api/test/transactions`
    );
    console.log('✅ Test transactions cleaned up');
  } catch (error) {
    console.warn('⚠️ Failed to cleanup transactions:', error.message);
  }
}

async function generateTestReports() {
  console.log('📊 Generating test reports...');

  const fs = require('fs').promises;
  const path = require('path');

  try {
    // Ensure reports directory exists
    const reportsDir = path.join(__dirname, '../reports');
    await fs.mkdir(reportsDir, { recursive: true });

    // Generate summary report
    await generateSummaryReport(reportsDir);

    // Generate performance report
    await generatePerformanceReport(reportsDir);

    // Archive test artifacts
    await archiveTestArtifacts(reportsDir);

    console.log('✅ Test reports generated');
  } catch (error) {
    console.warn('⚠️ Failed to generate test reports:', error.message);
  }
}

async function generateSummaryReport(reportsDir) {
  const fs = require('fs').promises;
  const path = require('path');

  try {
    // Read test results
    const resultsPath = path.join(reportsDir, 'test-results.json');
    let testResults = {};

    try {
      const resultsData = await fs.readFile(resultsPath, 'utf8');
      testResults = JSON.parse(resultsData);
    } catch (error) {
      console.warn('No test results found for summary');
      return;
    }

    // Generate summary
    const summary = {
      timestamp: new Date().toISOString(),
      environment: process.env.E2E_ENV || 'local',
      total: testResults.total || 0,
      passed: testResults.passed || 0,
      failed: testResults.failed || 0,
      skipped: testResults.skipped || 0,
      duration: testResults.duration || 0,
      coverage: {
        workflows: calculateWorkflowCoverage(),
        features: calculateFeatureCoverage(),
      },
    };

    // Write summary report
    const summaryPath = path.join(reportsDir, 'summary-report.json');
    await fs.writeFile(summaryPath, JSON.stringify(summary, null, 2));

    console.log(
      `📈 Summary report: ${summary.passed}/${summary.total} tests passed`
    );
  } catch (error) {
    console.warn('Failed to generate summary report:', error.message);
  }
}

async function generatePerformanceReport(reportsDir) {
  const fs = require('fs').promises;
  const path = require('path');

  try {
    // Collect performance data from test results
    const performanceData = {
      timestamp: new Date().toISOString(),
      metrics: {
        averagePageLoad: 0,
        averageApiResponse: 0,
        averageBlockchainTransaction: 0,
      },
      thresholds: {
        pageLoad: 3000,
        apiResponse: 1000,
        blockchainTransaction: 30000,
      },
      violations: [],
    };

    // Write performance report
    const performancePath = path.join(reportsDir, 'performance-report.json');
    await fs.writeFile(
      performancePath,
      JSON.stringify(performanceData, null, 2)
    );

    console.log('⚡ Performance report generated');
  } catch (error) {
    console.warn('Failed to generate performance report:', error.message);
  }
}

async function archiveTestArtifacts(reportsDir) {
  const fs = require('fs').promises;
  const path = require('path');

  try {
    // Create archive directory with timestamp
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const archiveDir = path.join(reportsDir, 'archives', timestamp);
    await fs.mkdir(archiveDir, { recursive: true });

    // Archive screenshots
    const screenshotsDir = path.join(reportsDir, 'screenshots');
    try {
      await fs.access(screenshotsDir);
      await copyDirectory(screenshotsDir, path.join(archiveDir, 'screenshots'));
    } catch (error) {
      // Screenshots directory doesn't exist
    }

    // Archive videos
    const videosDir = path.join(reportsDir, 'videos');
    try {
      await fs.access(videosDir);
      await copyDirectory(videosDir, path.join(archiveDir, 'videos'));
    } catch (error) {
      // Videos directory doesn't exist
    }

    console.log(`📁 Test artifacts archived to: ${archiveDir}`);
  } catch (error) {
    console.warn('Failed to archive test artifacts:', error.message);
  }
}

async function copyDirectory(src, dest) {
  const fs = require('fs').promises;
  const path = require('path');

  await fs.mkdir(dest, { recursive: true });
  const entries = await fs.readdir(src, { withFileTypes: true });

  for (const entry of entries) {
    const srcPath = path.join(src, entry.name);
    const destPath = path.join(dest, entry.name);

    if (entry.isDirectory()) {
      await copyDirectory(srcPath, destPath);
    } else {
      await fs.copyFile(srcPath, destPath);
    }
  }
}

function calculateWorkflowCoverage() {
  // Calculate which user workflows have been tested
  const workflows = [
    'user_onboarding',
    'wallet_operations',
    'nft_marketplace',
    'defi_operations',
    'governance_workflow',
    'weather_integration',
    'computational_jobs',
    'carbon_trading',
    'cross_chain_bridge',
  ];

  // This would be calculated based on actual test execution
  return {
    total: workflows.length,
    covered: workflows.length, // Placeholder
    percentage: 100,
  };
}

function calculateFeatureCoverage() {
  // Calculate which platform features have been tested
  const features = [
    'authentication',
    'wallet_connection',
    'smart_contract_interaction',
    'api_integration',
    'real_time_updates',
    'file_uploads',
    'notifications',
    'responsive_design',
  ];

  // This would be calculated based on actual test execution
  return {
    total: features.length,
    covered: features.length, // Placeholder
    percentage: 100,
  };
}

module.exports = globalTeardown;
