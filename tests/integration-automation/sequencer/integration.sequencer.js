/**
 * Integration Test Sequencer
 * Controls the order and dependencies of integration test execution
 */

const Sequencer = require('@jest/test-sequencer').default;
const fs = require('fs');
const path = require('path');

class IntegrationTestSequencer extends Sequencer {
  /**
   * Define test execution priorities and dependencies
   */
  constructor() {
    super();

    // Test execution priorities (lower number = higher priority)
    this.testPriorities = {
      // Setup and infrastructure tests first
      setup: 1,
      health: 2,
      mock: 3,

      // Basic service interactions
      'service-interaction': 10,
      database: 11,
      redis: 12,
      blockchain: 13,

      // API integration tests
      'api-integration': 20,
      user: 21,
      weather: 22,
      carbon: 23,

      // Complex workflows
      workflow: 30,
      'cross-service': 31,
      'end-to-end': 32,

      // Performance and load tests last
      performance: 40,
      load: 41,

      // Cleanup tests
      cleanup: 50,
    };

    // Test dependencies (test A must run before test B)
    this.testDependencies = {
      'api-integration': ['service-interaction', 'mock'],
      'end-to-end': ['api-integration', 'service-interaction'],
      workflow: ['api-integration'],
      'cross-service': ['service-interaction', 'api-integration'],
      performance: ['api-integration', 'end-to-end'],
      load: ['performance'],
    };

    // Test execution context tracking
    this.executionContext = {
      startTime: Date.now(),
      completedTests: new Set(),
      failedTests: new Set(),
      skippedTests: new Set(),
    };
  }

  /**
   * Sort tests based on priority, dependencies, and performance characteristics
   */
  sort(tests) {
    console.log(`🔄 Sequencing ${tests.length} integration tests...`);

    // Analyze test characteristics
    const testAnalysis = this.analyzeTests(tests);

    // Group tests by type and priority
    const groupedTests = this.groupTestsByType(tests);

    // Sort within each group and resolve dependencies
    const sequencedTests = this.sequenceTests(groupedTests, testAnalysis);

    console.log(`✅ Test sequencing complete. Execution order:`);
    sequencedTests.forEach((test, index) => {
      const testInfo = this.getTestInfo(test);
      console.log(
        `  ${index + 1}. ${testInfo.name} (${testInfo.type}, priority: ${testInfo.priority})`
      );
    });

    return sequencedTests;
  }

  /**
   * Analyze test characteristics for optimal sequencing
   */
  analyzeTests(tests) {
    const analysis = {
      totalTests: tests.length,
      testsByType: {},
      estimatedDuration: 0,
      dependencies: new Map(),
      isolationRequirements: new Set(),
      resourceRequirements: new Map(),
    };

    tests.forEach(test => {
      const testInfo = this.getTestInfo(test);

      // Group by type
      if (!analysis.testsByType[testInfo.type]) {
        analysis.testsByType[testInfo.type] = [];
      }
      analysis.testsByType[testInfo.type].push(test);

      // Estimate duration based on test type and complexity
      analysis.estimatedDuration += this.estimateTestDuration(testInfo);

      // Track dependencies
      const dependencies = this.getTestDependencies(testInfo.type);
      if (dependencies.length > 0) {
        analysis.dependencies.set(test.path, dependencies);
      }

      // Track resource requirements
      const resources = this.getTestResourceRequirements(testInfo);
      analysis.resourceRequirements.set(test.path, resources);
    });

    console.log(`📊 Test Analysis Complete:`);
    console.log(`  Total Tests: ${analysis.totalTests}`);
    console.log(
      `  Estimated Duration: ${Math.round(analysis.estimatedDuration / 1000)}s`
    );
    console.log(
      `  Test Types: ${Object.keys(analysis.testsByType).join(', ')}`
    );
    console.log(
      `  Dependencies: ${analysis.dependencies.size} tests have dependencies`
    );

    return analysis;
  }

  /**
   * Group tests by type for organized execution
   */
  groupTestsByType(tests) {
    const groups = {
      setup: [],
      infrastructure: [],
      unit: [],
      integration: [],
      workflow: [],
      performance: [],
      cleanup: [],
    };

    tests.forEach(test => {
      const testInfo = this.getTestInfo(test);
      const group = this.determineTestGroup(testInfo);

      if (groups[group]) {
        groups[group].push(test);
      } else {
        groups.integration.push(test); // Default group
      }
    });

    return groups;
  }

  /**
   * Sequence tests within groups and resolve dependencies
   */
  sequenceTests(groupedTests, analysis) {
    const sequenced = [];

    // Define group execution order
    const groupOrder = [
      'setup',
      'infrastructure',
      'unit',
      'integration',
      'workflow',
      'performance',
      'cleanup',
    ];

    groupOrder.forEach(groupName => {
      if (groupedTests[groupName] && groupedTests[groupName].length > 0) {
        console.log(
          `🔄 Sequencing ${groupName} tests (${groupedTests[groupName].length} tests)...`
        );

        // Sort tests within group
        const sortedGroup = this.sortTestsWithinGroup(
          groupedTests[groupName],
          analysis
        );
        sequenced.push(...sortedGroup);
      }
    });

    return sequenced;
  }

  /**
   * Sort tests within a specific group
   */
  sortTestsWithinGroup(tests, analysis) {
    return tests.sort((testA, testB) => {
      const infoA = this.getTestInfo(testA);
      const infoB = this.getTestInfo(testB);

      // Priority-based sorting
      const priorityA = this.getTestPriority(infoA.type);
      const priorityB = this.getTestPriority(infoB.type);

      if (priorityA !== priorityB) {
        return priorityA - priorityB;
      }

      // Dependency-based sorting
      const depsA = this.getTestDependencies(infoA.type);
      const depsB = this.getTestDependencies(infoB.type);

      // If A depends on B's type, B should run first
      if (depsA.includes(infoB.type)) {
        return 1;
      }
      if (depsB.includes(infoA.type)) {
        return -1;
      }

      // Resource-based sorting (tests requiring fewer resources first)
      const resourcesA = this.getTestResourceRequirements(infoA);
      const resourcesB = this.getTestResourceRequirements(infoB);

      if (resourcesA.parallel !== resourcesB.parallel) {
        return resourcesA.parallel ? 1 : -1; // Serial tests first
      }

      // Duration-based sorting (shorter tests first within same priority)
      const durationA = this.estimateTestDuration(infoA);
      const durationB = this.estimateTestDuration(infoB);

      if (Math.abs(durationA - durationB) > 10000) {
        // 10s difference threshold
        return durationA - durationB;
      }

      // Alphabetical sorting as final tiebreaker
      return infoA.name.localeCompare(infoB.name);
    });
  }

  /**
   * Extract test information from test path
   */
  getTestInfo(test) {
    const relativePath = path.relative(process.cwd(), test.path);
    const fileName = path.basename(test.path, path.extname(test.path));

    // Determine test type from path and filename
    let type = 'integration'; // default

    if (relativePath.includes('setup')) type = 'setup';
    else if (relativePath.includes('health')) type = 'health';
    else if (relativePath.includes('mock')) type = 'mock';
    else if (fileName.includes('service-interaction'))
      type = 'service-interaction';
    else if (fileName.includes('api-integration')) type = 'api-integration';
    else if (fileName.includes('end-to-end')) type = 'end-to-end';
    else if (fileName.includes('workflow')) type = 'workflow';
    else if (fileName.includes('cross-service')) type = 'cross-service';
    else if (fileName.includes('performance')) type = 'performance';
    else if (fileName.includes('load')) type = 'load';
    else if (fileName.includes('cleanup')) type = 'cleanup';

    return {
      name: fileName,
      type,
      path: test.path,
      relativePath,
      priority: this.getTestPriority(type),
      context: test.context || {},
    };
  }

  /**
   * Get priority for test type
   */
  getTestPriority(testType) {
    return this.testPriorities[testType] || 25; // Default middle priority
  }

  /**
   * Get dependencies for test type
   */
  getTestDependencies(testType) {
    return this.testDependencies[testType] || [];
  }

  /**
   * Determine which execution group a test belongs to
   */
  determineTestGroup(testInfo) {
    if (testInfo.type === 'setup' || testInfo.type === 'health') return 'setup';
    if (testInfo.type === 'mock') return 'infrastructure';
    if (
      ['service-interaction', 'database', 'redis', 'blockchain'].includes(
        testInfo.type
      )
    )
      return 'integration';
    if (['workflow', 'cross-service', 'end-to-end'].includes(testInfo.type))
      return 'workflow';
    if (['performance', 'load'].includes(testInfo.type)) return 'performance';
    if (testInfo.type === 'cleanup') return 'cleanup';

    return 'integration'; // Default group
  }

  /**
   * Estimate test duration based on type and characteristics
   */
  estimateTestDuration(testInfo) {
    const baseDurations = {
      setup: 5000,
      health: 2000,
      mock: 3000,
      'service-interaction': 15000,
      'api-integration': 30000,
      'end-to-end': 60000,
      workflow: 45000,
      'cross-service': 40000,
      performance: 90000,
      load: 120000,
      cleanup: 5000,
    };

    const baseDuration = baseDurations[testInfo.type] || 20000;

    // Adjust based on file size (larger files typically have more tests)
    try {
      const stats = fs.statSync(testInfo.path);
      const sizeMultiplier = Math.min(2.0, 1.0 + stats.size / 50000); // Up to 2x for large files
      return Math.round(baseDuration * sizeMultiplier);
    } catch (error) {
      return baseDuration;
    }
  }

  /**
   * Get resource requirements for test
   */
  getTestResourceRequirements(testInfo) {
    const requirements = {
      parallel: true, // Can run in parallel by default
      memory: 'medium',
      network: true,
      database: false,
      docker: false,
    };

    // Adjust based on test type
    switch (testInfo.type) {
      case 'setup':
      case 'cleanup':
        requirements.parallel = false; // Must run serially
        requirements.docker = true;
        break;

      case 'service-interaction':
      case 'api-integration':
        requirements.database = true;
        requirements.network = true;
        break;

      case 'end-to-end':
      case 'workflow':
        requirements.parallel = false; // Complex workflows run serially
        requirements.memory = 'high';
        requirements.database = true;
        requirements.docker = true;
        break;

      case 'performance':
      case 'load':
        requirements.parallel = false; // Performance tests need dedicated resources
        requirements.memory = 'high';
        break;
    }

    return requirements;
  }

  /**
   * Check if test can be skipped based on previous failures
   */
  shouldSkipTest(test, analysis) {
    const testInfo = this.getTestInfo(test);
    const dependencies = this.getTestDependencies(testInfo.type);

    // Skip if dependencies failed
    for (const dep of dependencies) {
      const depTests = Object.keys(analysis.testsByType).filter(
        type => type === dep
      );
      if (
        depTests.some(depType => this.executionContext.failedTests.has(depType))
      ) {
        console.log(
          `⏭️  Skipping ${testInfo.name} due to failed dependency: ${dep}`
        );
        return true;
      }
    }

    return false;
  }

  /**
   * Log sequencing decisions for debugging
   */
  logSequencingDecisions(tests, sequencedTests) {
    const originalOrder = tests.map(test => this.getTestInfo(test).name);
    const newOrder = sequencedTests.map(test => this.getTestInfo(test).name);

    console.log(`\n📋 Test Execution Sequence:`);
    console.log(`Original order: ${originalOrder.join(' → ')}`);
    console.log(`Optimized order: ${newOrder.join(' → ')}`);

    // Highlight major changes
    const changes = [];
    originalOrder.forEach((testName, index) => {
      const newIndex = newOrder.indexOf(testName);
      if (Math.abs(newIndex - index) > 1) {
        changes.push(`${testName}: ${index + 1} → ${newIndex + 1}`);
      }
    });

    if (changes.length > 0) {
      console.log(`📊 Major sequence changes: ${changes.join(', ')}`);
    } else {
      console.log(`✅ Test sequence optimized with minimal changes`);
    }
  }
}

module.exports = IntegrationTestSequencer;
