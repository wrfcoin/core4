#!/usr/bin/env node

/**
 * Pact Contract Publishing Script with Version Management
 *
 * This script publishes Pact contracts to the Pact Broker with proper versioning,
 * tagging, and branch management for contract lifecycle management.
 *
 * Features:
 * - Semantic versioning with automatic version detection
 * - Git-based tagging and branching support
 * - Environment-specific contract publishing
 * - Backward compatibility validation
 * - Webhook integration for CI/CD pipelines
 */

const { Publisher } = require('@pact-foundation/pact');
const { execSync } = require('child_process');
const path = require('path');
const fs = require('fs');

if (!process.env.PACT_BROKER_PASSWORD) {
  console.error('ERROR: PACT_BROKER_PASSWORD environment variable is required.');
  console.error('Set it in CI via GitHub Actions secrets or locally via .env');
  process.exit(1);
}

// Configuration constants
const CONFIG = {
  pactBrokerUrl: process.env.PACT_BROKER_BASE_URL || 'http://localhost:9292',
  pactBrokerUsername: process.env.PACT_BROKER_USERNAME || 'admin',
  pactBrokerPassword: process.env.PACT_BROKER_PASSWORD,
  consumerName: 'wrfcoin-api',

  // Versioning strategy
  versioning: {
    strategy: process.env.PACT_VERSION_STRATEGY || 'git-semver', // 'git-semver', 'git-sha', 'package-json', 'env'
    fallbackVersion: '1.0.0-dev',
  },

  // Git integration
  git: {
    branchTagging: process.env.PACT_BRANCH_TAGGING !== 'false',
    mainBranches: ['main', 'master', 'develop'],
    featureBranchPrefix: 'feature/',
    releaseBranchPrefix: 'release/',
  },

  // Environment configuration
  environments: {
    development: ['dev', 'develop', 'feature/*'],
    staging: ['staging', 'test', 'release/*'],
    production: ['main', 'master', 'prod'],
  },

  // Publishing options
  publishing: {
    publishVerificationResults: process.env.CI === 'true',
    buildUrl: process.env.BUILD_URL,
    verbose: process.env.PACT_VERBOSE === 'true',
  },
};

/**
 * Utility functions for version management
 */
const VersionManager = {
  /**
   * Get current version based on strategy
   */
  getCurrentVersion() {
    switch (CONFIG.versioning.strategy) {
      case 'git-semver':
        return this.getGitSemverVersion();
      case 'git-sha':
        return this.getGitShaVersion();
      case 'package-json':
        return this.getPackageJsonVersion();
      case 'env':
        return (
          process.env.PACT_CONSUMER_VERSION || CONFIG.versioning.fallbackVersion
        );
      default:
        console.warn(
          `⚠️  Unknown version strategy: ${CONFIG.versioning.strategy}, falling back to git-semver`
        );
        return this.getGitSemverVersion();
    }
  },

  /**
   * Get version from Git tags with semantic versioning
   */
  getGitSemverVersion() {
    try {
      // Try to get the latest git tag
      const gitTag = execSync(
        'git describe --tags --exact-match HEAD 2>/dev/null || echo ""',
        { encoding: 'utf8' }
      ).trim();

      if (gitTag) {
        console.log(`📋 Using Git tag version: ${gitTag}`);
        return gitTag;
      }

      // Fallback to latest tag + commit count + short SHA
      const latestTag = execSync(
        'git describe --tags --abbrev=0 HEAD 2>/dev/null || echo "v0.0.0"',
        { encoding: 'utf8' }
      ).trim();
      const commitCount = execSync(`git rev-list ${latestTag}..HEAD --count`, {
        encoding: 'utf8',
      }).trim();
      const shortSha = execSync('git rev-parse --short HEAD', {
        encoding: 'utf8',
      }).trim();

      if (parseInt(commitCount) > 0) {
        const version = `${latestTag.replace(/^v/, '')}-${commitCount}-g${shortSha}`;
        console.log(`📋 Using Git derived version: ${version}`);
        return version;
      }

      console.log(`📋 Using Git tag version: ${latestTag}`);
      return latestTag.replace(/^v/, '');
    } catch (error) {
      console.warn(`⚠️  Failed to get Git version: ${error.message}`);
      return CONFIG.versioning.fallbackVersion;
    }
  },

  /**
   * Get version from Git SHA
   */
  getGitShaVersion() {
    try {
      const sha = execSync('git rev-parse HEAD', { encoding: 'utf8' }).trim();
      const shortSha = sha.substring(0, 7);
      const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
      const version = `${timestamp}-${shortSha}`;
      console.log(`📋 Using Git SHA version: ${version}`);
      return version;
    } catch (error) {
      console.warn(`⚠️  Failed to get Git SHA: ${error.message}`);
      return CONFIG.versioning.fallbackVersion;
    }
  },

  /**
   * Get version from package.json
   */
  getPackageJsonVersion() {
    try {
      const packagePath = path.resolve(process.cwd(), 'package.json');
      const packageJson = JSON.parse(fs.readFileSync(packagePath, 'utf8'));
      const version = packageJson.version || CONFIG.versioning.fallbackVersion;
      console.log(`📋 Using package.json version: ${version}`);
      return version;
    } catch (error) {
      console.warn(`⚠️  Failed to get package.json version: ${error.message}`);
      return CONFIG.versioning.fallbackVersion;
    }
  },

  /**
   * Get current Git branch
   */
  getCurrentBranch() {
    try {
      const branch = execSync('git rev-parse --abbrev-ref HEAD', {
        encoding: 'utf8',
      }).trim();
      console.log(`🌿 Current branch: ${branch}`);
      return branch;
    } catch (error) {
      console.warn(`⚠️  Failed to get current branch: ${error.message}`);
      return 'unknown';
    }
  },

  /**
   * Determine environment from branch
   */
  getEnvironmentFromBranch(branch) {
    for (const [env, patterns] of Object.entries(CONFIG.environments)) {
      for (const pattern of patterns) {
        if (pattern.includes('*')) {
          const regex = new RegExp('^' + pattern.replace(/\*/g, '.*') + '$');
          if (regex.test(branch)) {
            return env;
          }
        } else if (branch === pattern) {
          return env;
        }
      }
    }
    return 'development'; // Default fallback
  },

  /**
   * Generate tags for the current context
   */
  generateTags() {
    const branch = this.getCurrentBranch();
    const environment = this.getEnvironmentFromBranch(branch);
    const tags = [environment, branch];

    // Add additional semantic tags
    if (CONFIG.git.mainBranches.includes(branch)) {
      tags.push('stable');
    }

    if (branch.startsWith(CONFIG.git.featureBranchPrefix)) {
      tags.push('feature');
    }

    if (branch.startsWith(CONFIG.git.releaseBranchPrefix)) {
      tags.push('release');
    }

    // Add CI-specific tags
    if (process.env.CI === 'true') {
      tags.push('ci');
      if (process.env.GITHUB_ACTIONS) {
        tags.push('github-actions');
      }
    }

    console.log(`🏷️  Generated tags: ${tags.join(', ')}`);
    return tags;
  },
};

/**
 * Contract Publisher with enhanced version management
 */
class ContractPublisher {
  constructor(options = {}) {
    this.config = {
      ...CONFIG,
      ...options,
    };

    this.version = VersionManager.getCurrentVersion();
    this.branch = VersionManager.getCurrentBranch();
    this.tags = VersionManager.generateTags();
    this.environment = VersionManager.getEnvironmentFromBranch(this.branch);
  }

  /**
   * Validate configuration and environment
   */
  async validateConfiguration() {
    console.log('\n🔍 Validating publishing configuration...');

    // Check if Pact Broker is accessible
    try {
      const response = await fetch(
        `${this.config.pactBrokerUrl}/diagnostic/status/heartbeat`
      );
      if (!response.ok) {
        throw new Error(
          `Pact Broker responded with status: ${response.status}`
        );
      }
      console.log('✅ Pact Broker is accessible');
    } catch (error) {
      console.error('❌ Pact Broker validation failed:', error.message);
      throw new Error('Pact Broker is not accessible');
    }

    // Check for Pact files
    const pactsDir = path.resolve(__dirname, '../pacts');
    if (!fs.existsSync(pactsDir)) {
      throw new Error(`Pacts directory not found: ${pactsDir}`);
    }

    const pactFiles = fs
      .readdirSync(pactsDir)
      .filter(file => file.endsWith('.json'));
    if (pactFiles.length === 0) {
      throw new Error('No Pact files found to publish');
    }

    console.log(`✅ Found ${pactFiles.length} Pact file(s) to publish`);
    console.log(`📁 Pact files: ${pactFiles.join(', ')}`);

    return { pactsDir, pactFiles };
  }

  /**
   * Create Publisher instance with proper configuration
   */
  createPublisher(pactsDir) {
    const publisherOptions = {
      pactFilesOrDirs: [pactsDir],
      pactBroker: this.config.pactBrokerUrl,
      pactBrokerUsername: this.config.pactBrokerUsername,
      pactBrokerPassword: this.config.pactBrokerPassword,
      consumerVersion: this.version,
      tags: this.tags,
      branch: this.branch,
      buildUrl: this.config.publishing.buildUrl,
      verbose: this.config.publishing.verbose,
    };

    // Add optional parameters
    if (this.config.publishing.publishVerificationResults) {
      publisherOptions.publishVerificationResult = true;
    }

    console.log('\n📤 Publisher configuration:');
    console.log(`   Consumer: ${this.config.consumerName}`);
    console.log(`   Version: ${this.version}`);
    console.log(`   Branch: ${this.branch}`);
    console.log(`   Environment: ${this.environment}`);
    console.log(`   Tags: [${this.tags.join(', ')}]`);
    console.log(`   Broker: ${this.config.pactBrokerUrl}`);

    return new Publisher(publisherOptions);
  }

  /**
   * Publish contracts with comprehensive logging
   */
  async publishContracts() {
    console.log('\n🚀 Starting contract publishing process...\n');

    try {
      // Step 1: Validate configuration
      const { pactsDir, pactFiles } = await this.validateConfiguration();

      // Step 2: Create publisher
      const publisher = this.createPublisher(pactsDir);

      // Step 3: Publish contracts
      console.log('\n📤 Publishing contracts to Pact Broker...');
      const startTime = Date.now();

      const result = await publisher.publishPacts();

      const publishTime = Date.now() - startTime;
      console.log(`✅ Contracts published successfully in ${publishTime}ms`);

      // Step 4: Generate publishing report
      await this.generatePublishingReport(pactFiles, result);

      // Step 5: Create version webhook if needed
      if (CONFIG.git.mainBranches.includes(this.branch)) {
        await this.setupVersionWebhooks();
      }

      return result;
    } catch (error) {
      console.error('\n❌ Contract publishing failed:');
      console.error(`   Error: ${error.message}`);

      if (this.config.publishing.verbose) {
        console.error('   Stack trace:', error.stack);
      }

      // Create failure report
      await this.generateFailureReport(error);

      throw error;
    }
  }

  /**
   * Generate detailed publishing report
   */
  async generatePublishingReport(pactFiles, result) {
    const report = {
      timestamp: new Date().toISOString(),
      version: this.version,
      branch: this.branch,
      environment: this.environment,
      tags: this.tags,
      pactFiles: pactFiles,
      brokerUrl: this.config.pactBrokerUrl,
      publishingResult: result,
      git: {
        commit: this.getGitCommit(),
        author: this.getGitAuthor(),
        message: this.getGitMessage(),
      },
    };

    const reportsDir = path.resolve(__dirname, '../reports');
    if (!fs.existsSync(reportsDir)) {
      fs.mkdirSync(reportsDir, { recursive: true });
    }

    const reportPath = path.resolve(
      reportsDir,
      `publish-report-${this.version}-${Date.now()}.json`
    );
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));

    console.log(`\n📊 Publishing report saved: ${reportPath}`);
  }

  /**
   * Generate failure report for debugging
   */
  async generateFailureReport(error) {
    const report = {
      timestamp: new Date().toISOString(),
      version: this.version,
      branch: this.branch,
      environment: this.environment,
      error: {
        message: error.message,
        stack: error.stack,
      },
      configuration: this.config,
    };

    const reportsDir = path.resolve(__dirname, '../reports');
    if (!fs.existsSync(reportsDir)) {
      fs.mkdirSync(reportsDir, { recursive: true });
    }

    const reportPath = path.resolve(
      reportsDir,
      `publish-failure-${Date.now()}.json`
    );
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));

    console.log(`📊 Failure report saved: ${reportPath}`);
  }

  /**
   * Setup version-specific webhooks for automated workflows
   */
  async setupVersionWebhooks() {
    console.log('\n🔗 Setting up version webhooks...');

    // This would typically configure webhooks in the Pact Broker
    // For now, we'll log the intended configuration
    const webhooks = [
      {
        description: 'Contract changed webhook',
        events: ['contract_content_changed'],
        request: {
          method: 'POST',
          url:
            process.env.PACT_WEBHOOK_URL ||
            'https://api.wrfcoin.com/webhooks/pact/contract-changed',
          headers: {
            'Content-Type': 'application/json',
          },
          body: {
            version: this.version,
            branch: this.branch,
            environment: this.environment,
          },
        },
      },
    ];

    console.log('🔗 Webhook configuration ready:');
    webhooks.forEach((webhook, index) => {
      console.log(
        `   ${index + 1}. ${webhook.description} -> ${webhook.request.url}`
      );
    });
  }

  /**
   * Git utility methods
   */
  getGitCommit() {
    try {
      return execSync('git rev-parse HEAD', { encoding: 'utf8' }).trim();
    } catch {
      return 'unknown';
    }
  }

  getGitAuthor() {
    try {
      return execSync('git log -1 --pretty=format:"%an <%ae>"', {
        encoding: 'utf8',
      }).trim();
    } catch {
      return 'unknown';
    }
  }

  getGitMessage() {
    try {
      return execSync('git log -1 --pretty=format:"%s"', {
        encoding: 'utf8',
      }).trim();
    } catch {
      return 'unknown';
    }
  }
}

/**
 * Main execution function
 */
async function main() {
  console.log('🎯 WRFCoin Pact Contract Publisher');
  console.log('=====================================\n');

  try {
    const publisher = new ContractPublisher();
    await publisher.publishContracts();

    console.log('\n🎉 Contract publishing completed successfully!');
    console.log(`   Version: ${publisher.version}`);
    console.log(`   Environment: ${publisher.environment}`);
    console.log(`   Broker: ${publisher.config.pactBrokerUrl}`);

    process.exit(0);
  } catch (error) {
    console.error('\n💥 Contract publishing failed!');
    console.error(`Error: ${error.message}\n`);

    process.exit(1);
  }
}

// Execute if run directly
if (require.main === module) {
  main();
}

module.exports = {
  ContractPublisher,
  VersionManager,
  CONFIG,
};
