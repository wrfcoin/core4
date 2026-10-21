/**
 * OWASP ZAP Dynamic Application Security Testing (DAST) Scanner
 * Integrates ZAP for comprehensive web vulnerability scanning
 */

const { execSync, spawn } = require('child_process');
const fs = require('fs-extra');
const path = require('path');
const winston = require('winston');
const axios = require('axios');
const xml2js = require('xml2js');
const config = require('../config/security-config');

class ZAPScanner {
  constructor() {
    this.logger = winston.createLogger({
      level: 'info',
      format: winston.format.combine(
        winston.format.timestamp(),
        winston.format.json()
      ),
      transports: [
        new winston.transports.File({ filename: '../logs/dast-scanner.log' }),
        new winston.transports.Console(),
      ],
    });

    this.config = config.dast.zap;
    this.targets = config.targets;
    this.reportDir = path.resolve('../reports/security/dast');
    this.zapApiUrl = 'http://localhost:8080';
    this.zapApiKey = 'wrfcoin-security-key';
  }

  async initialize() {
    try {
      // Ensure Docker is available
      await this.checkDockerInstallation();

      // Ensure report directory exists
      await fs.ensureDir(this.reportDir);

      this.logger.info('ZAP scanner initialized successfully');
    } catch (error) {
      this.logger.error('Failed to initialize ZAP scanner:', error);
      throw error;
    }
  }

  async checkDockerInstallation() {
    try {
      const version = execSync('docker --version', { encoding: 'utf8' });
      this.logger.info(`Docker version: ${version.trim()}`);
    } catch (error) {
      throw new Error(
        'Docker not found. Please install Docker to run ZAP scans.'
      );
    }
  }

  async runBaselineScan(targetUrl = null) {
    const targets = targetUrl
      ? [{ baseUrl: targetUrl, name: 'Custom Target' }]
      : Object.values(this.targets);
    const timestamp = new Date().toISOString();

    this.logger.info('Starting ZAP baseline scan...');

    const results = {
      scanType: 'baseline',
      timestamp,
      targets: [],
      summary: {
        totalAlerts: 0,
        high: 0,
        medium: 0,
        low: 0,
        informational: 0,
      },
    };

    for (const target of targets) {
      try {
        this.logger.info(`Scanning target: ${target.name} (${target.baseUrl})`);
        const targetResult = await this.executeBaselineScan(target, timestamp);
        results.targets.push(targetResult);

        // Update summary
        this.updateSummary(results.summary, targetResult.alerts);
      } catch (error) {
        this.logger.error(`Failed to scan ${target.name}:`, error);
        results.targets.push({
          name: target.name,
          baseUrl: target.baseUrl,
          error: error.message,
          alerts: [],
        });
      }
    }

    // Generate reports
    await this.generateReports(results, 'baseline');

    return results;
  }

  async executeBaselineScan(target, timestamp) {
    const containerName = `zap-baseline-${timestamp.replace(/[:.]/g, '-')}`;
    const reportFile = `zap-baseline-${target.name.toLowerCase().replace(/\s+/g, '-')}-${timestamp.replace(/[:.]/g, '-')}.xml`;
    const reportPath = path.join(this.reportDir, reportFile);

    // Prepare ZAP baseline command
    const command = [
      'docker',
      'run',
      '--rm',
      '--name',
      containerName,
      '-v',
      `${this.reportDir}:/zap/wrk/:rw`,
      '-t',
      this.config.dockerImage,
      'zap-baseline.py',
      '-t',
      target.baseUrl,
      '-x',
      reportFile,
      '-d', // debug mode
    ];

    this.logger.info(`Executing: ${command.join(' ')}`);

    return new Promise((resolve, reject) => {
      const process = spawn(command[0], command.slice(1), {
        stdio: ['pipe', 'pipe', 'pipe'],
      });

      let stdout = '';
      let stderr = '';

      process.stdout.on('data', data => {
        stdout += data.toString();
        this.logger.info(`ZAP: ${data.toString().trim()}`);
      });

      process.stderr.on('data', data => {
        stderr += data.toString();
        this.logger.warn(`ZAP Warning: ${data.toString().trim()}`);
      });

      const timeout = setTimeout(() => {
        process.kill('SIGTERM');
        reject(new Error('ZAP baseline scan timeout'));
      }, this.config.baselineTimeout * 1000);

      process.on('close', async code => {
        clearTimeout(timeout);

        try {
          // Parse XML report
          const alerts = await this.parseZapReport(reportPath);

          resolve({
            name: target.name,
            baseUrl: target.baseUrl,
            exitCode: code,
            alerts,
            reportPath,
          });
        } catch (parseError) {
          this.logger.error('Failed to parse ZAP report:', parseError);
          resolve({
            name: target.name,
            baseUrl: target.baseUrl,
            exitCode: code,
            alerts: [],
            error: parseError.message,
            reportPath,
          });
        }
      });

      process.on('error', error => {
        clearTimeout(timeout);
        reject(error);
      });
    });
  }

  async runFullScan(targetUrl = null) {
    const targets = targetUrl
      ? [{ baseUrl: targetUrl, name: 'Custom Target' }]
      : Object.values(this.targets);
    const timestamp = new Date().toISOString();

    this.logger.info('Starting ZAP full active scan...');

    const results = {
      scanType: 'full',
      timestamp,
      targets: [],
      summary: {
        totalAlerts: 0,
        high: 0,
        medium: 0,
        low: 0,
        informational: 0,
      },
    };

    for (const target of targets) {
      try {
        this.logger.info(
          `Full scanning target: ${target.name} (${target.baseUrl})`
        );
        const targetResult = await this.executeFullScan(target, timestamp);
        results.targets.push(targetResult);

        // Update summary
        this.updateSummary(results.summary, targetResult.alerts);
      } catch (error) {
        this.logger.error(`Failed to full scan ${target.name}:`, error);
        results.targets.push({
          name: target.name,
          baseUrl: target.baseUrl,
          error: error.message,
          alerts: [],
        });
      }
    }

    // Generate reports
    await this.generateReports(results, 'full');

    return results;
  }

  async executeFullScan(target, timestamp) {
    const containerName = `zap-full-${timestamp.replace(/[:.]/g, '-')}`;
    const reportFile = `zap-full-${target.name.toLowerCase().replace(/\s+/g, '-')}-${timestamp.replace(/[:.]/g, '-')}.xml`;
    const reportPath = path.join(this.reportDir, reportFile);

    // Prepare ZAP full scan command with more comprehensive options
    const command = [
      'docker',
      'run',
      '--rm',
      '--name',
      containerName,
      '-v',
      `${this.reportDir}:/zap/wrk/:rw`,
      '-t',
      this.config.dockerImage,
      'zap-full-scan.py',
      '-t',
      target.baseUrl,
      '-x',
      reportFile,
      '-d', // debug mode
      '-m',
      this.config.spiderMaxDepth.toString(),
      '-l',
      this.config.activeScanIntensity,
    ];

    this.logger.info(`Executing: ${command.join(' ')}`);

    return new Promise((resolve, reject) => {
      const process = spawn(command[0], command.slice(1), {
        stdio: ['pipe', 'pipe', 'pipe'],
      });

      let stdout = '';
      let stderr = '';

      process.stdout.on('data', data => {
        stdout += data.toString();
        this.logger.info(`ZAP: ${data.toString().trim()}`);
      });

      process.stderr.on('data', data => {
        stderr += data.toString();
        this.logger.warn(`ZAP Warning: ${data.toString().trim()}`);
      });

      const timeout = setTimeout(() => {
        process.kill('SIGTERM');
        reject(new Error('ZAP full scan timeout'));
      }, this.config.fullScanTimeout * 1000);

      process.on('close', async code => {
        clearTimeout(timeout);

        try {
          // Parse XML report
          const alerts = await this.parseZapReport(reportPath);

          resolve({
            name: target.name,
            baseUrl: target.baseUrl,
            exitCode: code,
            alerts,
            reportPath,
          });
        } catch (parseError) {
          this.logger.error('Failed to parse ZAP report:', parseError);
          resolve({
            name: target.name,
            baseUrl: target.baseUrl,
            exitCode: code,
            alerts: [],
            error: parseError.message,
            reportPath,
          });
        }
      });

      process.on('error', error => {
        clearTimeout(timeout);
        reject(error);
      });
    });
  }

  async parseZapReport(reportPath) {
    try {
      if (!(await fs.pathExists(reportPath))) {
        this.logger.warn(`Report file not found: ${reportPath}`);
        return [];
      }

      const xmlContent = await fs.readFile(reportPath, 'utf8');
      const parser = new xml2js.Parser();
      const result = await parser.parseStringPromise(xmlContent);

      const alerts = [];

      if (result.OWASPZAPReport && result.OWASPZAPReport.site) {
        const sites = Array.isArray(result.OWASPZAPReport.site)
          ? result.OWASPZAPReport.site
          : [result.OWASPZAPReport.site];

        for (const site of sites) {
          if (site.alerts && site.alerts[0] && site.alerts[0].alertitem) {
            const alertItems = Array.isArray(site.alerts[0].alertitem)
              ? site.alerts[0].alertitem
              : [site.alerts[0].alertitem];

            for (const alert of alertItems) {
              alerts.push(this.processZapAlert(alert));
            }
          }
        }
      }

      return alerts;
    } catch (error) {
      this.logger.error('Failed to parse ZAP XML report:', error);
      return [];
    }
  }

  processZapAlert(alert) {
    return {
      pluginId: alert.pluginid ? alert.pluginid[0] : '',
      alertRef: alert.alertRef ? alert.alertRef[0] : '',
      name: alert.name ? alert.name[0] : '',
      riskLevel: alert.riskcode
        ? this.mapZapRisk(alert.riskcode[0])
        : 'informational',
      confidence: alert.confidence ? alert.confidence[0] : '',
      description: alert.desc ? alert.desc[0] : '',
      uri: alert.uri ? alert.uri[0] : '',
      param: alert.param ? alert.param[0] : '',
      attack: alert.attack ? alert.attack[0] : '',
      evidence: alert.evidence ? alert.evidence[0] : '',
      solution: alert.solution ? alert.solution[0] : '',
      reference: alert.reference ? alert.reference[0] : '',
      cweid: alert.cweid ? alert.cweid[0] : '',
      wascid: alert.wascid ? alert.wascid[0] : '',
      sourceid: alert.sourceid ? alert.sourceid[0] : '',
    };
  }

  mapZapRisk(riskCode) {
    const riskMap = {
      3: 'high',
      2: 'medium',
      1: 'low',
      0: 'informational',
    };
    return riskMap[riskCode] || 'informational';
  }

  updateSummary(summary, alerts) {
    summary.totalAlerts += alerts.length;

    alerts.forEach(alert => {
      summary[alert.riskLevel] = (summary[alert.riskLevel] || 0) + 1;
    });
  }

  async generateReports(results, scanType) {
    const baseFilename = `zap-${scanType}-report-${results.timestamp.replace(/[:.]/g, '-')}`;

    // JSON Report
    await this.generateJSONReport(results, baseFilename);

    // HTML Report
    await this.generateHTMLReport(results, baseFilename);

    // CSV Report
    await this.generateCSVReport(results, baseFilename);

    // Security Summary Report
    await this.generateSecurityReport(results, baseFilename);
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
    <title>ZAP Security Scan Report - ${results.scanType.toUpperCase()}</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; }
        .summary { background: #f5f5f5; padding: 15px; border-radius: 5px; margin-bottom: 20px; }
        .target { border: 1px solid #ddd; margin: 20px 0; padding: 15px; border-radius: 5px; }
        .alert { border: 1px solid #ddd; margin: 10px 0; padding: 15px; border-radius: 5px; }
        .high { border-left: 5px solid #d32f2f; }
        .medium { border-left: 5px solid #f57c00; }
        .low { border-left: 5px solid #fbc02d; }
        .informational { border-left: 5px solid #1976d2; }
        .evidence { background: #f8f8f8; padding: 10px; border-radius: 3px; margin: 10px 0; }
        pre { white-space: pre-wrap; word-wrap: break-word; }
        .risk-high { color: #d32f2f; font-weight: bold; }
        .risk-medium { color: #f57c00; font-weight: bold; }
        .risk-low { color: #fbc02d; font-weight: bold; }
        .risk-informational { color: #1976d2; }
    </style>
</head>
<body>
    <h1>ZAP Security Scan Report - ${results.scanType.toUpperCase()}</h1>
    
    <div class="summary">
        <h2>Executive Summary</h2>
        <p><strong>Scan Type:</strong> ${results.scanType}</p>
        <p><strong>Timestamp:</strong> ${results.timestamp}</p>
        <p><strong>Total Alerts:</strong> ${results.summary.totalAlerts}</p>
        <p><span class="risk-high">High Risk:</span> ${results.summary.high || 0}</p>
        <p><span class="risk-medium">Medium Risk:</span> ${results.summary.medium || 0}</p>
        <p><span class="risk-low">Low Risk:</span> ${results.summary.low || 0}</p>
        <p><span class="risk-informational">Informational:</span> ${results.summary.informational || 0}</p>
    </div>
    
    ${results.targets
      .map(
        target => `
        <div class="target">
            <h2>${target.name}</h2>
            <p><strong>URL:</strong> ${target.baseUrl}</p>
            ${target.error ? `<p><strong>Error:</strong> ${target.error}</p>` : ''}
            <p><strong>Alerts Found:</strong> ${target.alerts ? target.alerts.length : 0}</p>
            
            ${
              target.alerts
                ? target.alerts
                    .map(
                      alert => `
                <div class="alert ${alert.riskLevel}">
                    <h3>${alert.name}</h3>
                    <p><strong>Risk Level:</strong> <span class="risk-${alert.riskLevel}">${alert.riskLevel.toUpperCase()}</span></p>
                    <p><strong>Confidence:</strong> ${alert.confidence}</p>
                    <p><strong>URL:</strong> ${alert.uri}</p>
                    ${alert.param ? `<p><strong>Parameter:</strong> ${alert.param}</p>` : ''}
                    <p><strong>Description:</strong> ${alert.description}</p>
                    ${
                      alert.evidence
                        ? `
                        <div class="evidence">
                            <strong>Evidence:</strong>
                            <pre>${alert.evidence}</pre>
                        </div>
                    `
                        : ''
                    }
                    ${alert.solution ? `<p><strong>Solution:</strong> ${alert.solution}</p>` : ''}
                    ${alert.reference ? `<p><strong>Reference:</strong> ${alert.reference}</p>` : ''}
                    ${alert.cweid ? `<p><strong>CWE ID:</strong> ${alert.cweid}</p>` : ''}
                </div>
            `
                    )
                    .join('')
                : ''
            }
        </div>
    `
      )
      .join('')}
</body>
</html>`;
  }

  async generateCSVReport(results, baseFilename) {
    const { Parser } = require('json2csv');

    // Flatten all alerts from all targets
    const allAlerts = [];
    results.targets.forEach(target => {
      if (target.alerts) {
        target.alerts.forEach(alert => {
          allAlerts.push({
            target: target.name,
            targetUrl: target.baseUrl,
            ...alert,
          });
        });
      }
    });

    const fields = [
      'target',
      'targetUrl',
      'name',
      'riskLevel',
      'confidence',
      'uri',
      'param',
      'description',
      'solution',
      'cweid',
    ];
    const parser = new Parser({ fields });
    const csv = parser.parse(allAlerts);

    const reportPath = path.join(this.reportDir, `${baseFilename}.csv`);
    await fs.writeFile(reportPath, csv);
    this.logger.info(`CSV report generated: ${reportPath}`);
  }

  async generateSecurityReport(results, baseFilename) {
    const securityReport = {
      scanType: results.scanType,
      timestamp: results.timestamp,
      executiveSummary: {
        totalTargets: results.targets.length,
        totalAlerts: results.summary.totalAlerts,
        riskDistribution: {
          high: results.summary.high || 0,
          medium: results.summary.medium || 0,
          low: results.summary.low || 0,
          informational: results.summary.informational || 0,
        },
        riskScore: this.calculateRiskScore(results.summary),
        complianceStatus: this.assessCompliance(results.summary),
      },
      recommendations: this.generateRecommendations(results),
      detailedFindings: results.targets,
    };

    const reportPath = path.join(
      this.reportDir,
      `${baseFilename}-security.json`
    );
    await fs.writeJSON(reportPath, securityReport, { spaces: 2 });
    this.logger.info(`Security report generated: ${reportPath}`);
  }

  calculateRiskScore(summary) {
    return (
      (summary.high || 0) * 10 +
      (summary.medium || 0) * 5 +
      (summary.low || 0) * 1
    );
  }

  assessCompliance(summary) {
    const highRisk = summary.high || 0;
    const mediumRisk = summary.medium || 0;

    if (highRisk === 0 && mediumRisk <= 5) {
      return 'COMPLIANT';
    } else if (highRisk <= 2 && mediumRisk <= 10) {
      return 'PARTIALLY_COMPLIANT';
    } else {
      return 'NON_COMPLIANT';
    }
  }

  generateRecommendations(results) {
    const recommendations = [];

    const highRiskCount = results.summary.high || 0;
    const mediumRiskCount = results.summary.medium || 0;

    if (highRiskCount > 0) {
      recommendations.push({
        priority: 'CRITICAL',
        title: 'Address High-Risk Vulnerabilities',
        description: `Found ${highRiskCount} high-risk vulnerabilities that require immediate attention.`,
        action:
          'Review and remediate all high-risk findings before production deployment.',
      });
    }

    if (mediumRiskCount > 10) {
      recommendations.push({
        priority: 'HIGH',
        title: 'Reduce Medium-Risk Vulnerability Count',
        description: `Found ${mediumRiskCount} medium-risk vulnerabilities.`,
        action:
          'Implement security controls to reduce medium-risk vulnerabilities below 10.',
      });
    }

    // Add specific recommendations based on common vulnerability types
    recommendations.push({
      priority: 'MEDIUM',
      title: 'Implement Security Headers',
      description: 'Ensure all responses include proper security headers.',
      action:
        'Add Content-Security-Policy, X-Frame-Options, and other security headers.',
    });

    return recommendations;
  }

  evaluateResults(results) {
    const { high, medium } = results.summary;

    if (high > 0 && config.cicd.failOnCritical) {
      this.logger.error(`Found ${high} high-risk vulnerabilities - failing CI`);
      return true;
    }

    if (high > 0 && config.cicd.failOnHigh) {
      this.logger.error(`Found ${high} high-risk vulnerabilities - failing CI`);
      return true;
    }

    return false;
  }
}

// CLI Usage
if (require.main === module) {
  const scanner = new ZAPScanner();

  const args = process.argv.slice(2);
  const scanType = args[0] || 'baseline';
  const targetUrl = args[1] || null;

  async function main() {
    try {
      await scanner.initialize();

      let results;
      if (scanType === 'full') {
        results = await scanner.runFullScan(targetUrl);
      } else {
        results = await scanner.runBaselineScan(targetUrl);
      }

      console.log('ZAP scan completed successfully');
      console.log(`Total alerts: ${results.summary.totalAlerts}`);
      console.log(`High risk: ${results.summary.high || 0}`);
      console.log(`Medium risk: ${results.summary.medium || 0}`);
      console.log(`Low risk: ${results.summary.low || 0}`);

      const shouldFailCI = scanner.evaluateResults(results);
      if (shouldFailCI) {
        console.error('Security scan failed CI thresholds');
        process.exit(1);
      }
    } catch (error) {
      console.error('ZAP scan failed:', error);
      process.exit(1);
    }
  }

  main();
}

module.exports = ZAPScanner;
