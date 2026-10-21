# Integration Tests

This directory contains integration tests that verify interactions between different platform
components.

## Test Categories

### Component Integration Tests

- **`wallet-nft-integration.test.js`**: Tests interactions between wallet and NFT marketplace

  - NFT purchase flows with wallet balance updates
  - NFT creation and listing with gas fee handling
  - NFT transfers between wallets with ownership verification
  - Batch NFT operations with gas optimization
  - NFT royalty payments integration

- **`governance-wallet-integration.test.js`**: Tests interactions between governance and wallet
  systems

  - Proposal creation with token staking
  - Voting with delegated voting power
  - Proposal execution and treasury fund distribution
  - Token vesting and wallet integration
  - Multi-signature governance proposals

- **`cross-chain-wallet-integration.test.js`**: Tests cross-chain bridge and wallet interactions
  - Cross-chain token bridging with balance updates
  - Atomic swap operations
  - Multi-asset portfolio rebalancing
  - Bridge failure recovery mechanisms

## Test Structure

Each integration test follows this pattern:

1. **Setup**: Initialize test database and mock services
2. **Component A Action**: Trigger action in first component
3. **State Verification**: Verify intermediate state changes
4. **Component B Response**: Verify second component responds correctly
5. **End-to-End Verification**: Confirm complete integration works
6. **Cleanup**: Reset test environment

## Mock Services

Integration tests use comprehensive mock services:

- **Mock Blockchain Service**: Simulates blockchain transactions and confirmations
- **Mock WebSocket Service**: Real-time data streams between components
- **Mock External APIs**: Weather services, cross-chain protocols
- **Test Database**: Isolated schemas for parallel test execution

## Running Integration Tests

```bash
# Run all integration tests
npm run test:integration

# Run specific integration test
npx jest tests/integration/wallet-nft-integration.test.js

# Run with coverage
npm run test:integration -- --coverage

# Run in watch mode
npm run test:integration -- --watch
```

## Rust Harness Coverage (`tests/integration/src/`)

`tests/integration/src/` is a Rust integration harness crate (`test_server`) with executable
tests for the blockchain/NFT/agent/node wiring used by the local simulation server.

```bash
# Run the Rust harness tests
cargo test -p test_server
```

## Test Data

Integration tests use realistic test data:

- **User Accounts**: Pre-configured test users with wallets
- **Token Balances**: Sufficient balances for all test operations
- **NFT Assets**: Sample NFTs and metadata
- **Governance Proposals**: Template proposals for testing

## Performance Considerations

- Tests use database transactions for isolation
- Mock services respond within realistic timeframes
- Parallel execution with separate test schemas
- Cleanup operations prevent test interference

## Best Practices

1. **Test Real Interactions**: Don't mock the integration points
2. **Verify State Changes**: Check both components reflect changes
3. **Handle Async Operations**: Properly wait for blockchain confirmations
4. **Test Error Scenarios**: Include failure cases and recovery
5. **Maintain Test Data**: Keep test fixtures realistic and up-to-date

## Dependencies

Integration tests require:

- Test database with proper permissions
- Mock service infrastructure
- Realistic test data fixtures
- Proper cleanup mechanisms

See main test README for complete setup instructions.
