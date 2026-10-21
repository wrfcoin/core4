/**
 * UI Verification Global Teardown
 *
 * Cleanup and reporting after test execution:
 * - Generate summary report
 * - Compile accessibility findings
 * - Archive traces if needed
 */
const fs = require('fs');
const path = require('path');

async function globalTeardown(config) {
  console.log('\n🎨 UI Verification Teardown Starting...\n');

  const baseDir = path.join(__dirname, '..');
  const reportsDir = path.join(baseDir, 'reports');
  const accessibilityDir = path.join(baseDir, 'accessibility');
  const screenshotsDir = path.join(baseDir, 'screenshots');

  // Collect all accessibility reports
  const accessibilityFiles = fs.existsSync(accessibilityDir)
    ? fs.readdirSync(accessibilityDir).filter(f => f.endsWith('.json'))
    : [];

  // Aggregate accessibility findings
  const allViolations = [];
  const allWarnings = [];
  let totalPasses = 0;

  for (const file of accessibilityFiles) {
    try {
      const content = JSON.parse(
        fs.readFileSync(path.join(accessibilityDir, file), 'utf8')
      );
      if (content.accessibility) {
        allViolations.push(...(content.accessibility.violations || []));
        allWarnings.push(...(content.accessibility.warnings || []));
        totalPasses += content.accessibility.passes || 0;
      }
    } catch (e) {
      // Skip invalid files
    }
  }

  // Count screenshots
  const screenshotCount = fs.existsSync(screenshotsDir)
    ? fs.readdirSync(screenshotsDir).filter(f => f.endsWith('.png')).length
    : 0;

  // Generate summary report
  const summary = {
    completedAt: new Date().toISOString(),
    screenshots: screenshotCount,
    accessibility: {
      totalViolations: allViolations.length,
      totalWarnings: allWarnings.length,
      totalPasses,
      criticalViolations: allViolations.filter(v => v.impact === 'critical').length,
      seriousViolations: allViolations.filter(v => v.impact === 'serious').length,
    },
    reports: accessibilityFiles.length,
  };

  // Write summary
  fs.writeFileSync(
    path.join(reportsDir, 'ui-verification-summary.json'),
    JSON.stringify(summary, null, 2)
  );

  // Console output
  console.log('  📊 UI Verification Summary:');
  console.log(`     Screenshots captured: ${screenshotCount}`);
  console.log(`     Accessibility reports: ${accessibilityFiles.length}`);
  console.log(`     Accessibility violations: ${allViolations.length}`);
  if (summary.accessibility.criticalViolations > 0) {
    console.log(`     ⚠ Critical violations: ${summary.accessibility.criticalViolations}`);
  }
  console.log('');

  // If there are critical violations, generate a detailed report
  if (allViolations.length > 0) {
    const violationReport = {
      generatedAt: new Date().toISOString(),
      violations: allViolations,
      warnings: allWarnings,
    };

    fs.writeFileSync(
      path.join(reportsDir, 'accessibility-violations.json'),
      JSON.stringify(violationReport, null, 2)
    );

    console.log('  📋 Accessibility violation report saved to:');
    console.log('     reports/accessibility-violations.json\n');
  }

  console.log('🎨 UI Verification Teardown Complete\n');
}

module.exports = globalTeardown;
