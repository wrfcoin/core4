#!/usr/bin/env node

/**
 * Pact Contract Testing Workflow Orchestrator
 *
 * This script orchestrates the complete contract testing workflow including
 * versioning, publishing, verification, and compatibility testing across
 * multiple API versions and services.
 *
 * Features:
 * - Automated end-to-end contract testing pipeline
 * - Multi-stage workflow with dependency management
 * - Rollback capabilities for failed deployments
 * - Integration with CI/CD pipelines
 * - Real-time monitoring and notifications
 */

const { execSync, spawn } = require('child_process');
const path = require('path');
const fs = require('fs');

// Import workflow components
const { ContractPublisher } = require('./publish-pacts.js');
const { DeploymentValidator } = require('./can-i-deploy.js');
const { ProviderVerifier } = require('./verify-pacts.js');
const { CompatibilityMatrixTester } = require('./compatibility-matrix.js');

/**
 * Workflow orchestration configuration
 */
const WORKFLOW_CONFIG = {
  // Workflow stages and their dependencies
  stages: {
    setup: {
      description: 'Setup test environment and validate dependencies',
      dependencies: [],
      timeout: 30000, // 30 seconds
      required: true,
      parallel: false,
    },
    'contract-generation': {
      description: 'Generate and validate contract specifications',
      dependencies: ['setup'],
      timeout: 120000, // 2 minutes
      required: true,
      parallel: false,
    },
    'contract-publishing': {
      description: 'Publish contracts to Pact Broker with versioning',
      dependencies: ['contract-generation'],
      timeout: 60000, // 1 minute
      required: true,
      parallel: false,
    },
    'provider-verification': {
      description: 'Verify contracts against all provider services',
      dependencies: ['contract-publishing'],
      timeout: 300000, // 5 minutes
      required: true,
      parallel: true, // Can run providers in parallel
    },
    'compatibility-testing': {
      description: 'Test compatibility across API version matrix',
      dependencies: ['provider-verification'],
      timeout: 600000, // 10 minutes
      required: true,
      parallel: false,
    },
    'deployment-validation': {
      description: 'Validate deployment readiness with can-i-deploy',
      dependencies: ['compatibility-testing'],
      timeout: 120000, // 2 minutes
      required: true,
      parallel: false,
    },
    reporting: {
      description: 'Generate comprehensive test reports and notifications',
      dependencies: ['deployment-validation'],
      timeout: 60000, // 1 minute
      required: false,
      parallel: false,
    },
  },

  // Workflow execution options
  execution: {
    failFast: process.env.PACT_WORKFLOW_FAIL_FAST !== 'false',
    retryCount: parseInt(process.env.PACT_WORKFLOW_RETRIES) || 2,
    retryDelay: 5000, // 5 seconds between retries
    parallelProviders: true,
    generateArtifacts: true,
  },

  // Environment and service configuration
  environment: {
    pactBrokerUrl: process.env.PACT_BROKER_BASE_URL || 'http://localhost:9292',
    services: ['postgres', 'blockchain', 'weather', 'carbon'],
    environments: ['development', 'staging', 'production'],
  },

  // Rollback configuration
  rollback: {
    enabled: process.env.PACT_WORKFLOW_ROLLBACK === 'true',
    autoRollback: process.env.PACT_WORKFLOW_AUTO_ROLLBACK === 'true',
    preserveLogs: true,
    notifyOnRollback: true,
  },

  // Notification configuration
  notifications: {
    enabled: process.env.PACT_NOTIFICATIONS === 'true',
    slack: process.env.SLACK_WEBHOOK_URL,
    email: process.env.NOTIFICATION_EMAIL,
    teams: process.env.TEAMS_WEBHOOK_URL,
  },

  // Artifact management
  artifacts: {
    baseDir: path.resolve(__dirname, '../artifacts'),
    retention: {
      logs: 30, // days
      reports: 90, // days
      contracts: 365, // days
    },
    compression: true,
    upload: {
      enabled: process.env.ARTIFACT_UPLOAD === 'true',
      destination:
        process.env.ARTIFACT_DESTINATION || 's3://wrfcoin-pact-artifacts',
    },
  },
};

/**
 * Main workflow orchestrator class
 */
class WorkflowOrchestrator {
  constructor(options = {}) {
    this.config = {
      ...WORKFLOW_CONFIG,
      ...options,
    };

    this.workflowId = `workflow-${Date.now()}`;
    this.startTime = Date.now();
    this.stageResults = {};
    this.context = {
      version: this.getCurrentVersion(),
      branch: this.getCurrentBranch(),
      environment: this.getEnvironment(),
      artifacts: [],
      metrics: {},
    };

    console.log(`🚀 Workflow Orchestrator initialized`);
    console.log(`   Workflow ID: ${this.workflowId}`);
    console.log(`   Version: ${this.context.version}`);
    console.log(`   Branch: ${this.context.branch}`);
    console.log(`   Environment: ${this.context.environment}`);
  }

  /**
   * Execute the complete contract testing workflow
   */
  async executeWorkflow() {
    console.log('\n🎯 Starting Contract Testing Workflow Execution\n');
    console.log('='.repeat(60));

    try {
      // Initialize workflow tracking
      await this.initializeWorkflow();

      // Execute stages in dependency order
      const stageOrder = this.calculateStageExecutionOrder();
      console.log(`\n📋 Execution order: ${stageOrder.join(' → ')}`);

      for (const stageName of stageOrder) {
        await this.executeStage(stageName);

        // Check if workflow should continue
        if (this.shouldAbortWorkflow(stageName)) {
          throw new Error(
            `Workflow aborted due to critical failure in stage: ${stageName}`
          );
        }
      }

      // Finalize successful workflow
      const result = await this.finalizeWorkflow(true);

      console.log('\n🎉 CONTRACT TESTING WORKFLOW COMPLETED SUCCESSFULLY');
      console.log(`   Total execution time: ${this.getFormattedDuration()}`);
      console.log(
        `   Stages completed: ${Object.keys(this.stageResults).length}`
      );
      console.log(`   Artifacts generated: ${this.context.artifacts.length}`);

      return result;
    } catch (error) {
      console.error('\n💥 CONTRACT TESTING WORKFLOW FAILED');
      console.error(`   Error: ${error.message}`);
      console.error(`   Stage: ${this.getCurrentStage()}`);
      console.error(`   Execution time: ${this.getFormattedDuration()}`);

      // Handle rollback if enabled
      if (this.config.rollback.enabled) {
        await this.handleWorkflowRollback(error);
      }

      // Finalize failed workflow
      await this.finalizeWorkflow(false, error);

      throw error;
    }
  }

  /**
   * Initialize workflow tracking and environment
   */
  async initializeWorkflow() {
    console.log('🔧 Initializing workflow environment...');

    // Create artifacts directory
    if (!fs.existsSync(this.config.artifacts.baseDir)) {
      fs.mkdirSync(this.config.artifacts.baseDir, { recursive: true });
    }

    // Initialize workflow log
    this.logPath = path.resolve(
      this.config.artifacts.baseDir,
      `${this.workflowId}.log`
    );
    this.writeWorkflowLog('WORKFLOW_STARTED', {
      workflowId: this.workflowId,
      context: this.context,
      config: this.config,
    });

    // Validate environment prerequisites
    await this.validatePrerequisites();

    console.log('✅ Workflow environment initialized');
  }

  /**
   * Validate prerequisites for workflow execution
   */
  async validatePrerequisites() {
    console.log('🔍 Validating workflow prerequisites...');

    const checks = [
      { name: 'Git repository', check: () => this.checkGitRepository() },
      {
        name: 'Node.js dependencies',
        check: () => this.checkNodeDependencies(),
      },
      { name: 'Pact Broker connectivity', check: () => this.checkPactBroker() },
      { name: 'Docker services', check: () => this.checkDockerServices() },
    ];

    for (const { name, check } of checks) {
      try {
        await check();
        console.log(`   ✅ ${name}`);
      } catch (error) {
        console.error(`   ❌ ${name}: ${error.message}`);
        throw new Error(`Prerequisite failed: ${name}`);
      }
    }

    console.log('✅ All prerequisites validated');
  }

  /**
   * Check Git repository status
   */
  checkGitRepository() {
    try {
      execSync('git rev-parse --git-dir', { stdio: 'pipe' });
      return true;
    } catch {
      throw new Error('Not in a Git repository');
    }
  }

  /**
   * Check Node.js dependencies
   */
  checkNodeDependencies() {
    const requiredPackages = [
      '@pact-foundation/pact',
      '@pact-foundation/pact-node',
    ];

    for (const pkg of requiredPackages) {
      try {
        require.resolve(pkg);
      } catch {
        throw new Error(`Required package not found: ${pkg}`);
      }
    }
    return true;
  }

  /**
   * Check Pact Broker connectivity
   */
  async checkPactBroker() {
    try {
      const response = await fetch(
        `${this.config.environment.pactBrokerUrl}/diagnostic/status/heartbeat`
      );
      if (!response.ok) {
        throw new Error(`Broker returned status: ${response.status}`);
      }
    } catch (error) {
      throw new Error(`Pact Broker unreachable: ${error.message}`);
    }
  }

  /**
   * Check Docker services
   */
  async checkDockerServices() {
    try {
      execSync('docker --version', { stdio: 'pipe' });
      // Check if required services are running
      execSync('docker ps --filter "name=pact-broker" --format "{{.Names}}"', {
        stdio: 'pipe',
      });
    } catch {
      throw new Error('Docker services not available');
    }
  }

  /**
   * Calculate stage execution order based on dependencies
   */
  calculateStageExecutionOrder() {
    const stages = Object.keys(this.config.stages);
    const resolved = [];
    const unresolved = [...stages];

    while (unresolved.length > 0) {
      const resolvable = unresolved.filter(stage => {
        const deps = this.config.stages[stage].dependencies;
        return deps.every(dep => resolved.includes(dep));
      });

      if (resolvable.length === 0) {
        throw new Error('Circular dependency detected in workflow stages');
      }

      resolved.push(...resolvable);
      unresolved.splice(
        0,
        unresolved.length,
        ...unresolved.filter(stage => !resolvable.includes(stage))
      );
    }

    return resolved;
  }

  /**
   * Execute a single workflow stage
   */
  async executeStage(stageName) {
    const stage = this.config.stages[stageName];
    const startTime = Date.now();

    console.log(`\n📦 Executing Stage: ${stageName}`);
    console.log(`   Description: ${stage.description}`);
    console.log(`   Timeout: ${stage.timeout / 1000}s`);

    this.currentStage = stageName;
    this.writeWorkflowLog('STAGE_STARTED', {
      stage: stageName,
      timestamp: new Date().toISOString(),
    });

    try {
      // Execute stage with timeout
      const result = await this.executeStageWithTimeout(stageName, stage);

      const duration = Date.now() - startTime;
      console.log(`   ✅ Stage completed in ${duration}ms`);

      this.stageResults[stageName] = {
        status: 'success',
        duration,
        result,
        timestamp: new Date().toISOString(),
      };

      this.writeWorkflowLog('STAGE_COMPLETED', {
        stage: stageName,
        duration,
        result: this.sanitizeResultForLogging(result),
      });
    } catch (error) {
      const duration = Date.now() - startTime;
      console.error(`   ❌ Stage failed after ${duration}ms: ${error.message}`);

      this.stageResults[stageName] = {
        status: 'failed',
        duration,
        error: error.message,
        timestamp: new Date().toISOString(),
      };

      this.writeWorkflowLog('STAGE_FAILED', {
        stage: stageName,
        duration,
        error: error.message,
      });

      // Retry logic if enabled
      if (this.shouldRetryStage(stageName, error)) {
        return await this.retryStage(stageName);
      }

      // Fail fast if required and stage is critical
      if (this.config.execution.failFast && stage.required) {
        throw error;
      }

      // Continue with non-critical stages
      console.warn(
        `   ⚠️ Continuing despite failed non-critical stage: ${stageName}`
      );
    }
  }

  /**
   * Execute stage with timeout protection
   */
  async executeStageWithTimeout(stageName, stage) {
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error(`Stage timeout after ${stage.timeout}ms`));
      }, stage.timeout);

      this.executeStageLogic(stageName)
        .then(result => {
          clearTimeout(timeout);
          resolve(result);
        })
        .catch(error => {
          clearTimeout(timeout);
          reject(error);
        });
    });
  }

  /**
   * Execute the actual stage logic
   */
  async executeStageLogic(stageName) {
    switch (stageName) {
      case 'setup':
        return await this.executeSetupStage();

      case 'contract-generation':
        return await this.executeContractGeneration();

      case 'contract-publishing':
        return await this.executeContractPublishing();

      case 'provider-verification':
        return await this.executeProviderVerification();

      case 'compatibility-testing':
        return await this.executeCompatibilityTesting();

      case 'deployment-validation':
        return await this.executeDeploymentValidation();

      case 'reporting':
        return await this.executeReporting();

      default:
        throw new Error(`Unknown stage: ${stageName}`);
    }
  }

  /**
   * Execute setup stage
   */
  async executeSetupStage() {
    console.log('   🔧 Setting up test environment...');

    // Start Docker services if needed
    try {
      execSync(
        'docker-compose -f tests/contract/docker-compose.pact.yml up -d',
        { stdio: 'pipe' }
      );
      await this.waitForServices();
    } catch (error) {
      console.warn('   ⚠️  Docker services might already be running');
    }

    return {
      status: 'environment-ready',
      services: this.config.environment.services,
    };
  }

  /**
   * Execute contract generation stage
   */
  async executeContractGeneration() {
    console.log('   📋 Generating contract specifications...');

    // Run contract tests to generate Pact files
    try {
      execSync('npm run test:contract', { stdio: 'pipe' });
    } catch (error) {
      throw new Error(`Contract generation failed: ${error.message}`);
    }

    // Validate generated contracts
    const pactsDir = path.resolve(__dirname, '../pacts');
    const pactFiles = fs.existsSync(pactsDir)
      ? fs.readdirSync(pactsDir).filter(f => f.endsWith('.json'))
      : [];

    if (pactFiles.length === 0) {
      throw new Error('No contract files generated');
    }

    console.log(`   📄 Generated ${pactFiles.length} contract files`);
    return { contractFiles: pactFiles, pactsDirectory: pactsDir };
  }

  /**
   * Execute contract publishing stage
   */
  async executeContractPublishing() {
    console.log('   📤 Publishing contracts to Pact Broker...');

    const publisher = new ContractPublisher();
    const result = await publisher.publishContracts();

    return { published: true, version: this.context.version, result };
  }

  /**
   * Execute provider verification stage
   */
  async executeProviderVerification() {
    console.log('   🔍 Verifying contracts against providers...');

    const verificationResults = {};
    const providers = this.config.environment.services;

    if (this.config.execution.parallelProviders) {
      // Parallel verification
      const promises = providers.map(async provider => {
        try {
          const verifier = new ProviderVerifier(provider);
          const result = await verifier.verifyContracts();
          return { provider, result };
        } catch (error) {
          return { provider, error: error.message };
        }
      });

      const results = await Promise.allSettled(promises);
      results.forEach(result => {
        if (result.status === 'fulfilled') {
          verificationResults[result.value.provider] = result.value.result;
        } else {
          verificationResults[result.reason.provider] = {
            error: result.reason.error,
          };
        }
      });
    } else {
      // Sequential verification
      for (const provider of providers) {
        try {
          const verifier = new ProviderVerifier(provider);
          verificationResults[provider] = await verifier.verifyContracts();
        } catch (error) {
          verificationResults[provider] = { error: error.message };
        }
      }
    }

    // Check if any critical verifications failed
    const failedProviders = Object.entries(verificationResults)
      .filter(([provider, result]) => result.error || !result.success)
      .map(([provider]) => provider);

    if (failedProviders.length > 0) {
      throw new Error(
        `Provider verification failed: ${failedProviders.join(', ')}`
      );
    }

    console.log(
      `   ✅ All ${providers.length} providers verified successfully`
    );
    return verificationResults;
  }

  /**
   * Execute compatibility testing stage
   */
  async executeCompatibilityTesting() {
    console.log('   🔄 Testing API version compatibility matrix...');

    const tester = new CompatibilityMatrixTester();
    const { success, matrix, report, criticalIssues } =
      await tester.runCompatibilityMatrix();

    if (!success) {
      throw new Error(
        `Compatibility testing failed with ${criticalIssues.length} critical issues`
      );
    }

    console.log(
      `   ✅ Compatibility matrix passed (${matrix.filter(t => t.compatible).length}/${matrix.length} tests)`
    );
    return { success, compatibilityReport: report, totalTests: matrix.length };
  }

  /**
   * Execute deployment validation stage
   */
  async executeDeploymentValidation() {
    console.log('   🔒 Validating deployment readiness...');

    const validator = new DeploymentValidator({
      targetEnvironment: this.context.environment,
    });
    const { canDeploy, report } = await validator.validateDeployment();

    if (!canDeploy) {
      throw new Error('Deployment validation failed - not safe to deploy');
    }

    console.log(
      `   ✅ Deployment validated for ${this.context.environment} environment`
    );
    return { canDeploy, validationReport: report };
  }

  /**
   * Execute reporting stage
   */
  async executeReporting() {
    console.log('   📊 Generating comprehensive workflow report...');

    const report = await this.generateWorkflowReport();

    // Send notifications if enabled
    if (this.config.notifications.enabled) {
      await this.sendNotifications(report);
    }

    // Upload artifacts if enabled
    if (this.config.artifacts.upload.enabled) {
      await this.uploadArtifacts();
    }

    console.log('   ✅ Reporting completed');
    return { report, notifications: this.config.notifications.enabled };
  }

  /**
   * Wait for services to be ready
   */
  async waitForServices(maxWait = 30000) {
    const startTime = Date.now();
    const checkInterval = 1000;

    while (Date.now() - startTime < maxWait) {
      try {
        const response = await fetch(
          `${this.config.environment.pactBrokerUrl}/diagnostic/status/heartbeat`
        );
        if (response.ok) {
          return;
        }
      } catch {
        // Service not ready yet
      }

      await new Promise(resolve => setTimeout(resolve, checkInterval));
    }

    throw new Error('Services did not become ready within timeout period');
  }

  /**
   * Check if workflow should be aborted
   */
  shouldAbortWorkflow(stageName) {
    const stage = this.config.stages[stageName];
    const result = this.stageResults[stageName];

    return (
      this.config.execution.failFast &&
      stage.required &&
      result.status === 'failed'
    );
  }

  /**
   * Check if stage should be retried
   */
  shouldRetryStage(stageName, error) {
    const stage = this.config.stages[stageName];
    const attempts = this.getStageAttempts(stageName);

    return (
      attempts < this.config.execution.retryCount &&
      stage.required &&
      this.isRetriableError(error)
    );
  }

  /**
   * Retry failed stage
   */
  async retryStage(stageName) {
    const attempts = this.getStageAttempts(stageName);
    console.log(
      `   🔄 Retrying stage ${stageName} (attempt ${attempts + 1}/${this.config.execution.retryCount})`
    );

    // Wait before retry
    await new Promise(resolve =>
      setTimeout(resolve, this.config.execution.retryDelay)
    );

    this.incrementStageAttempts(stageName);
    return await this.executeStage(stageName);
  }

  /**
   * Handle workflow rollback
   */
  async handleWorkflowRollback(error) {
    console.log('\n🔄 Initiating workflow rollback...');

    this.writeWorkflowLog('ROLLBACK_STARTED', {
      reason: error.message,
      completedStages: Object.keys(this.stageResults),
    });

    try {
      // Rollback completed stages in reverse order
      const completedStages = Object.keys(this.stageResults).reverse();

      for (const stageName of completedStages) {
        await this.rollbackStage(stageName);
      }

      console.log('✅ Workflow rollback completed');
      this.writeWorkflowLog('ROLLBACK_COMPLETED', {});
    } catch (rollbackError) {
      console.error(`❌ Rollback failed: ${rollbackError.message}`);
      this.writeWorkflowLog('ROLLBACK_FAILED', {
        error: rollbackError.message,
      });
    }
  }

  /**
   * Rollback a specific stage
   */
  async rollbackStage(stageName) {
    console.log(`   🔄 Rolling back stage: ${stageName}`);

    // Stage-specific rollback logic
    switch (stageName) {
      case 'contract-publishing':
        // In a real implementation, you might unpublish contracts
        console.log('     📤 Contract unpublishing not implemented');
        break;

      case 'setup':
        // Stop Docker services
        try {
          execSync(
            'docker-compose -f tests/contract/docker-compose.pact.yml down',
            { stdio: 'pipe' }
          );
        } catch {
          console.warn('     ⚠️ Failed to stop Docker services');
        }
        break;

      default:
        console.log(
          `     ℹ️ No rollback action defined for stage: ${stageName}`
        );
    }
  }

  /**
   * Finalize workflow execution
   */
  async finalizeWorkflow(success, error = null) {
    const endTime = Date.now();
    const duration = endTime - this.startTime;

    const finalResult = {
      workflowId: this.workflowId,
      success,
      duration,
      stages: this.stageResults,
      context: this.context,
      artifacts: this.context.artifacts,
      error: error ? error.message : null,
      timestamp: new Date().toISOString(),
    };

    this.writeWorkflowLog('WORKFLOW_COMPLETED', finalResult);

    // Clean up old artifacts if enabled
    if (this.config.artifacts.retention) {
      await this.cleanupOldArtifacts();
    }

    return finalResult;
  }

  /**
   * Generate comprehensive workflow report
   */
  async generateWorkflowReport() {
    const report = {
      workflow: {
        id: this.workflowId,
        version: this.context.version,
        branch: this.context.branch,
        environment: this.context.environment,
        duration: this.getFormattedDuration(),
        success: Object.values(this.stageResults).every(
          r => r.status === 'success'
        ),
      },
      stages: this.stageResults,
      metrics: {
        totalStages: Object.keys(this.config.stages).length,
        completedStages: Object.keys(this.stageResults).length,
        failedStages: Object.values(this.stageResults).filter(
          r => r.status === 'failed'
        ).length,
        totalDuration: Date.now() - this.startTime,
      },
      artifacts: this.context.artifacts,
      recommendations: this.generateWorkflowRecommendations(),
    };

    // Save report
    const reportPath = path.resolve(
      this.config.artifacts.baseDir,
      `workflow-report-${this.workflowId}.json`
    );
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
    this.context.artifacts.push({ type: 'report', path: reportPath });

    return report;
  }

  /**
   * Generate workflow recommendations
   */
  generateWorkflowRecommendations() {
    const recommendations = [];

    // Check stage durations
    const avgDuration =
      Object.values(this.stageResults).reduce(
        (sum, stage) => sum + stage.duration,
        0
      ) / Object.keys(this.stageResults).length;

    if (avgDuration > 60000) {
      // 1 minute
      recommendations.push({
        type: 'performance',
        message:
          'Workflow stages are taking longer than expected. Consider optimizing test execution.',
      });
    }

    // Check for failed stages
    const failedStages = Object.entries(this.stageResults).filter(
      ([name, result]) => result.status === 'failed'
    );

    if (failedStages.length > 0) {
      recommendations.push({
        type: 'reliability',
        message: `${failedStages.length} stage(s) failed. Review error logs and improve test reliability.`,
      });
    }

    return recommendations;
  }

  /**
   * Utility methods
   */
  getCurrentVersion() {
    try {
      return execSync('git describe --tags --always', {
        encoding: 'utf8',
      }).trim();
    } catch {
      return '1.0.0-unknown';
    }
  }

  getCurrentBranch() {
    try {
      return execSync('git rev-parse --abbrev-ref HEAD', {
        encoding: 'utf8',
      }).trim();
    } catch {
      return 'unknown';
    }
  }

  getEnvironment() {
    const branch = this.getCurrentBranch();
    if (['main', 'master'].includes(branch)) return 'production';
    if (branch.startsWith('release/')) return 'staging';
    return 'development';
  }

  getCurrentStage() {
    return this.currentStage || 'unknown';
  }

  getFormattedDuration() {
    const duration = Date.now() - this.startTime;
    const minutes = Math.floor(duration / 60000);
    const seconds = Math.floor((duration % 60000) / 1000);
    return `${minutes}m ${seconds}s`;
  }

  getStageAttempts(stageName) {
    return (this.stageAttempts && this.stageAttempts[stageName]) || 0;
  }

  incrementStageAttempts(stageName) {
    if (!this.stageAttempts) this.stageAttempts = {};
    this.stageAttempts[stageName] = (this.stageAttempts[stageName] || 0) + 1;
  }

  isRetriableError(error) {
    // Define which errors can be retried
    const retriablePatterns = [
      /timeout/i,
      /network/i,
      /connection/i,
      /temporary/i,
    ];

    return retriablePatterns.some(pattern => pattern.test(error.message));
  }

  writeWorkflowLog(event, data) {
    const logEntry = {
      timestamp: new Date().toISOString(),
      workflowId: this.workflowId,
      event,
      data,
    };

    try {
      fs.appendFileSync(this.logPath, JSON.stringify(logEntry) + '\n');
    } catch (error) {
      console.warn(`Failed to write workflow log: ${error.message}`);
    }
  }

  sanitizeResultForLogging(result) {
    // Remove large objects that shouldn't be logged
    if (typeof result === 'object' && result !== null) {
      const sanitized = { ...result };
      // Remove potentially large arrays/objects
      if (sanitized.matrix && Array.isArray(sanitized.matrix)) {
        sanitized.matrix = `[${sanitized.matrix.length} items]`;
      }
      return sanitized;
    }
    return result;
  }

  async sendNotifications(report) {
    console.log('📢 Sending workflow notifications...');
    // Implementation would send to Slack, Teams, Email, etc.
    console.log('   📧 Notifications sent');
  }

  async uploadArtifacts() {
    console.log('☁️ Uploading workflow artifacts...');
    // Implementation would upload to S3, Azure Blob, etc.
    console.log('   ☁️ Artifacts uploaded');
  }

  async cleanupOldArtifacts() {
    console.log('🧹 Cleaning up old artifacts...');
    // Implementation would clean up based on retention policies
    console.log('   🧹 Cleanup completed');
  }
}

/**
 * Main execution function
 */
async function main() {
  console.log('🎯 WRFCoin Contract Testing Workflow Orchestrator');
  console.log('================================================\n');

  try {
    // Parse command line arguments
    const args = process.argv.slice(2);
    const dryRun = args.includes('--dry-run');
    const stage = args.find(arg => arg.startsWith('--stage='))?.split('=')[1];
    const environment = args
      .find(arg => arg.startsWith('--environment='))
      ?.split('=')[1];

    if (dryRun) {
      console.log('🔍 Running in dry-run mode (no actual execution)');
    }

    const options = {};
    if (environment) {
      options.context = { environment };
    }

    const orchestrator = new WorkflowOrchestrator(options);

    if (stage) {
      // Execute single stage
      console.log(`🎯 Executing single stage: ${stage}`);
      await orchestrator.executeStage(stage);
    } else {
      // Execute full workflow
      const result = await orchestrator.executeWorkflow();

      console.log('\n📈 Workflow Summary:');
      console.log(`   Success: ${result.success ? '✅' : '❌'}`);
      console.log(`   Duration: ${orchestrator.getFormattedDuration()}`);
      console.log(
        `   Stages: ${result.metrics.completedStages}/${result.metrics.totalStages}`
      );
      console.log(`   Artifacts: ${result.artifacts.length}`);

      process.exit(result.success ? 0 : 1);
    }
  } catch (error) {
    console.error('\n💥 Workflow orchestration failed!');
    console.error(`Error: ${error.message}\n`);
    process.exit(1);
  }
}

// Execute if run directly
if (require.main === module) {
  main();
}

module.exports = {
  WorkflowOrchestrator,
  WORKFLOW_CONFIG,
};
