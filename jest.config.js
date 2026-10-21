/**
 * Jest configuration for Wrfcoin Project
 */
export default {
  // Test environment
  testEnvironment: 'node',

  // Test file patterns - consolidated to tests/ directory only
  testMatch: [
    '**/tests/**/*.(test|spec).(js|jsx|ts|tsx)',
    '**/sdk/js/__tests__/**/*.test.js',
    '**/frontend/src/__tests__/**/*.test.js',
    '**/__tests__/**/*.(js|jsx|ts|tsx)',
  ],

  // Coverage configuration
  collectCoverage: true,
  coverageDirectory: 'coverage',
  coverageReporters: ['text', 'lcov', 'html', 'json-summary'],
  coverageThreshold: {
    global: {
      branches: 80,
      functions: 80,
      lines: 80,
      statements: 80,
    },
  },
  collectCoverageFrom: [
    'backend/**/*.{js,ts}',
    'frontend/src/**/*.{js,jsx,ts,tsx}',
    'scripts/**/*.{js,ts}',
    'sdk/js/**/*.{js,ts}',
    'shared/**/*.{js,ts}',
    '!**/node_modules/**',
    '!**/coverage/**',
    '!**/*.config.js',
    '!**/*.min.js',
    '!**/vendor/**',
    '!**/examples/**',
    '!**/tests/**',
    '!**/__tests__/**',
    '!**/*.d.ts',
  ],

  // Module file extensions
  moduleFileExtensions: ['js', 'jsx', 'ts', 'tsx', 'json', 'node'],

  // Transform files
  preset: 'ts-jest',
  transform: {
    '^.+\\.tsx?$': 'ts-jest',
    '^.+\\.jsx?$': 'babel-jest',
  },
  transformIgnorePatterns: ['node_modules/(?!(uuid|axios|@noble/curves|@noble/hashes|other-esm-modules)/)'],

  // Module name mapping for path aliases
  moduleNameMapper: {
    // Strip .js from relative imports for ESM/CJS interop (ts-jest CJS mode)
    '^(\\.{1,2}/.*)\\.js$': '$1',
    // Replace Vite-only frontend config with a Jest-compatible stub so that
    // import.meta.env / window references never reach the TypeScript compiler.
    // Pattern matches the raw import string (relative or absolute-like) ending
    // in /services/config with or without the .ts extension.
    '.*/services/config(\\.ts)?$': '<rootDir>/tests/mocks/frontend-config.mock.js',
    '^react$': '<rootDir>/node_modules/react',
    '^react-dom$': '<rootDir>/node_modules/react-dom',
    '^react/jsx-runtime$': '<rootDir>/node_modules/react/jsx-runtime.js',
    '^@/(.*)$': '<rootDir>/backend/$1',
    '^@frontend/(.*)$': '<rootDir>/frontend/src/$1',
    '^@sdk/(.*)$': '<rootDir>/sdk/$1',
    '^@/shared/(.*)$': '<rootDir>/shared/$1',
    '^@/types/(.*)$': '<rootDir>/backend/shared/types/$1',
    // Handle @noble/curves subpath exports - use nested @noble/hashes from within @noble/curves
    '^@noble/curves/bls12-381$': '<rootDir>/node_modules/@noble/curves/bls12-381.js',
    '^@noble/curves/abstract/(.*)$': '<rootDir>/node_modules/@noble/curves/abstract/$1.js',
    '^@noble/curves/(.*)$': '<rootDir>/node_modules/@noble/curves/$1.js',
    // Handle @noble/hashes imports with .js extension (used by noble curves source)
    '^@noble/hashes/sha2\\.js$': '<rootDir>/node_modules/@noble/curves/node_modules/@noble/hashes/sha2.js',
    '^@noble/hashes/utils\\.js$': '<rootDir>/node_modules/@noble/curves/node_modules/@noble/hashes/utils.js',
    '^@noble/hashes/hmac\\.js$': '<rootDir>/node_modules/@noble/curves/node_modules/@noble/hashes/hmac.js',
    '^@noble/hashes/sha3\\.js$': '<rootDir>/node_modules/@noble/curves/node_modules/@noble/hashes/sha3.js',
    '^@noble/hashes/blake2\\.js$': '<rootDir>/node_modules/@noble/curves/node_modules/@noble/hashes/blake2.js',
    // Handle @noble/hashes imports without .js extension
    '^@noble/hashes/sha2$': '<rootDir>/node_modules/@noble/curves/node_modules/@noble/hashes/sha2.js',
    '^@noble/hashes/utils$': '<rootDir>/node_modules/@noble/curves/node_modules/@noble/hashes/utils.js',
    '^@noble/hashes/hmac$': '<rootDir>/node_modules/@noble/curves/node_modules/@noble/hashes/hmac.js',
    '^@noble/hashes/sha3$': '<rootDir>/node_modules/@noble/curves/node_modules/@noble/hashes/sha3.js',
    '^@noble/hashes/blake2$': '<rootDir>/node_modules/@noble/curves/node_modules/@noble/hashes/blake2.js',
    // Catch-all for other hashes subpaths (with and without .js)
    '^@noble/hashes/(.*)\\.js$': '<rootDir>/node_modules/@noble/curves/node_modules/@noble/hashes/$1.js',
    '^@noble/hashes/(.*)$': '<rootDir>/node_modules/@noble/curves/node_modules/@noble/hashes/$1.js',
  },

  // Ignore patterns
  testPathIgnorePatterns: [
    '/node_modules/',
    '/blockchain-core/target/',
    '/dist/',
    '/build/',
    '/zen-mcp-server/',
  ],

  // Setup files
  setupFilesAfterEnv: ['<rootDir>/tests/jest.setup.js'],

  // Verbose output
  verbose: true,

  // Clear mocks between tests
  clearMocks: true,

  // Timeout for tests (30 seconds)
  testTimeout: 30000,

  // TypeScript configuration for ts-jest
  globals: {
    'ts-jest': {
      tsconfig: { jsx: "react",
        target: 'ES2020',
        module: 'CommonJS',
        moduleResolution: 'node',
        esModuleInterop: true,
        allowSyntheticDefaultImports: true,
        skipLibCheck: true,
        types: ['jest', 'node'],
        // Frontend component tests require jsx and dom support
        jsx: 'react-jsx',
        lib: ['ES2020', 'DOM', 'DOM.Iterable'],
      },
      // Ignore import.meta (TS1343) and related Vite-env errors (TS2339,
      // TS2304) — only triggered when the full services/config compile chain
      // is reached; real resolution is via moduleNameMapper below.
      diagnostics: { ignoreCodes: [1343, 2339, 2304] },
    },
  },
};
