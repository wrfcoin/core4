/**
 * Chaos Test Setup
 *
 * Global setup for chaos/resilience tests.
 */

import { assertProductionSafe } from './helpers/chaos-utils';

// Block chaos tests in production
beforeAll(() => {
  assertProductionSafe();
});

// Mock logger to prevent console spam during tests
jest.mock('../../backend/server/utils/logger', () => ({
  appLogger: {
    info: jest.fn(),
    warn: jest.fn(),
    error: jest.fn(),
    debug: jest.fn(),
  },
}));

// Set test timeout for all chaos tests
jest.setTimeout(120000);
