/**
 * Enhanced Jest Configuration for CI/CD Gatekeeping - WRFCoin Platform
 * Optimized for parallel execution, comprehensive coverage, and quality gates
 */

const path = require('path');
const os = require('os');

// CI environment detection
const isCI = process.env.CI === 'true';
const isCoverage = process.env.COVERAGE === 'true';
const nodeEnv = process.env.NODE_ENV || 'test';

// Performance optimization settings
const cpuCount = os.cpus().length;
const maxWorkers = isCI
  ? Math.min(cpuCount, 4)
  : Math.max(1, Math.floor(cpuCount * 0.75));

module.exports = {
  // Use project root as base
  rootDir: path.join(__dirname, '..'),

  // Enhanced parallel execution configuration
  maxWorkers: maxWorkers,
  workerIdleMemoryLimit: '512MB',

  // Test environment optimizations
  testEnvironment: 'node',

  // Optimized test discovery patterns
  testMatch: [
    '<rootDir>/tests/unit/**/*.test.{js,jsx,ts,tsx}',
    '<rootDir>/tests/integration/**/*.test.{js,jsx,ts,tsx}',
    '<rootDir>/tests/security/**/*.test.{js,jsx,ts,tsx}',
    '<rootDir>/backend/**/__tests__/**/*.test.{js,ts}',
    '<rootDir>/frontend/src/**/__tests__/**/*.test.{js,jsx,ts,tsx}',
  ],

  // Enhanced project configuration for parallel execution
  projects: [
    // Unit tests - fastest, run first
    {
      displayName: 'unit',
      testMatch: ['<rootDir>/tests/unit/**/*.test.{js,ts}'],
      testEnvironment: 'node',
      setupFilesAfterEnv: ['<rootDir>/tests/utils/test-database.js'],
      maxWorkers: maxWorkers,
      runner: 'jest-runner',
      testTimeout: 15000,
    },

    // Frontend tests - parallel execution
    {
      displayName: 'frontend',
      testMatch: ['<rootDir>/frontend/**/*.test.{js,jsx,ts,tsx}'],
      testEnvironment: 'jsdom',
      setupFilesAfterEnv: ['<rootDir>/tests/utils/global-setup.js'],
      maxWorkers: Math.max(1, Math.floor(maxWorkers / 2)),
      runner: 'jest-runner',
      testTimeout: 20000,
    },

    // Integration tests - controlled parallelism
    {
      displayName: 'integration',
      testMatch: ['<rootDir>/tests/integration/**/*.test.{js,ts}'],
      testEnvironment: 'node',
      globalSetup: '<rootDir>/tests/integration/backend/globalSetup.js',
      globalTeardown: '<rootDir>/tests/integration/backend/globalTeardown.js',
      maxWorkers: Math.min(2, maxWorkers),
      runner: 'jest-runner',
      testTimeout: 90000,
      setupFilesAfterEnv: ['<rootDir>/tests/utils/integration-helpers.js'],
    },

    // Security tests - sequential execution for stability
    {
      displayName: 'security',
      testMatch: ['<rootDir>/tests/security/**/*.test.{js,ts}'],
      testEnvironment: 'node',
      setupFilesAfterEnv: ['<rootDir>/tests/security/test-runner.js'],
      maxWorkers: 1,
      runner: 'jest-runner',
      testTimeout: 180000,
      verbose: true,
    },

    // Smart contract tests - optimized for Hardhat integration
    {
      displayName: 'smart-contracts',
      testMatch: ['<rootDir>/tests/unit/smart-contracts/**/*.test.{js,ts}'],
      testEnvironment: 'node',
      maxWorkers: Math.max(1, Math.floor(maxWorkers / 3)),
      runner: 'jest-runner',
      testTimeout: 120000,
      setupFilesAfterEnv: ['<rootDir>/tests/utils/hardhat-helpers.js'],
    },
  ],

  // Enhanced coverage configuration with strict thresholds
  collectCoverage: isCoverage,
  coverageDirectory: '<rootDir>/tests/results/coverage',
  coverageReporters: [
    'text',
    'text-summary',
    'lcov',
    'html',
    'json-summary',
    'cobertura', // For CI/CD integration
    'clover', // For additional CI tools
  ],

  collectCoverageFrom: [
    // Frontend coverage with stricter patterns
    'frontend/src/**/*.{js,jsx,ts,tsx}',
    '!frontend/src/**/*.d.ts',
    '!frontend/src/**/index.{js,ts}',
    '!frontend/src/**/*.stories.{js,jsx,ts,tsx}',
    '!frontend/src/**/*.test.{js,jsx,ts,tsx}',
    '!frontend/src/test-utils/**',

    // Backend coverage
    'backend/**/*.{js,ts}',
    '!backend/**/*.d.ts',
    '!backend/**/migrations/**',
    '!backend/**/seeds/**',
    '!backend/**/__tests__/**',
    '!backend/**/test/**',

    // Scripts and utilities
    'scripts/**/*.{js,ts}',
    '!scripts/test-*.js',
    '!scripts/**/node_modules/**',

    // SDK coverage
    'sdk/**/*.{js,ts}',
    '!sdk/**/*.d.ts',
    '!sdk/**/dist/**',
    '!sdk/**/build/**',
    '!sdk/**/__tests__/**',

    // Exclusions
    '!**/node_modules/**',
    '!**/vendor/**',
    '!**/test/**',
    '!**/tests/**',
    '!**/__tests__/**',
    '!**/coverage/**',
    '!**/*.config.js',
    '!**/*.min.js',
    '!blockchain-core/**', // Rust code tested separately
  ],

  // Strict coverage thresholds for quality gates
  coverageThreshold: {
    global: {
      branches: 80,
      functions: 80,
      lines: 80,
      statements: 80,
    },
    // Backend services require higher coverage
    './backend/': {
      branches: 85,
      functions: 85,
      lines: 85,
      statements: 85,
    },
    // Critical API routes require highest coverage
    './backend/api/': {
      branches: 90,
      functions: 90,
      lines: 90,
      statements: 90,
    },
    // GraphQL resolvers
    './backend/graphql/': {
      branches: 85,
      functions: 85,
      lines: 85,
      statements: 85,
    },
    // Frontend components
    './frontend/src/components/': {
      branches: 75,
      functions: 75,
      lines: 75,
      statements: 75,
    },
    // Frontend pages
    './frontend/src/pages/': {
      branches: 70,
      functions: 70,
      lines: 70,
      statements: 70,
    },
    // SDK modules
    './sdk/': {
      branches: 85,
      functions: 85,
      lines: 85,
      statements: 85,
    },
  },

  // Enhanced setup and teardown
  setupFilesAfterEnv: [
    '<rootDir>/tests/utils/global-setup.js',
    '<rootDir>/tests/utils/test-helpers.js',
    '<rootDir>/tests/utils/coverage-helpers.js',
  ],

  // Module resolution optimizations
  moduleNameMapper: {
    // Frontend aliases
    '^@/(.*)$': '<rootDir>/frontend/src/$1',
    '^@components/(.*)$': '<rootDir>/frontend/src/components/$1',
    '^@hooks/(.*)$': '<rootDir>/frontend/src/hooks/$1',
    '^@utils/(.*)$': '<rootDir>/frontend/src/utils/$1',
    '^@pages/(.*)$': '<rootDir>/frontend/src/pages/$1',
    '^@services/(.*)$': '<rootDir>/frontend/src/services/$1',

    // Backend aliases
    '^@backend/(.*)$': '<rootDir>/backend/$1',
    '^@api/(.*)$': '<rootDir>/backend/api/$1',
    '^@graphql/(.*)$': '<rootDir>/backend/graphql/$1',
    '^@models/(.*)$': '<rootDir>/backend/models/$1',
    '^@middleware/(.*)$': '<rootDir>/backend/middleware/$1',
    '^@shared/(.*)$': '<rootDir>/backend/shared/$1',
    '^@db/(.*)$': '<rootDir>/backend/database/$1',

    // Test utilities
    '^@tests/(.*)$': '<rootDir>/tests/$1',
    '^@testUtils/(.*)$': '<rootDir>/tests/utils/$1',
    '^@mocks/(.*)$': '<rootDir>/tests/mocks/$1',

    // Asset mocks
    '\\.(css|less|scss|sass)$': '<rootDir>/tests/mocks/styleMock.js',
    '\\.(jpg|jpeg|png|gif|svg)$': '<rootDir>/tests/mocks/fileMock.js',
  },

  // Transform optimizations
  preset: 'ts-jest',
  transform: {
    '^.+\\.tsx?$': [
      'ts-jest',
      {
        tsconfig: '<rootDir>/tsconfig.json',
        isolatedModules: true, // Performance optimization
      },
    ],
    '^.+\\.jsx?$': [
      'babel-jest',
      {
        presets: [
          ['@babel/preset-env', { targets: { node: 'current' } }],
          '@babel/preset-react',
        ],
      },
    ],
  },

  transformIgnorePatterns: [
    'node_modules/(?!(uuid|axios|@opentelemetry|@apollo|graphql-ws|ws)/)',
  ],

  // Performance optimizations
  cacheDirectory: '<rootDir>/tests/.jest-cache',
  clearMocks: true,
  resetMocks: true,
  restoreMocks: true,

  // Test result aggregation and reporting
  reporters: [
    'default',
    [
      'jest-html-reporter',
      {
        pageTitle: 'WRFCoin CI/CD Test Report',
        outputPath: '<rootDir>/tests/results/jest-report.html',
        includeFailureMsg: true,
        includeConsoleLog: true,
        includeSuiteFailure: true,
        theme: 'lightTheme',
      },
    ],
    [
      'jest-junit',
      {
        outputDirectory: '<rootDir>/tests/results',
        outputName: 'junit.xml',
        classNameTemplate: '{classname}',
        titleTemplate: '{title}',
        ancestorSeparator: ' › ',
        usePathForSuiteName: true,
      },
    ],
    // Coverage diff reporter for PRs
    isCoverage && [
      'jest-coverage-badges',
      {
        outputDir: '<rootDir>/tests/results/coverage/badges',
        generateSummary: true,
      },
    ],
    // Performance metrics reporter
    [
      '<rootDir>/tests/utils/performance-reporter.js',
      {
        outputPath: '<rootDir>/tests/results/performance-metrics.json',
        thresholds: {
          maxTestTime: 5000,
          maxSuiteTime: 60000,
        },
      },
    ],
  ].filter(Boolean),

  // Test environment variables
  testEnvironmentOptions: {
    node: {
      // Node.js specific options
    },
    jsdom: {
      // JSDOM specific options for frontend tests
      url: 'http://localhost:3000',
    },
  },

  // Global test timeout with CI adjustments
  testTimeout: isCI ? 60000 : 30000,

  // Enhanced watch mode for development
  watchPlugins: [
    'jest-watch-typeahead/filename',
    'jest-watch-typeahead/testname',
    'jest-watch-select-projects',
  ],

  // Test path ignore patterns
  testPathIgnorePatterns: [
    '/node_modules/',
    '/blockchain-core/target/',
    '/dist/',
    '/build/',
    '/coverage/',
    '/.next/',
    '/tests/archive/',
    '/tests/results/',
    '/tests/fuzz/', // Fuzz tests run separately
  ],

  // Module file extensions
  moduleFileExtensions: ['js', 'jsx', 'ts', 'tsx', 'json', 'node'],

  // Verbose output control
  verbose: isCI || process.env.VERBOSE === 'true',

  // Fail fast in CI
  bail: isCI ? 1 : 0,

  // Force exit in CI to prevent hanging
  forceExit: isCI,

  // Detect open handles in CI
  detectOpenHandles: isCI,

  // Error handling
  errorOnDeprecated: true,

  // Test result processor for additional analysis
  testResultsProcessor: '<rootDir>/tests/utils/test-results-processor.js',

  // Global variables
  globals: {
    'ts-jest': {
      tsconfig: '<rootDir>/tsconfig.json',
      isolatedModules: true,
    },
    __TEST_ENV__: nodeEnv,
    __CI__: isCI,
    __COVERAGE__: isCoverage,
  },
};
