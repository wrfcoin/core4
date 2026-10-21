/**
 * Semgrep Static Application Security Testing (SAST) Scanner
 * Integrates Semgrep for comprehensive code analysis
 */

const { execSync, spawn } = require('child_process');
const fs = require('fs-extra');
const path = require('path');
const winston = require('winston');
const config = require('../config/security-config');

class SemgrepScanner {
  constructor() {
    this.logger = winston.createLogger({
      level: 'info',
      format: winston.format.combine(
        winston.format.timestamp(),
        winston.format.json()
      ),
      transports: [
        new winston.transports.File({ filename: '../logs/sast-scanner.log' }),
        new winston.transports.Console(),
      ],
    });

    this.config = config.sast.semgrep;
    this.projectRoot = path.resolve('../..');
    this.reportDir = path.resolve('../reports/security/sast');
  }

  async initialize() {
    try {
      // Ensure Semgrep is installed
      await this.checkSemgrepInstallation();

      // Ensure report directory exists
      await fs.ensureDir(this.reportDir);

      // Update Semgrep rules
      await this.updateRules();

      this.logger.info('Semgrep scanner initialized successfully');
    } catch (error) {
      this.logger.error('Failed to initialize Semgrep scanner:', error);
      throw error;
    }
  }

  async checkSemgrepInstallation() {
    try {
      const version = execSync('semgrep --version', { encoding: 'utf8' });
      this.logger.info(`Semgrep version: ${version.trim()}`);
    } catch (error) {
      this.logger.error('Semgrep not found. Installing...');
      await this.installSemgrep();
    }
  }

  async installSemgrep() {
    try {
      this.logger.info('Installing Semgrep via pip...');
      execSync('pip3 install semgrep', { stdio: 'inherit' });
      this.logger.info('Semgrep installed successfully');
    } catch (error) {
      this.logger.error('Failed to install Semgrep:', error);
      throw new Error(
        'Semgrep installation failed. Please install manually: pip3 install semgrep'
      );
    }
  }

  async updateRules() {
    try {
      this.logger.info('Updating Semgrep rules...');
      execSync('semgrep --update', { stdio: 'inherit' });
      this.logger.info('Semgrep rules updated successfully');
    } catch (error) {
      this.logger.warn('Failed to update Semgrep rules:', error);
      // Don't throw - we can continue with existing rules
    }
  }

  async runScan(targetPath = null, rulesets = null) {
    const scanTarget = targetPath || this.projectRoot;
    const rules = rulesets || this.config.ruleSets;
    const timestamp = new Date().toISOString();

    this.logger.info(`Starting Semgrep scan on: ${scanTarget}`);

    try {
      const scanResults = await this.executeSemgrepScan(scanTarget, rules);
      const processedResults = await this.processResults(scanResults);

      // Generate reports
      await this.generateReports(processedResults, timestamp);

      // Check if scan should fail CI
      const shouldFailCI = this.evaluateResults(processedResults);

      return {
        success: !shouldFailCI,
        results: processedResults,
        timestamp,
        shouldFailCI,
        reportPath: path.join(
          this.reportDir,
          `semgrep-report-${timestamp.replace(/[:.]/g, '-')}.json`
        ),
      };
    } catch (error) {
      this.logger.error('Semgrep scan failed:', error);
      throw error;
    }
  }

  async executeSemgrepScan(targetPath, rules) {
    const command = this.buildSemgrepCommand(targetPath, rules);

    this.logger.info(`Executing command: ${command}`);

    return new Promise((resolve, reject) => {
      const process = spawn('bash', ['-c', command], {
        cwd: this.projectRoot,
        stdio: ['pipe', 'pipe', 'pipe'],
      });

      let stdout = '';
      let stderr = '';

      process.stdout.on('data', data => {
        stdout += data.toString();
      });

      process.stderr.on('data', data => {
        stderr += data.toString();
      });

      process.on('close', code => {
        if (code === 0 || code === 1) {
          // 0 = no findings, 1 = findings found
          try {
            const results = JSON.parse(stdout);
            resolve(results);
          } catch (parseError) {
            this.logger.error('Failed to parse Semgrep output:', parseError);
            reject(new Error('Invalid JSON output from Semgrep'));
          }
        } else {
          this.logger.error(`Semgrep failed with code ${code}:`);
          this.logger.error('STDERR:', stderr);
          reject(new Error(`Semgrep scan failed with exit code ${code}`));
        }
      });

      process.on('error', error => {
        reject(error);
      });
    });
  }

  buildSemgrepCommand(targetPath, rules) {
    const ruleFlags = rules.map(rule => `--config=${rule}`).join(' ');
    const excludeFlags = this.config.excludePaths
      .map(path => `--exclude="${path}"`)
      .join(' ');

    return `semgrep ${ruleFlags} ${excludeFlags} --json --severity=${this.config.severityThreshold} "${targetPath}"`;
  }

  async processResults(rawResults) {
    const findings = rawResults.results || [];

    const processed = {
      summary: {
        totalFindings: findings.length,
        critical: 0,
        high: 0,
        medium: 0,
        low: 0,
        info: 0,
      },
      findings: [],
      vulnerabilityTypes: {},
      fileStats: {},
      ruleStats: {},
    };

    findings.forEach(finding => {
      const severity = this.mapSeverity(finding.extra.severity);
      processed.summary[severity]++;

      // Process finding
      const processedFinding = {
        id: finding.check_id,
        severity,
        message: finding.extra.message,
        file: finding.path,
        line: finding.start.line,
        column: finding.start.col,
        code: finding.extra.lines,
        rule: finding.check_id,
        cwe: finding.extra.metadata?.cwe || [],
        owasp: finding.extra.metadata?.owasp || [],
        references: finding.extra.metadata?.references || [],
        impact: finding.extra.metadata?.impact || 'Unknown',
        likelihood: finding.extra.metadata?.likelihood || 'Unknown',
        confidence: finding.extra.metadata?.confidence || 'Unknown',
      };

      processed.findings.push(processedFinding);

      // Update statistics
      this.updateStats(processed, processedFinding);
    });

    return processed;
  }

  mapSeverity(semgrepSeverity) {
    const severityMap = {
      ERROR: 'high',
      WARNING: 'medium',
      INFO: 'low',
    };
    return severityMap[semgrepSeverity] || 'low';
  }

  updateStats(processed, finding) {
    // Vulnerability types
    const vulnType = finding.rule.split('.').pop();
    processed.vulnerabilityTypes[vulnType] =
      (processed.vulnerabilityTypes[vulnType] || 0) + 1;

    // File statistics
    processed.fileStats[finding.file] =
      (processed.fileStats[finding.file] || 0) + 1;

    // Rule statistics
    processed.ruleStats[finding.rule] =
      (processed.ruleStats[finding.rule] || 0) + 1;
  }

  async generateReports(results, timestamp) {
    const baseFilename = `semgrep-report-${timestamp.replace(/[:.]/g, '-')}`;

    // JSON Report
    await this.generateJSONReport(results, baseFilename);

    // HTML Report
    await this.generateHTMLReport(results, baseFilename);

    // CSV Report
    await this.generateCSVReport(results, baseFilename);

    // SARIF Report (for GitHub integration)
    await this.generateSARIFReport(results, baseFilename);
  }

  async generateJSONReport(results, baseFilename) {
    const reportPath = path.join(this.reportDir, `${baseFilename}.json`);
    await fs.writeJSON(reportPath, results, { spaces: 2 });
    this.logger.info(`JSON report generated: ${reportPath}`);
  }

  async generateHTMLReport(results, baseFilename) {
    const htmlContent = this.generateHTMLContent(results);
    const reportPath = path.join(this.reportDir, `${baseFilename}.html`);
    await fs.writeFile(reportPath, htmlContent);
    this.logger.info(`HTML report generated: ${reportPath}`);
  }

  generateHTMLContent(results) {
    return `
<!DOCTYPE html>
<html>
<head>
    <title>Semgrep Security Scan Report</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; }
        .summary { background: #f5f5f5; padding: 15px; border-radius: 5px; margin-bottom: 20px; }
        .finding { border: 1px solid #ddd; margin: 10px 0; padding: 15px; border-radius: 5px; }
        .critical { border-left: 5px solid #d32f2f; }
        .high { border-left: 5px solid #f57c00; }
        .medium { border-left: 5px solid #fbc02d; }
        .low { border-left: 5px solid #388e3c; }
        .code { background: #f8f8f8; padding: 10px; border-radius: 3px; margin: 10px 0; }
        pre { white-space: pre-wrap; }
    </style>
</head>
<body>
    <h1>Semgrep Security Scan Report</h1>
    
    <div class="summary">
        <h2>Summary</h2>
        <p><strong>Total Findings:</strong> ${results.summary.totalFindings}</p>
        <p><strong>Critical:</strong> ${results.summary.critical}</p>
        <p><strong>High:</strong> ${results.summary.high}</p>
        <p><strong>Medium:</strong> ${results.summary.medium}</p>
        <p><strong>Low:</strong> ${results.summary.low}</p>
    </div>
    
    <h2>Findings</h2>
    ${results.findings
      .map(
        finding => `
        <div class="finding ${finding.severity}">
            <h3>${finding.rule}</h3>
            <p><strong>Severity:</strong> ${finding.severity.toUpperCase()}</p>
            <p><strong>File:</strong> ${finding.file}:${finding.line}:${finding.column}</p>
            <p><strong>Message:</strong> ${finding.message}</p>
            ${finding.cwe.length > 0 ? `<p><strong>CWE:</strong> ${finding.cwe.join(', ')}</p>` : ''}
            ${finding.owasp.length > 0 ? `<p><strong>OWASP:</strong> ${finding.owasp.join(', ')}</p>` : ''}
            <div class="code">
                <strong>Code:</strong>
                <pre>${finding.code}</pre>
            </div>
        </div>
    `
      )
      .join('')}
</body>
</html>`;
  }

  async generateCSVReport(results, baseFilename) {
    const { Parser } = require('json2csv');
    const fields = [
      'rule',
      'severity',
      'file',
      'line',
      'message',
      'cwe',
      'owasp',
    ];
    const parser = new Parser({ fields });
    const csv = parser.parse(results.findings);

    const reportPath = path.join(this.reportDir, `${baseFilename}.csv`);
    await fs.writeFile(reportPath, csv);
    this.logger.info(`CSV report generated: ${reportPath}`);
  }

  async generateSARIFReport(results, baseFilename) {
    const sarif = {
      version: '2.1.0',
      $schema:
        'https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json',
      runs: [
        {
          tool: {
            driver: {
              name: 'Semgrep',
              version: '1.0.0',
              informationUri: 'https://semgrep.dev/',
            },
          },
          results: results.findings.map(finding => ({
            ruleId: finding.rule,
            level: this.mapSeverityToSARIF(finding.severity),
            message: { text: finding.message },
            locations: [
              {
                physicalLocation: {
                  artifactLocation: { uri: finding.file },
                  region: {
                    startLine: finding.line,
                    startColumn: finding.column,
                  },
                },
              },
            ],
          })),
        },
      ],
    };

    const reportPath = path.join(this.reportDir, `${baseFilename}.sarif`);
    await fs.writeJSON(reportPath, sarif, { spaces: 2 });
    this.logger.info(`SARIF report generated: ${reportPath}`);
  }

  mapSeverityToSARIF(severity) {
    const map = {
      critical: 'error',
      high: 'error',
      medium: 'warning',
      low: 'note',
    };
    return map[severity] || 'note';
  }

  evaluateResults(results) {
    const { critical, high, medium } = results.summary;

    if (critical > 0 && config.cicd.failOnCritical) {
      this.logger.error(
        `Found ${critical} critical vulnerabilities - failing CI`
      );
      return true;
    }

    if (high > 0 && config.cicd.failOnHigh) {
      this.logger.error(
        `Found ${high} high-severity vulnerabilities - failing CI`
      );
      return true;
    }

    if (high > config.cicd.maxAllowedHigh) {
      this.logger.error(
        `Found ${high} high-severity vulnerabilities (max allowed: ${config.cicd.maxAllowedHigh}) - failing CI`
      );
      return true;
    }

    if (medium > config.cicd.maxAllowedMedium) {
      this.logger.error(
        `Found ${medium} medium-severity vulnerabilities (max allowed: ${config.cicd.maxAllowedMedium}) - failing CI`
      );
      return true;
    }

    return false;
  }

  async scanChangedFiles(baseBranch = 'main') {
    try {
      // Get list of changed files
      const changedFiles = execSync(
        `git diff --name-only ${baseBranch}...HEAD`,
        {
          encoding: 'utf8',
          cwd: this.projectRoot,
        }
      )
        .trim()
        .split('\n')
        .filter(file => file.endsWith('.js') || file.endsWith('.ts'));

      if (changedFiles.length === 0) {
        this.logger.info('No JavaScript/TypeScript files changed');
        return { success: true, results: { summary: { totalFindings: 0 } } };
      }

      this.logger.info(`Scanning ${changedFiles.length} changed files`);

      // Create temporary file list for Semgrep
      const fileListPath = path.join(this.reportDir, 'changed-files.txt');
      await fs.writeFile(fileListPath, changedFiles.join('\n'));

      // Run Semgrep on changed files only
      const results = await this.runScan(null, [
        '--config=auto',
        `--include-from=${fileListPath}`,
      ]);

      // Cleanup
      await fs.remove(fileListPath);

      return results;
    } catch (error) {
      this.logger.error('Failed to scan changed files:', error);
      throw error;
    }
  }
}

// CLI Usage
if (require.main === module) {
  const scanner = new SemgrepScanner();

  const args = process.argv.slice(2);
  const scanType = args[0] || 'full';

  async function main() {
    try {
      await scanner.initialize();

      let results;
      if (scanType === 'changed') {
        results = await scanner.scanChangedFiles(args[1]);
      } else {
        results = await scanner.runScan();
      }

      console.log('Scan completed successfully');
      console.log(`Total findings: ${results.results.summary.totalFindings}`);
      console.log(`Critical: ${results.results.summary.critical}`);
      console.log(`High: ${results.results.summary.high}`);
      console.log(`Medium: ${results.results.summary.medium}`);
      console.log(`Low: ${results.results.summary.low}`);

      if (results.shouldFailCI) {
        console.error('Security scan failed CI thresholds');
        process.exit(1);
      }
    } catch (error) {
      console.error('Scan failed:', error);
      process.exit(1);
    }
  }

  main();
}

module.exports = SemgrepScanner;
