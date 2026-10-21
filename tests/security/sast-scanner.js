/**
 * Static Application Security Testing (SAST) Scanner
 * Automated security scanning for source code vulnerabilities
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');
const config = require('../test.config');

class SASTScanner {
  constructor() {
    this.reportDir = path.join(__dirname, '../reports/security');
    this.projectRoot = path.resolve(__dirname, '../..');
    this.vulnerabilities = [];
    this.scanResults = {
      summary: {
        totalFiles: 0,
        vulnerableFiles: 0,
        totalVulnerabilities: 0,
        criticalCount: 0,
        highCount: 0,
        mediumCount: 0,
        lowCount: 0,
      },
      vulnerabilities: [],
    };

    // Ensure report directory exists
    if (!fs.existsSync(this.reportDir)) {
      fs.mkdirSync(this.reportDir, { recursive: true });
    }
  }

  /**
   * Run comprehensive SAST scan
   */
  async runComprehensiveScan(options = {}) {
    const {
      scanTypes = ['secrets', 'vulnerabilities', 'dependencies', 'config'],
      outputFormats = ['json', 'html'],
      severity = 'medium',
    } = options;

    console.log('🔒 Starting SAST Security Scan\n');
    console.log(`📁 Project Root: ${this.projectRoot}`);
    console.log(`🎯 Scan Types: ${scanTypes.join(', ')}`);

    try {
      // Initialize scan results
      this.scanResults.metadata = {
        timestamp: new Date().toISOString(),
        projectRoot: this.projectRoot,
        scanTypes,
        minSeverity: severity,
      };

      // Run each scan type
      for (const scanType of scanTypes) {
        console.log(`\n🔍 Running ${scanType} scan...`);
        await this.runScanType(scanType);
      }

      // Generate reports
      if (outputFormats.includes('json')) {
        await this.generateJSONReport();
      }
      if (outputFormats.includes('html')) {
        await this.generateHTMLReport();
      }

      this.printScanSummary();
      return this.scanResults;
    } catch (error) {
      console.error('❌ SAST scan failed:', error);
      throw error;
    }
  }

  /**
   * Run specific scan type
   */
  async runScanType(scanType) {
    switch (scanType) {
      case 'secrets':
        await this.scanForSecrets();
        break;
      case 'vulnerabilities':
        await this.scanForVulnerabilities();
        break;
      case 'dependencies':
        await this.scanDependencies();
        break;
      case 'config':
        await this.scanConfiguration();
        break;
      default:
        console.warn(`⚠️  Unknown scan type: ${scanType}`);
    }
  }

  /**
   * Scan for exposed secrets and sensitive data
   */
  async scanForSecrets() {
    const secretPatterns = [
      {
        pattern: /(?:password|pwd|pass)\s*[:=]\s*['"`]([^'"`\s]+)['"`]/gi,
        type: 'password',
        severity: 'high',
      },
      {
        pattern: /(?:api[_-]?key|apikey)\s*[:=]\s*['"`]([^'"`\s]+)['"`]/gi,
        type: 'api_key',
        severity: 'critical',
      },
      {
        pattern: /(?:secret|token)\s*[:=]\s*['"`]([^'"`\s]+)['"`]/gi,
        type: 'secret_token',
        severity: 'high',
      },
      {
        pattern:
          /(?:private[_-]?key|privatekey)\s*[:=]\s*['"`]([^'"`\s]+)['"`]/gi,
        type: 'private_key',
        severity: 'critical',
      },
      {
        pattern:
          /(?:db[_-]?password|database[_-]?password)\s*[:=]\s*['"`]([^'"`\s]+)['"`]/gi,
        type: 'db_password',
        severity: 'high',
      },
      {
        pattern:
          /(?:jwt[_-]?secret|jwt[_-]?key)\s*[:=]\s*['"`]([^'"`\s]+)['"`]/gi,
        type: 'jwt_secret',
        severity: 'high',
      },
      {
        pattern:
          /(?:aws[_-]?access[_-]?key|aws[_-]?secret)\s*[:=]\s*['"`]([^'"`\s]+)['"`]/gi,
        type: 'aws_credentials',
        severity: 'critical',
      },
    ];

    const excludePatterns = [
      /test[_-]?password/i,
      /dummy[_-]?key/i,
      /example[_-]?secret/i,
      /placeholder/i,
      /your[_-]?key[_-]?here/i,
    ];

    const fileExtensions = ['.js', '.ts', '.json', '.env', '.config.js', '.rs'];
    const filesToScan = this.getFilesToScan(fileExtensions);

    for (const filePath of filesToScan) {
      try {
        const content = fs.readFileSync(filePath, 'utf8');
        const lines = content.split('\n');

        lines.forEach((line, lineIndex) => {
          for (const { pattern, type, severity } of secretPatterns) {
            let match;
            while ((match = pattern.exec(line)) !== null) {
              const value = match[1];

              // Skip if matches exclude patterns
              const isExcluded = excludePatterns.some(exclude =>
                exclude.test(line)
              );
              if (isExcluded) continue;

              // Skip obvious test/example values
              if (
                value.length < 8 ||
                /^(test|demo|example|placeholder)$/i.test(value)
              )
                continue;

              this.addVulnerability({
                type: 'secrets',
                category: type,
                severity,
                file: path.relative(this.projectRoot, filePath),
                line: lineIndex + 1,
                description: `Potential ${type.replace('_', ' ')} exposed in source code`,
                evidence: line.trim(),
                recommendation: `Move ${type.replace('_', ' ')} to environment variables or secure vault`,
              });
            }
          }
        });
      } catch (error) {
        console.warn(`⚠️  Could not scan file: ${filePath} - ${error.message}`);
      }
    }
  }

  /**
   * Scan for common code vulnerabilities
   */
  async scanForVulnerabilities() {
    const vulnerabilityPatterns = [
      // SQL Injection patterns
      {
        pattern: /\$\{[^}]*\}.*(?:SELECT|INSERT|UPDATE|DELETE)/gi,
        type: 'sql_injection',
        severity: 'critical',
        description: 'Potential SQL injection via template literals',
      },
      {
        pattern: /query\s*\+\s*['"]/gi,
        type: 'sql_injection',
        severity: 'high',
        description: 'SQL query concatenation without parameterization',
      },

      // XSS patterns
      {
        pattern: /innerHTML\s*=\s*[^;]*\+/gi,
        type: 'xss',
        severity: 'high',
        description: 'Potential XSS via innerHTML concatenation',
      },
      {
        pattern: /document\.write\s*\([^)]*\+/gi,
        type: 'xss',
        severity: 'high',
        description: 'Potential XSS via document.write concatenation',
      },

      // Command injection patterns
      {
        pattern: /exec\s*\(\s*[^)]*\+/gi,
        type: 'command_injection',
        severity: 'critical',
        description: 'Potential command injection via exec concatenation',
      },
      {
        pattern: /system\s*\(\s*[^)]*\+/gi,
        type: 'command_injection',
        severity: 'critical',
        description: 'Potential command injection via system concatenation',
      },

      // Path traversal patterns
      {
        pattern: /\.\.\/|\.\.\\/gi,
        type: 'path_traversal',
        severity: 'medium',
        description: 'Potential path traversal pattern',
      },

      // Weak crypto patterns
      {
        pattern: /md5|sha1(?!256)/gi,
        type: 'weak_crypto',
        severity: 'medium',
        description: 'Use of weak cryptographic algorithm',
      },
      {
        pattern: /Math\.random\(\)/gi,
        type: 'weak_random',
        severity: 'low',
        description:
          'Use of weak random number generator for security purposes',
      },

      // Unsafe eval patterns
      {
        pattern: /eval\s*\(/gi,
        type: 'code_injection',
        severity: 'critical',
        description: 'Use of dangerous eval() function',
      },
      {
        pattern: /Function\s*\(\s*['"][^'"]*['"]\s*\)/gi,
        type: 'code_injection',
        severity: 'high',
        description: 'Dynamic code execution via Function constructor',
      },
    ];

    const fileExtensions = ['.js', '.ts', '.rs'];
    const filesToScan = this.getFilesToScan(fileExtensions);

    for (const filePath of filesToScan) {
      try {
        const content = fs.readFileSync(filePath, 'utf8');
        const lines = content.split('\n');

        lines.forEach((line, lineIndex) => {
          for (const {
            pattern,
            type,
            severity,
            description,
          } of vulnerabilityPatterns) {
            if (pattern.test(line)) {
              this.addVulnerability({
                type: 'vulnerability',
                category: type,
                severity,
                file: path.relative(this.projectRoot, filePath),
                line: lineIndex + 1,
                description,
                evidence: line.trim(),
                recommendation: this.getVulnerabilityRecommendation(type),
              });
            }
          }
        });
      } catch (error) {
        console.warn(`⚠️  Could not scan file: ${filePath} - ${error.message}`);
      }
    }
  }

  /**
   * Scan dependencies for known vulnerabilities
   */
  async scanDependencies() {
    const packageJsonFiles = this.findFiles('package.json');
    const cargoTomlFiles = this.findFiles('Cargo.toml');

    // Scan Node.js dependencies
    for (const packageFile of packageJsonFiles) {
      try {
        const packageData = JSON.parse(fs.readFileSync(packageFile, 'utf8'));
        const dependencies = {
          ...packageData.dependencies,
          ...packageData.devDependencies,
        };

        // Check for known vulnerable packages (simplified check)
        const vulnerablePackages = [
          {
            name: 'lodash',
            versions: ['<4.17.21'],
            severity: 'high',
            cve: 'CVE-2021-23337',
          },
          {
            name: 'minimist',
            versions: ['<1.2.6'],
            severity: 'critical',
            cve: 'CVE-2021-44906',
          },
          {
            name: 'node-forge',
            versions: ['<1.3.0'],
            severity: 'high',
            cve: 'CVE-2022-24771',
          },
        ];

        for (const [depName, depVersion] of Object.entries(dependencies)) {
          const vulnPackage = vulnerablePackages.find(
            pkg => pkg.name === depName
          );
          if (vulnPackage) {
            this.addVulnerability({
              type: 'dependency',
              category: 'vulnerable_package',
              severity: vulnPackage.severity,
              file: path.relative(this.projectRoot, packageFile),
              line: 1,
              description: `Vulnerable dependency: ${depName}@${depVersion}`,
              evidence: `"${depName}": "${depVersion}"`,
              recommendation: `Update ${depName} to latest secure version`,
              cve: vulnPackage.cve,
            });
          }
        }
      } catch (error) {
        console.warn(
          `⚠️  Could not scan package.json: ${packageFile} - ${error.message}`
        );
      }
    }

    // Scan Rust dependencies (basic check)
    for (const cargoFile of cargoTomlFiles) {
      try {
        const content = fs.readFileSync(cargoFile, 'utf8');

        // Look for potentially outdated crates (simplified)
        if (content.includes('rand = "0.7"')) {
          this.addVulnerability({
            type: 'dependency',
            category: 'outdated_crate',
            severity: 'medium',
            file: path.relative(this.projectRoot, cargoFile),
            line: 1,
            description: 'Potentially outdated Rust crate version',
            evidence: 'rand = "0.7"',
            recommendation: 'Update to latest version and run cargo audit',
          });
        }
      } catch (error) {
        console.warn(
          `⚠️  Could not scan Cargo.toml: ${cargoFile} - ${error.message}`
        );
      }
    }
  }

  /**
   * Scan configuration files for security issues
   */
  async scanConfiguration() {
    const configPatterns = [
      {
        pattern: /debug\s*[:=]\s*true/gi,
        type: 'debug_enabled',
        severity: 'medium',
        file: ['.env', '.config'],
      },
      {
        pattern: /cors\s*[:=]\s*\*/gi,
        type: 'permissive_cors',
        severity: 'high',
        file: ['.js', '.ts'],
      },
      {
        pattern: /ssl\s*[:=]\s*false/gi,
        type: 'ssl_disabled',
        severity: 'high',
        file: ['.env', '.config'],
      },
      {
        pattern: /secure\s*[:=]\s*false/gi,
        type: 'insecure_config',
        severity: 'medium',
        file: ['.js', '.ts'],
      },
    ];

    const configFiles = [
      ...this.findFiles('.env'),
      ...this.findFiles('config.js'),
      ...this.findFiles('config.ts'),
      ...this.findFiles('.config.js'),
    ];

    for (const filePath of configFiles) {
      try {
        const content = fs.readFileSync(filePath, 'utf8');
        const lines = content.split('\n');

        lines.forEach((line, lineIndex) => {
          for (const { pattern, type, severity } of configPatterns) {
            if (pattern.test(line)) {
              this.addVulnerability({
                type: 'configuration',
                category: type,
                severity,
                file: path.relative(this.projectRoot, filePath),
                line: lineIndex + 1,
                description: `Insecure configuration: ${type.replace('_', ' ')}`,
                evidence: line.trim(),
                recommendation: this.getConfigRecommendation(type),
              });
            }
          }
        });
      } catch (error) {
        console.warn(
          `⚠️  Could not scan config file: ${filePath} - ${error.message}`
        );
      }
    }
  }

  /**
   * Add vulnerability to results
   */
  addVulnerability(vulnerability) {
    this.vulnerabilities.push(vulnerability);
    this.scanResults.vulnerabilities.push(vulnerability);

    // Update counters
    this.scanResults.summary.totalVulnerabilities++;
    switch (vulnerability.severity) {
      case 'critical':
        this.scanResults.summary.criticalCount++;
        break;
      case 'high':
        this.scanResults.summary.highCount++;
        break;
      case 'medium':
        this.scanResults.summary.mediumCount++;
        break;
      case 'low':
        this.scanResults.summary.lowCount++;
        break;
    }
  }

  /**
   * Get files to scan based on extensions
   */
  getFilesToScan(extensions) {
    const files = [];

    for (const ext of extensions) {
      files.push(...this.findFiles(`*${ext}`));
    }

    // Filter out node_modules, build, dist, etc.
    return files.filter(
      file =>
        !file.includes('node_modules') &&
        !file.includes('build/') &&
        !file.includes('dist/') &&
        !file.includes('.git/') &&
        !file.includes('target/') &&
        !file.includes('test/reports/')
    );
  }

  /**
   * Find files by pattern
   */
  findFiles(pattern) {
    try {
      const command = `find ${this.projectRoot} -name "${pattern}" -type f`;
      const output = execSync(command, { encoding: 'utf8' });
      return output
        .trim()
        .split('\n')
        .filter(line => line.length > 0);
    } catch (error) {
      return [];
    }
  }

  /**
   * Get vulnerability-specific recommendations
   */
  getVulnerabilityRecommendation(type) {
    const recommendations = {
      sql_injection: 'Use parameterized queries or prepared statements',
      xss: 'Sanitize user input and use secure DOM manipulation methods',
      command_injection:
        'Validate and sanitize command arguments, use safe APIs',
      path_traversal: 'Validate file paths and use safe path resolution',
      weak_crypto: 'Use strong cryptographic algorithms (SHA-256, bcrypt)',
      weak_random: 'Use cryptographically secure random generators',
      code_injection: 'Avoid dynamic code execution, use safe alternatives',
    };

    return recommendations[type] || 'Review and secure the identified pattern';
  }

  /**
   * Get configuration-specific recommendations
   */
  getConfigRecommendation(type) {
    const recommendations = {
      debug_enabled: 'Disable debug mode in production environments',
      permissive_cors: 'Configure CORS with specific allowed origins',
      ssl_disabled: 'Enable SSL/TLS for secure communication',
      insecure_config: 'Review and harden security configuration',
    };

    return (
      recommendations[type] || 'Review configuration for security implications'
    );
  }

  /**
   * Generate JSON report
   */
  async generateJSONReport() {
    const reportPath = path.join(this.reportDir, 'sast-scan-results.json');
    fs.writeFileSync(reportPath, JSON.stringify(this.scanResults, null, 2));
    console.log(`📄 JSON report generated: ${reportPath}`);
  }

  /**
   * Generate HTML report
   */
  async generateHTMLReport() {
    const htmlTemplate = `
<!DOCTYPE html>
<html>
<head>
    <title>SAST Security Scan Report</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; background: #f5f5f5; }
        .header { background: #d32f2f; color: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; }
        .summary { background: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        .summary-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 15px; }
        .summary-card { text-align: center; padding: 15px; border-radius: 8px; }
        .critical { background: #ffebee; border-left: 5px solid #f44336; }
        .high { background: #fff3e0; border-left: 5px solid #ff9800; }
        .medium { background: #f3e5f5; border-left: 5px solid #9c27b0; }
        .low { background: #e8f5e8; border-left: 5px solid #4caf50; }
        .vulnerability { background: white; margin: 10px 0; padding: 15px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        .vuln-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 10px; }
        .severity-badge { padding: 5px 10px; border-radius: 15px; color: white; font-size: 0.8em; }
        .code-evidence { background: #f5f5f5; padding: 10px; border-radius: 4px; font-family: monospace; overflow-x: auto; }
        .recommendation { background: #e8f5e8; padding: 10px; border-radius: 4px; margin-top: 10px; }
    </style>
</head>
<body>
    <div class="header">
        <h1>🔒 SAST Security Scan Report</h1>
        <p>Generated: ${this.scanResults.metadata.timestamp}</p>
    </div>

    <div class="summary">
        <h2>Scan Summary</h2>
        <div class="summary-grid">
            <div class="summary-card critical">
                <h3>${this.scanResults.summary.criticalCount}</h3>
                <p>Critical</p>
            </div>
            <div class="summary-card high">
                <h3>${this.scanResults.summary.highCount}</h3>
                <p>High</p>
            </div>
            <div class="summary-card medium">
                <h3>${this.scanResults.summary.mediumCount}</h3>
                <p>Medium</p>
            </div>
            <div class="summary-card low">
                <h3>${this.scanResults.summary.lowCount}</h3>
                <p>Low</p>
            </div>
        </div>
        <p><strong>Total Vulnerabilities:</strong> ${this.scanResults.summary.totalVulnerabilities}</p>
    </div>

    <h2>Detailed Findings</h2>
    ${this.scanResults.vulnerabilities
      .map(
        vuln => `
        <div class="vulnerability ${vuln.severity}">
            <div class="vuln-header">
                <h3>${vuln.description}</h3>
                <span class="severity-badge" style="background: ${this.getSeverityColor(vuln.severity)}">${vuln.severity.toUpperCase()}</span>
            </div>
            <p><strong>File:</strong> ${vuln.file}:${vuln.line}</p>
            <p><strong>Type:</strong> ${vuln.category}</p>
            ${vuln.cve ? `<p><strong>CVE:</strong> ${vuln.cve}</p>` : ''}
            <div class="code-evidence">
                <strong>Evidence:</strong><br>
                ${vuln.evidence}
            </div>
            <div class="recommendation">
                <strong>Recommendation:</strong> ${vuln.recommendation}
            </div>
        </div>
    `
      )
      .join('')}
</body>
</html>`;

    const htmlPath = path.join(this.reportDir, 'sast-scan-report.html');
    fs.writeFileSync(htmlPath, htmlTemplate);
    console.log(`📊 HTML report generated: ${htmlPath}`);
  }

  /**
   * Get severity color for HTML report
   */
  getSeverityColor(severity) {
    const colors = {
      critical: '#f44336',
      high: '#ff9800',
      medium: '#9c27b0',
      low: '#4caf50',
    };
    return colors[severity] || '#757575';
  }

  /**
   * Print scan summary
   */
  printScanSummary() {
    console.log('\n' + '='.repeat(60));
    console.log('🔒 SAST SECURITY SCAN SUMMARY');
    console.log('='.repeat(60));
    console.log(
      `📊 Total Vulnerabilities: ${this.scanResults.summary.totalVulnerabilities}`
    );
    console.log(`🚨 Critical: ${this.scanResults.summary.criticalCount}`);
    console.log(`⚠️  High: ${this.scanResults.summary.highCount}`);
    console.log(`⚡ Medium: ${this.scanResults.summary.mediumCount}`);
    console.log(`📝 Low: ${this.scanResults.summary.lowCount}`);

    if (this.scanResults.summary.criticalCount > 0) {
      console.log('\n🚨 CRITICAL ISSUES FOUND - Immediate attention required!');
    } else if (this.scanResults.summary.highCount > 0) {
      console.log('\n⚠️  HIGH SEVERITY ISSUES - Review and fix promptly');
    } else {
      console.log('\n✅ No critical or high severity issues found');
    }

    console.log('='.repeat(60));
  }
}

// Export for use in other test files
module.exports = {
  SASTScanner,
};

// Main execution function
async function runSASTScan(options = {}) {
  const scanner = new SASTScanner();
  return await scanner.runComprehensiveScan(options);
}

// Run scan if called directly
if (require.main === module) {
  runSASTScan({
    scanTypes: ['secrets', 'vulnerabilities', 'dependencies', 'config'],
    outputFormats: ['json', 'html'],
    severity: 'low',
  })
    .then(results => {
      const hasHighSeverity =
        results.summary.criticalCount > 0 || results.summary.highCount > 0;
      process.exit(hasHighSeverity ? 1 : 0);
    })
    .catch(error => {
      console.error('SAST scan execution failed:', error);
      process.exit(1);
    });
}
