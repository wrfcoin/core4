/**
 * Testing-focused Jest configuration
 * Optimized for TypeScript and dependency management
 */
module.exports = {
  // Test environment
  testEnvironment: 'node',

  // TypeScript support with ts-jest
  preset: 'ts-jest',

  // Test file patterns
  testMatch: [
    '**/__tests__/**/*.(js|jsx|ts|tsx)',
    '**/*.(test|spec).(js|jsx|ts|tsx)',
    '**/consolidated-tests/**/*.test.{js,jsx,ts,tsx}',
  ],

  // Transform configuration
  transform: {
    '^.+\\.tsx?$': [
      'ts-jest',
      {
        useESM: false,
        tsconfig: {
          compilerOptions: {
            module: 'commonjs',
            target: 'es2018',
            moduleResolution: 'node',
            allowSyntheticDefaultImports: true,
            esModuleInterop: true,
            skipLibCheck: true,
          },
        },
      },
    ],
    '^.+\\.(js|jsx)$': [
      'babel-jest',
      {
        presets: [
          ['@babel/preset-env', { targets: { node: 'current' } }],
          ['@babel/preset-react', { runtime: 'automatic' }],
        ],
      },
    ],
  },

  // Module file extensions
  moduleFileExtensions: ['ts', 'tsx', 'js', 'jsx', 'json', 'node'],

  // Module name mapping for path aliases
  moduleNameMapper: {
    '^@/(.*)$': '<rootDir>/backend/$1',
    '^@frontend/(.*)$': '<rootDir>/frontend/src/$1',
    '^@sdk/(.*)$': '<rootDir>/sdk/$1',
    '^@/shared/(.*)$': '<rootDir>/shared/$1',
    '^@/types/(.*)$': '<rootDir>/backend/shared/types/$1',
  },

  // Ignore patterns
  testPathIgnorePatterns: [
    '/node_modules/',
    '/blockchain-core/target/',
    '/dist/',
    '/build/',
    '/zen-mcp-server/',
    'consolidated-tests/e2e',
    'playwright',
  ],

  // Transform ignore patterns
  transformIgnorePatterns: ['node_modules/(?!(uuid|axios|@testing-library)/)'],

  // Setup files
  setupFilesAfterEnv: ['<rootDir>/jest.setup.js'],

  // Coverage configuration (reduced for faster testing)
  collectCoverage: false,

  // Clear mocks between tests
  clearMocks: true,

  // Timeout for tests
  testTimeout: 10000,

  // Verbose output
  verbose: false,

  // Fail fast on first error
  bail: 1,

  // Maximum worker processes
  maxWorkers: '50%',
};
