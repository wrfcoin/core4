/**
 * UI Verification Global Setup
 *
 * Prepares the test environment for UI verification:
 * - Creates output directories
 * - Clears previous test artifacts
 * - Verifies frontend server is running
 */
const fs = require('fs');
const path = require('path');
const http = require('http');

const OUTPUT_DIRS = [
  'screenshots',
  'traces',
  'reports',
  'accessibility',
  'reports/html-report',
  'reports/test-artifacts',
];

async function globalSetup(config) {
  console.log('\n🎨 UI Verification Setup Starting...\n');

  // Create output directories
  const baseDir = path.join(__dirname, '..');
  for (const dir of OUTPUT_DIRS) {
    const fullPath = path.join(baseDir, dir);
    if (!fs.existsSync(fullPath)) {
      fs.mkdirSync(fullPath, { recursive: true });
      console.log(`  ✓ Created directory: ${dir}`);
    }
  }

  // Clear previous screenshots (optional - keep for comparison)
  const screenshotsDir = path.join(baseDir, 'screenshots');
  const existingScreenshots = fs.readdirSync(screenshotsDir);
  if (existingScreenshots.length > 100) {
    // Only clear if too many accumulated
    console.log('  ⚠ Clearing old screenshots (>100 files)...');
    for (const file of existingScreenshots.slice(0, 50)) {
      fs.unlinkSync(path.join(screenshotsDir, file));
    }
  }

  // Verify frontend server is accessible
  const baseURL = config.projects?.[0]?.use?.baseURL || 'http://localhost:5000';
  const url = new URL(baseURL);

  console.log(`  🔍 Checking frontend server at ${baseURL}...`);

  try {
    await new Promise((resolve, reject) => {
      const req = http.get(baseURL, { timeout: 5000 }, (res) => {
        if (res.statusCode >= 200 && res.statusCode < 400) {
          resolve();
        } else {
          reject(new Error(`Server returned status ${res.statusCode}`));
        }
      });
      req.on('error', reject);
      req.on('timeout', () => reject(new Error('Connection timeout')));
    });
    console.log('  ✓ Frontend server is accessible\n');
  } catch (error) {
    console.warn(`  ⚠ Frontend server not responding: ${error.message}`);
    console.warn('  ℹ Tests will run but may fail if server is not started\n');
    console.warn('  ℹ Start the frontend with: cd frontend && npm run dev\n');
  }

  // Write test run metadata
  const metadata = {
    startTime: new Date().toISOString(),
    baseURL,
    nodeVersion: process.version,
    platform: process.platform,
  };

  fs.writeFileSync(
    path.join(baseDir, 'reports', 'test-run-metadata.json'),
    JSON.stringify(metadata, null, 2)
  );

  console.log('🎨 UI Verification Setup Complete\n');
}

module.exports = globalSetup;
