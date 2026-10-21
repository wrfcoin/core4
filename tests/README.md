# WRFCoin Platform - Unified Test Suite

This directory contains the consolidated test infrastructure for the WRFCoin platform, organized
into a clean, standardized structure that replaces the previous scattered test directories.

## Directory Structure

```
tests/
├── unit/                    # Unit tests
│   ├── smart-contracts/     # Smart contract unit tests
│   ├── templates/           # Test templates
│   ├── api/                 # API unit tests
│   ├── middleware/          # Middleware unit tests
│   ├── rust/                # Rust unit tests
│   └── javascript/          # JavaScript unit tests
├── integration/             # Integration tests
│   ├── backend/             # Backend integration tests
│   ├── blockchain-api/      # Blockchain API integration
│   ├── cross-chain/         # Cross-chain bridge tests
│   └── smart-contracts/     # Smart contract integration
├── e2e/                     # End-to-end tests
│   ├── workflows/           # User workflow tests
│   ├── user-journeys/       # Complete user journey tests
│   ├── dashboard/           # Dashboard E2E tests
│   ├── governance/          # Governance workflow tests
│   ├── security/            # Security E2E tests
│   └── utils/               # E2E test utilities
├── security/                # Security testing
│   ├── penetration/         # Penetration tests
│   ├── vulnerability/       # Vulnerability scans
│   └── compliance/          # Security compliance tests
├── performance/             # Performance testing
│   ├── load/                # Load testing
│   ├── stress/              # Stress testing
│   └── benchmarks/          # Benchmark tests
├── fuzz/                    # Fuzz testing
│   ├── fuzzing/             # Fuzz test suites
│   ├── property-based/      # Property-based testing
│   └── reports/             # Fuzz test reports
├── archive/                 # Archived/deprecated tests
├── results/                 # Test results and artifacts
│   ├── coverage/            # Coverage reports
│   ├── playwright-report/   # Playwright HTML reports
│   └── test-artifacts/      # Test artifacts and screenshots
├── utils/                   # Test utilities and helpers
├── mocks/                   # Mock objects and data
├── load/                    # Load testing configurations
└── configs/                 # Test configurations
```

## Configuration Files

### Jest Configuration

- **Main Config**: `tests/jest.config.js` - Unified Jest configuration for all test types
- **Projects**: Separate project configurations for unit, integration, security, and frontend tests

### Playwright Configuration

- **Main Config**: `tests/playwright.config.js` - Unified Playwright configuration for E2E tests
- **Projects**: Browser-specific and test-suite-specific configurations

## Running Tests

### All Tests

```bash
npm run test:all                    # Run all test suites
```

### By Test Type

```bash
npm run test:unit                   # Unit tests only
npm run test:integration            # Integration tests only
npm run test:e2e                   # End-to-end tests only
npm run test:security              # Security tests only
npm run test:performance           # Performance tests only
npm run test:fuzz                  # Fuzz tests only
npm run test:smart-contracts       # Smart contract tests only
```

### With Options

```bash
npm run test:watch                 # Watch mode
npm run test:coverage              # With coverage
npm run test:e2e:headed           # E2E tests with browser UI
npm run test:e2e:debug            # E2E tests in debug mode
npm run test:integration:verbose   # Verbose integration tests
```

### Load and Performance Testing

```bash
npm run test:load                  # Load testing with Artillery
npm run test:performance          # Comprehensive performance tests
npm run perf:benchmark            # API benchmarks
```

## Test Organization Principles

### 1. **Separation of Concerns**

- **Unit Tests**: Test individual functions/components in isolation
- **Integration Tests**: Test component interactions and API endpoints
- **E2E Tests**: Test complete user workflows across the full application stack
- **Security Tests**: Focused security validation and penetration testing
- **Performance Tests**: Load, stress, and benchmark testing

### 2. **Consistent Naming**

- Test files use `.test.js` or `.spec.js` extensions
- Integration tests include `.integration.test.js` for clarity
- E2E tests are organized by workflow or feature area
- Performance tests include performance metrics and benchmarks

### 3. **Shared Utilities**

- Common test utilities in `tests/utils/`
- Mock objects and test data in `tests/mocks/`
- Test database setup and teardown helpers
- Global setup and teardown for different test types

## Migration from Old Structure

This unified structure consolidates tests from:

- `/test/` - Legacy test directory
- `/consolidated-tests/` - Previously consolidated tests
- `/fuzz-testing/` - Fuzz testing suite
- `/test-archive/` - Archived tests
- `/test-results/` - Test results and artifacts

### Key Improvements

1. **Single Test Directory**: All tests now live under `/tests/`
2. **Unified Configurations**: Single Jest and Playwright configs with projects
3. **Organized by Type**: Clear separation of unit, integration, e2e, security, and performance
   tests
4. **Centralized Results**: All test artifacts and reports in `/tests/results/`
5. **Updated Scripts**: Package.json scripts updated to use new structure

## Contributing

### Adding New Tests

1. **Unit Tests**: Add to appropriate subdirectory in `tests/unit/`
2. **Integration Tests**: Add to relevant area in `tests/integration/`
3. **E2E Tests**: Add workflow tests to `tests/e2e/workflows/`
4. **Security Tests**: Add to `tests/security/` with appropriate categorization

### Test Standards

- Follow existing naming conventions
- Include comprehensive test documentation
- Add appropriate mocks and test data
- Ensure tests are independent and can run in parallel
- Include both positive and negative test cases

### Coverage Requirements

- **Backend**: 80% minimum coverage
- **Frontend**: 70% minimum coverage
- **Smart Contracts**: Tested via Hardhat with 75% minimum
- **Critical Paths**: 90% minimum coverage required

## Dependencies

### Testing Frameworks

- **Jest**: JavaScript/TypeScript unit and integration testing
- **Playwright**: End-to-end browser testing
- **Hardhat**: Smart contract testing
- **Artillery**: Load testing

### Test Utilities

- **@testing-library**: React component testing utilities
- **Supertest**: HTTP API testing
- **Sinon**: Mocking and stubbing
- **Fast-check**: Property-based testing

## CI/CD Integration

This test structure integrates with the CI/CD pipeline:

- **PR Checks**: Unit and integration tests run on every PR
- **Nightly Tests**: Full E2E and security test suite
- **Performance Monitoring**: Continuous performance benchmarks
- **Security Scans**: Regular security and fuzz testing

For more information, see the individual test directories and configuration files.
