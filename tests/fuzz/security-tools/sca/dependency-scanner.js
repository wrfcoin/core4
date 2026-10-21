/**
 * Software Composition Analysis (SCA) - Dependency Vulnerability Scanner
 * Integrates multiple tools: npm audit, Snyk, and OWASP Dependency Check
 */

const { execSync, spawn } = require('child_process');
const fs = require('fs-extra');
const path = require('path');
const winston = require('winston');
const semver = require('semver');
const config = require('../config/security-config');

class DependencyScanner {
  constructor() {
    this.logger = winston.createLogger({
      level: 'info',
      format: winston.format.combine(
        winston.format.timestamp(),
        winston.format.json()
      ),
      transports: [
        new winston.transports.File({ filename: '../logs/sca-scanner.log' }),
        new winston.transports.Console(),
      ],
    });

    this.config = config.sca;
    this.projectRoot = path.resolve('../..');
    this.reportDir = path.resolve('../reports/security/sca');
  }

  async initialize() {
    try {
      // Ensure report directory exists
      await fs.ensureDir(this.reportDir);

      // Check available tools
      await this.checkToolAvailability();

      this.logger.info('Dependency scanner initialized successfully');
    } catch (error) {
      this.logger.error('Failed to initialize dependency scanner:', error);
      throw error;
    }
  }

  async checkToolAvailability() {
    this.toolsAvailable = {
      npm: false,
      snyk: false,
      dependencyCheck: false,
    };

    // Check npm
    try {
      execSync('npm --version', { encoding: 'utf8' });
      this.toolsAvailable.npm = true;
      this.logger.info('npm detected');
    } catch (error) {
      this.logger.warn('npm not available');
    }

    // Check Snyk
    try {
      execSync('snyk --version', { encoding: 'utf8' });
      this.toolsAvailable.snyk = true;
      this.logger.info('Snyk detected');
    } catch (error) {
      this.logger.warn(
        'Snyk not available - install with: npm install -g snyk'
      );
    }

    // Check OWASP Dependency Check
    try {
      execSync('dependency-check --version', { encoding: 'utf8' });
      this.toolsAvailable.dependencyCheck = true;
      this.logger.info('OWASP Dependency Check detected');
    } catch (error) {
      this.logger.warn('OWASP Dependency Check not available');
    }
  }

  async runComprehensiveScan() {
    const timestamp = new Date().toISOString();
    this.logger.info('Starting comprehensive dependency vulnerability scan...');

    const results = {
      scanType: 'comprehensive',
      timestamp,
      summary: {
        totalVulnerabilities: 0,
        critical: 0,
        high: 0,
        medium: 0,
        low: 0,
      },
      tools: {},
      consolidatedFindings: [],
      packageAnalysis: {},
      recommendations: [],
    };

    // Run npm audit
    if (this.toolsAvailable.npm) {
      try {
        results.tools.npmAudit = await this.runNpmAudit();
        this.mergeFindings(results, results.tools.npmAudit, 'npm-audit');
      } catch (error) {
        this.logger.error('npm audit failed:', error);
        results.tools.npmAudit = { error: error.message };
      }
    }

    // Run Snyk scan
    if (this.toolsAvailable.snyk && this.config.snyk.enabled) {
      try {
        results.tools.snyk = await this.runSnykScan();
        this.mergeFindings(results, results.tools.snyk, 'snyk');
      } catch (error) {
        this.logger.error('Snyk scan failed:', error);
        results.tools.snyk = { error: error.message };
      }
    }

    // Run OWASP Dependency Check
    if (
      this.toolsAvailable.dependencyCheck &&
      this.config.dependencyCheck.enabled
    ) {
      try {
        results.tools.dependencyCheck = await this.runDependencyCheck();
        this.mergeFindings(
          results,
          results.tools.dependencyCheck,
          'dependency-check'
        );
      } catch (error) {
        this.logger.error('OWASP Dependency Check failed:', error);
        results.tools.dependencyCheck = { error: error.message };
      }
    }

    // Analyze package.json files
    results.packageAnalysis = await this.analyzePackageFiles();

    // Generate recommendations
    results.recommendations = this.generateRecommendations(results);

    // Generate reports
    await this.generateReports(results, timestamp);

    return results;
  }

  async runNpmAudit() {
    this.logger.info('Running npm audit...');

    try {
      const auditResult = execSync('npm audit --json', {
        cwd: this.projectRoot,
        encoding: 'utf8',
      });

      const auditData = JSON.parse(auditResult);

      return {
        tool: 'npm-audit',
        vulnerabilities: this.processNpmAuditData(auditData),
        metadata: auditData.metadata || {},
      };
    } catch (error) {
      // npm audit returns non-zero exit code when vulnerabilities are found
      if (error.stdout) {
        try {
          const auditData = JSON.parse(error.stdout);
          return {
            tool: 'npm-audit',
            vulnerabilities: this.processNpmAuditData(auditData),
            metadata: auditData.metadata || {},
          };
        } catch (parseError) {
          throw new Error(
            `Failed to parse npm audit output: ${parseError.message}`
          );
        }
      }
      throw error;
    }
  }

  processNpmAuditData(auditData) {
    const vulnerabilities = [];

    if (auditData.vulnerabilities) {
      for (const [packageName, vulnData] of Object.entries(
        auditData.vulnerabilities
      )) {
        if (vulnData.via && Array.isArray(vulnData.via)) {
          vulnData.via.forEach(via => {
            if (typeof via === 'object' && via.source) {
              vulnerabilities.push({
                id: via.source.toString(),
                package: packageName,
                severity: this.mapNpmSeverity(via.severity),
                title: via.title || 'Unknown vulnerability',
                description: via.overview || '',
                url: via.url || '',
                cwe: via.cwe || [],
                cvss: via.cvss || {},
                range: via.range || '',
                fixAvailable: vulnData.fixAvailable || false,
                source: 'npm-audit',
              });
            }
          });
        }
      }
    }

    return vulnerabilities;
  }

  mapNpmSeverity(npmSeverity) {
    const severityMap = {
      critical: 'critical',
      high: 'high',
      moderate: 'medium',
      low: 'low',
    };
    return severityMap[npmSeverity] || 'low';
  }

  async runSnykScan() {
    this.logger.info('Running Snyk scan...');

    const command = [
      'snyk',
      'test',
      '--json',
      '--severity-threshold=' + this.config.snyk.severityThreshold,
    ];

    if (this.config.snyk.testDevDependencies) {
      command.push('--dev');
    }

    return new Promise((resolve, reject) => {
      const process = spawn(command[0], command.slice(1), {
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
        try {
          const snykData = JSON.parse(stdout);

          const result = {
            tool: 'snyk',
            vulnerabilities: this.processSnykData(snykData),
            summary: snykData.summary || {},
            policy: snykData.policy || {},
          };

          resolve(result);
        } catch (parseError) {
          this.logger.error('Failed to parse Snyk output:', parseError);
          reject(
            new Error(`Failed to parse Snyk output: ${parseError.message}`)
          );
        }
      });

      process.on('error', error => {
        reject(error);
      });
    });
  }

  processSnykData(snykData) {
    const vulnerabilities = [];

    if (snykData.vulnerabilities) {
      snykData.vulnerabilities.forEach(vuln => {
        vulnerabilities.push({
          id: vuln.id,
          package: vuln.packageName,
          severity: vuln.severity,
          title: vuln.title,
          description: vuln.description,
          url: vuln.url || '',
          cwe: vuln.identifiers?.CWE || [],
          cvss: {
            score: vuln.cvssScore,
            vector: vuln.CVSSv3,
          },
          version: vuln.version,
          fixedIn: vuln.fixedIn || [],
          upgradePath: vuln.upgradePath || [],
          source: 'snyk',
        });
      });
    }

    return vulnerabilities;
  }

  async runDependencyCheck() {
    this.logger.info('Running OWASP Dependency Check...');

    const reportFile = path.join(
      this.reportDir,
      'dependency-check-report.json'
    );

    const command = [
      'dependency-check',
      '--project',
      'Wrfcoin',
      '--scan',
      this.projectRoot,
      '--format',
      'JSON',
      '--out',
      this.reportDir,
      '--enableRetired',
      '--enableExperimental',
    ];

    if (this.config.dependencyCheck.suppressionFile) {
      command.push(
        '--suppression',
        this.config.dependencyCheck.suppressionFile
      );
    }

    return new Promise((resolve, reject) => {
      const process = spawn(command[0], command.slice(1), {
        stdio: ['pipe', 'pipe', 'pipe'],
      });

      let stdout = '';
      let stderr = '';

      process.stdout.on('data', data => {
        stdout += data.toString();
        this.logger.info(`Dependency Check: ${data.toString().trim()}`);
      });

      process.stderr.on('data', data => {
        stderr += data.toString();
      });

      process.on('close', async code => {
        try {
          if (await fs.pathExists(reportFile)) {
            const reportData = await fs.readJSON(reportFile);

            const result = {
              tool: 'dependency-check',
              vulnerabilities: this.processDependencyCheckData(reportData),
              projectInfo: reportData.projectInfo || {},
              scanInfo: reportData.scanInfo || {},
            };

            resolve(result);
          } else {
            reject(new Error('Dependency Check report file not found'));
          }
        } catch (error) {
          reject(error);
        }
      });

      process.on('error', error => {
        reject(error);
      });
    });
  }

  processDependencyCheckData(reportData) {
    const vulnerabilities = [];

    if (reportData.dependencies) {
      reportData.dependencies.forEach(dep => {
        if (dep.vulnerabilities) {
          dep.vulnerabilities.forEach(vuln => {
            vulnerabilities.push({
              id: vuln.name,
              package: dep.fileName || dep.filePath,
              severity: this.mapDependencyCheckSeverity(vuln.severity),
              title: vuln.description || vuln.name,
              description: vuln.description || '',
              url: vuln.references?.[0]?.url || '',
              cwe: vuln.cwes?.map(cwe => cwe.cweId) || [],
              cvss: {
                score: vuln.cvssv3?.baseScore || vuln.cvssv2?.score,
                vector: vuln.cvssv3?.attackVector || vuln.cvssv2?.accessVector,
              },
              source: 'dependency-check',
            });
          });
        }
      });
    }

    return vulnerabilities;
  }

  mapDependencyCheckSeverity(severity) {
    const severityMap = {
      CRITICAL: 'critical',
      HIGH: 'high',
      MEDIUM: 'medium',
      LOW: 'low',
    };
    return severityMap[severity] || 'low';
  }

  mergeFindings(results, toolResults, toolName) {
    if (!toolResults.vulnerabilities) return;

    toolResults.vulnerabilities.forEach(vuln => {
      // Update summary
      results.summary.totalVulnerabilities++;
      results.summary[vuln.severity] =
        (results.summary[vuln.severity] || 0) + 1;

      // Add to consolidated findings
      const existingFinding = results.consolidatedFindings.find(
        f =>
          f.id === vuln.id ||
          (f.package === vuln.package && f.title === vuln.title)
      );

      if (existingFinding) {
        // Merge sources
        if (!existingFinding.sources.includes(toolName)) {
          existingFinding.sources.push(toolName);
        }
      } else {
        results.consolidatedFindings.push({
          ...vuln,
          sources: [toolName],
        });
      }
    });
  }

  async analyzePackageFiles() {
    const analysis = {
      packageFiles: [],
      totalDependencies: 0,
      outdatedPackages: [],
      securityRisks: [],
    };

    // Find all package.json files
    const packageFiles = await this.findPackageFiles(this.projectRoot);

    for (const packageFile of packageFiles) {
      try {
        const packageData = await fs.readJSON(packageFile);
        const fileAnalysis = await this.analyzePackageFile(
          packageFile,
          packageData
        );
        analysis.packageFiles.push(fileAnalysis);
        analysis.totalDependencies += fileAnalysis.dependencyCount;
      } catch (error) {
        this.logger.error(`Failed to analyze ${packageFile}:`, error);
      }
    }

    return analysis;
  }

  async findPackageFiles(dir) {
    const packageFiles = [];

    const items = await fs.readdir(dir);

    for (const item of items) {
      const fullPath = path.join(dir, item);
      const stat = await fs.stat(fullPath);

      if (
        stat.isDirectory() &&
        item !== 'node_modules' &&
        !item.startsWith('.')
      ) {
        const subFiles = await this.findPackageFiles(fullPath);
        packageFiles.push(...subFiles);
      } else if (item === 'package.json') {
        packageFiles.push(fullPath);
      }
    }

    return packageFiles;
  }

  async analyzePackageFile(filePath, packageData) {
    const dependencies = {
      ...packageData.dependencies,
      ...packageData.devDependencies,
    };

    const analysis = {
      file: path.relative(this.projectRoot, filePath),
      dependencyCount: Object.keys(dependencies).length,
      dependencies: [],
      risks: [],
    };

    for (const [name, version] of Object.entries(dependencies)) {
      const depAnalysis = {
        name,
        version,
        risks: [],
      };

      // Check for version range risks
      if (
        version.includes('*') ||
        version.includes('^') ||
        version.includes('~')
      ) {
        depAnalysis.risks.push(
          'Version range allows potentially vulnerable updates'
        );
      }

      // Check for pre-release versions
      if (
        version.includes('-') &&
        (version.includes('alpha') || version.includes('beta'))
      ) {
        depAnalysis.risks.push('Pre-release version may be unstable');
      }

      analysis.dependencies.push(depAnalysis);
    }

    return analysis;
  }

  generateRecommendations(results) {
    const recommendations = [];

    if (results.summary.critical > 0) {
      recommendations.push({
        priority: 'CRITICAL',
        title: 'Critical Vulnerabilities Found',
        description: `Found ${results.summary.critical} critical vulnerabilities requiring immediate attention.`,
        action:
          'Update or replace vulnerable packages immediately before production deployment.',
      });
    }

    if (results.summary.high > 5) {
      recommendations.push({
        priority: 'HIGH',
        title: 'High Number of High-Severity Vulnerabilities',
        description: `Found ${results.summary.high} high-severity vulnerabilities.`,
        action:
          'Review and update vulnerable dependencies. Consider automated dependency updates.',
      });
    }

    // Tool-specific recommendations
    if (!this.toolsAvailable.snyk) {
      recommendations.push({
        priority: 'MEDIUM',
        title: 'Install Snyk for Enhanced Scanning',
        description:
          'Snyk provides more comprehensive vulnerability detection than npm audit alone.',
        action: 'Install Snyk CLI: npm install -g snyk && snyk auth',
      });
    }

    recommendations.push({
      priority: 'LOW',
      title: 'Enable Automated Dependency Updates',
      description:
        'Automate dependency updates to catch vulnerabilities early.',
      action:
        'Configure Dependabot or Renovate for automated security updates.',
    });

    return recommendations;
  }

  async generateReports(results, timestamp) {
    const baseFilename = `dependency-scan-${timestamp.replace(/[:.]/g, '-')}`;

    // JSON Report
    await this.generateJSONReport(results, baseFilename);

    // HTML Report
    await this.generateHTMLReport(results, baseFilename);

    // CSV Report
    await this.generateCSVReport(results, baseFilename);

    // Security Report
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
    <title>Dependency Vulnerability Scan Report</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; }
        .summary { background: #f5f5f5; padding: 15px; border-radius: 5px; margin-bottom: 20px; }
        .vulnerability { border: 1px solid #ddd; margin: 10px 0; padding: 15px; border-radius: 5px; }
        .critical { border-left: 5px solid #d32f2f; }
        .high { border-left: 5px solid #f57c00; }
        .medium { border-left: 5px solid #fbc02d; }
        .low { border-left: 5px solid #388e3c; }
        .tool-badge { background: #1976d2; color: white; padding: 2px 6px; border-radius: 3px; font-size: 0.8em; margin-left: 5px; }
        .recommendation { background: #e3f2fd; border-left: 4px solid #1976d2; padding: 10px; margin: 10px 0; }
    </style>
</head>
<body>
    <h1>Dependency Vulnerability Scan Report</h1>
    
    <div class="summary">
        <h2>Executive Summary</h2>
        <p><strong>Scan Timestamp:</strong> ${results.timestamp}</p>
        <p><strong>Total Vulnerabilities:</strong> ${results.summary.totalVulnerabilities}</p>
        <p><strong>Critical:</strong> ${results.summary.critical || 0}</p>
        <p><strong>High:</strong> ${results.summary.high || 0}</p>
        <p><strong>Medium:</strong> ${results.summary.medium || 0}</p>
        <p><strong>Low:</strong> ${results.summary.low || 0}</p>
        <p><strong>Tools Used:</strong> ${Object.keys(results.tools).join(', ')}</p>
    </div>
    
    <h2>Vulnerabilities</h2>
    ${results.consolidatedFindings
      .map(
        vuln => `
        <div class="vulnerability ${vuln.severity}">
            <h3>${vuln.title} 
                ${vuln.sources.map(source => `<span class="tool-badge">${source}</span>`).join('')}
            </h3>
            <p><strong>Package:</strong> ${vuln.package}</p>
            <p><strong>Severity:</strong> ${vuln.severity.toUpperCase()}</p>
            <p><strong>ID:</strong> ${vuln.id}</p>
            <p><strong>Description:</strong> ${vuln.description}</p>
            ${vuln.fixedIn && vuln.fixedIn.length > 0 ? `<p><strong>Fixed In:</strong> ${vuln.fixedIn.join(', ')}</p>` : ''}
            ${vuln.url ? `<p><strong>Reference:</strong> <a href="${vuln.url}" target="_blank">${vuln.url}</a></p>` : ''}
            ${vuln.cwe && vuln.cwe.length > 0 ? `<p><strong>CWE:</strong> ${vuln.cwe.join(', ')}</p>` : ''}
        </div>
    `
      )
      .join('')}
    
    <h2>Recommendations</h2>
    ${results.recommendations
      .map(
        rec => `
        <div class="recommendation">
            <h3>${rec.title} (${rec.priority})</h3>
            <p>${rec.description}</p>
            <p><strong>Action:</strong> ${rec.action}</p>
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
      'id',
      'package',
      'severity',
      'title',
      'description',
      'sources',
      'url',
    ];
    const parser = new Parser({ fields });
    const csv = parser.parse(results.consolidatedFindings);

    const reportPath = path.join(this.reportDir, `${baseFilename}.csv`);
    await fs.writeFile(reportPath, csv);
    this.logger.info(`CSV report generated: ${reportPath}`);
  }

  async generateSecurityReport(results, baseFilename) {
    const securityReport = {
      executiveSummary: {
        totalVulnerabilities: results.summary.totalVulnerabilities,
        riskScore: this.calculateRiskScore(results.summary),
        complianceStatus: this.assessCompliance(results.summary),
        toolsCoverage: Object.keys(results.tools).length,
      },
      topRisks: results.consolidatedFindings
        .filter(v => v.severity === 'critical' || v.severity === 'high')
        .slice(0, 10),
      recommendations: results.recommendations,
      detailedFindings: results.consolidatedFindings,
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
      (summary.critical || 0) * 10 +
      (summary.high || 0) * 7 +
      (summary.medium || 0) * 4 +
      (summary.low || 0) * 1
    );
  }

  assessCompliance(summary) {
    const critical = summary.critical || 0;
    const high = summary.high || 0;

    if (critical === 0 && high <= 2) {
      return 'COMPLIANT';
    } else if (critical <= 1 && high <= 5) {
      return 'PARTIALLY_COMPLIANT';
    } else {
      return 'NON_COMPLIANT';
    }
  }

  evaluateResults(results) {
    const { critical, high } = results.summary;

    if (critical > 0 && config.cicd.failOnCritical) {
      this.logger.error(
        `Found ${critical} critical vulnerabilities - failing CI`
      );
      return true;
    }

    if (high > config.cicd.maxAllowedHigh) {
      this.logger.error(
        `Found ${high} high-severity vulnerabilities (max allowed: ${config.cicd.maxAllowedHigh}) - failing CI`
      );
      return true;
    }

    return false;
  }
}

// CLI Usage
if (require.main === module) {
  const scanner = new DependencyScanner();

  async function main() {
    try {
      await scanner.initialize();
      const results = await scanner.runComprehensiveScan();

      console.log('Dependency scan completed successfully');
      console.log(
        `Total vulnerabilities: ${results.summary.totalVulnerabilities}`
      );
      console.log(`Critical: ${results.summary.critical || 0}`);
      console.log(`High: ${results.summary.high || 0}`);
      console.log(`Medium: ${results.summary.medium || 0}`);
      console.log(`Low: ${results.summary.low || 0}`);

      const shouldFailCI = scanner.evaluateResults(results);
      if (shouldFailCI) {
        console.error('Dependency scan failed CI thresholds');
        process.exit(1);
      }
    } catch (error) {
      console.error('Dependency scan failed:', error);
      process.exit(1);
    }
  }

  main();
}

module.exports = DependencyScanner;
