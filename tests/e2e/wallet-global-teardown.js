/**
 * Wallet Testing Global Teardown
 * Cleanup after wallet functionality testing
 *
 * Task Master Subtask 8.1: Set up Playwright Environment and Test Fixtures
 * Created: 2025-06-23
 */

const path = require('path');
const fs = require('fs');

async function globalTeardown(config) {
  console.log('🧹 Starting Wallet Testing Global Teardown...');

  try {
    const testResultsDir = path.join(__dirname, 'wallet-test-results');

    // Generate test summary
    const summaryFile = path.join(testResultsDir, 'test-summary.json');
    const testSummary = {
      completedAt: new Date().toISOString(),
      environment: {
        nodeVersion: process.version,
        platform: process.platform,
        arch: process.arch,
      },
      testCategories: [
        'Wallet Creation and Import',
        'Multi-Asset Holdings',
        'Transfer Functionality',
        'Transaction History',
        'Backup and Restore',
        'Responsive Design',
        'Security and Error Handling',
      ],
      resultsLocation: testResultsDir,
    };

    fs.writeFileSync(summaryFile, JSON.stringify(testSummary, null, 2));
    console.log('✅ Test summary generated');

    // Log cleanup status
    console.log('📊 Cleanup Summary:');
    console.log(`   • Test results preserved in: ${testResultsDir}`);
    console.log(`   • Test summary: ${summaryFile}`);

    // Check for failure artifacts
    const screenshotsDir = path.join(testResultsDir, 'screenshots');
    if (fs.existsSync(screenshotsDir)) {
      const screenshots = fs.readdirSync(screenshotsDir);
      const failureScreenshots = screenshots.filter(file =>
        file.includes('failure')
      );

      if (failureScreenshots.length > 0) {
        console.log(
          `📸 ${failureScreenshots.length} failure screenshots available for debugging`
        );
      }
    }

    console.log('✅ Wallet Testing Global Teardown Complete!');
  } catch (error) {
    console.error('❌ Global teardown failed:', error);
    // Don't throw error to avoid masking test failures
  }
}

module.exports = globalTeardown;
