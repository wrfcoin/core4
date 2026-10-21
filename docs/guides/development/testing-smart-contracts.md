# Smart Contract Testing Guide

This guide provides practical examples of using the Wrfcoin Smart Contract Testing utilities in your
development workflow. These tools help you ensure your contracts are secure, efficient, and
reliable.

## Table of Contents

1. [Basic Contract Testing](#basic-contract-testing)
2. [Security Vulnerability Scanning](#security-vulnerability-scanning)
3. [Gas Optimization](#gas-optimization)
4. [Advanced Testing Scenarios](#advanced-testing-scenarios)

## Basic Contract Testing

The `ContractTestEnvironment` provides a controlled environment for testing smart contracts. Here's
a basic example:

```javascript
const { ContractTestEnvironment } = require('../../utils/testing');

// Create a test environment
const env = new ContractTestEnvironment({
  initialBalances: {
    // Predefined test accounts with balances
    WRF1TestUser1: 1000,
    WRF1TestUser2: 500,
  },
});

// Deploy a contract for testing
const contract = {
  name: 'WeatherDataContract',
  code: '/* contract code here */',
  initialState: {
    owner: 'WRF1TestUser1',
    dataPoints: {},
  },
};

env.deployContract(contract);

// Test a contract function call
const result = env.callContractFunction({
  caller: 'WRF1TestUser1',
  function: 'addWeatherData',
  params: {
    location: 'NYC',
    temperature: 22.5,
    humidity: 65,
    timestamp: Date.now(),
  },
});

console.log('Function call result:', result);

// Verify contract state after the call
const contractState = env.getContractState(contract.name);
console.log('Contract state:', contractState);
```

## Security Vulnerability Scanning

The `VulnerabilityScanner` helps identify common security vulnerabilities in your contracts:

```javascript
const { ContractTestEnvironment, VulnerabilityScanner } = require('../../utils/testing');

// Create a test environment with your contract
const env = new ContractTestEnvironment();
env.deployContract({
  name: 'WeatherOracleContract',
  code: '/* contract code here */',
  initialState: {
    /* initial state */
  },
});

// Create a scanner and run all security tests
const scanner = new VulnerabilityScanner();
const scanResults = scanner.scanContract(env);

// Check results
console.log('Vulnerability scan results:', scanResults);

// You can also run specific tests
const reentrancyResults = scanner.testReentrancyAttack(env);
console.log('Reentrancy test results:', reentrancyResults);

// Fix any detected issues and scan again to verify
if (scanResults.vulnerabilitiesFound) {
  console.log('Security issues found. Please fix before deploying!');
}
```

## Gas Optimization

The `GasAnalyzer` helps you optimize your contract for gas efficiency:

```javascript
const { ContractTestEnvironment, GasAnalyzer } = require('../../utils/testing');

// Create a test environment
const env = new ContractTestEnvironment();
env.deployContract({
  name: 'WeatherDataAggregator',
  code: '/* contract code here */',
  initialState: {
    /* initial state */
  },
});

// Define functions to analyze
const functionsToTest = {
  addDataPoint: {
    location: 'NYC',
    temperature: 22.5,
    timestamp: Date.now(),
  },
  calculateAverage: {
    location: 'NYC',
    timeRange: {
      start: Date.now() - 86400000,
      end: Date.now(),
    },
  },
};

// Create a gas analyzer
const analyzer = new GasAnalyzer();

// Record baseline gas usage
analyzer.recordBaseline(env, functionsToTest);

// After making optimizations to your contract, analyze the difference
env.updateContract('WeatherDataAggregator', '/* optimized contract code */');
const analysisResults = analyzer.analyzeGasUsage(env, functionsToTest);

console.log('Gas optimization results:', analysisResults);

// Find functions with the highest gas savings
const savings = analysisResults.functions.map(f => ({
  name: f.name,
  savings: f.baselineGas - f.currentGas,
  percentSaved: ((f.baselineGas - f.currentGas) / f.baselineGas) * 100,
}));

console.log(
  'Gas savings by function:',
  savings.sort((a, b) => b.savings - a.savings)
);
```

## Advanced Testing Scenarios

### Testing Role-Based Access Control

```javascript
const { ContractTestEnvironment } = require('../../utils/testing');

// Create test environment with multiple user roles
const env = new ContractTestEnvironment();

// Setup users with different roles
env.createAccount('WRF1Admin', 'admin', 1000);
env.createAccount('WRF1DataProvider', 'provider', 500);
env.createAccount('WRF1User', 'user', 100);

// Deploy contract with role-based permissions
env.deployContract({
  name: 'WeatherDataRegistry',
  code: '/* contract code with RBAC */',
  initialState: {
    owner: 'WRF1Admin',
    providers: ['WRF1DataProvider'],
    data: {},
  },
});

// Test authorized access (should succeed)
const authorizedResult = env.callContractFunction({
  caller: 'WRF1DataProvider',
  function: 'submitWeatherData',
  params: {
    location: 'London',
    temperature: 15.2,
    timestamp: Date.now(),
  },
});

// Test unauthorized access (should fail)
const unauthorizedResult = env.callContractFunction({
  caller: 'WRF1User',
  function: 'submitWeatherData',
  params: {
    location: 'London',
    temperature: 16.0,
    timestamp: Date.now(),
  },
});

console.log('Authorized call result:', authorizedResult);
console.log('Unauthorized call result:', unauthorizedResult);

// Check if access control is working as expected
if (authorizedResult.success && !unauthorizedResult.success) {
  console.log('Role-based access control is working correctly');
} else {
  console.log('Role-based access control has issues');
}
```

### Testing Contract Upgrades

```javascript
const { ContractTestEnvironment } = require('../../utils/testing');

// Create test environment
const env = new ContractTestEnvironment();

// Deploy initial version of contract
env.deployContract({
  name: 'WeatherDataStorageV1',
  code: '/* V1 contract code */',
  initialState: {
    owner: 'WRF1TestAdmin',
    version: '1.0',
    data: {},
  },
});

// Add some data to the contract
env.callContractFunction({
  caller: 'WRF1TestAdmin',
  function: 'addData',
  params: {
    location: 'Berlin',
    temperature: 18.5,
  },
});

// Get state before upgrade
const stateBeforeUpgrade = env.getContractState('WeatherDataStorageV1');

// Simulate contract upgrade
env.upgradeContract({
  name: 'WeatherDataStorageV1',
  newCode: '/* V2 contract code */',
  migrationFunction: oldState => {
    // Data migration logic between versions
    return {
      ...oldState,
      version: '2.0',
      // Add new fields or transform existing ones
      processedData: Object.entries(oldState.data).map(([loc, data]) => ({
        location: loc,
        celsius: data.temperature,
        fahrenheit: (data.temperature * 9) / 5 + 32,
      })),
    };
  },
});

// Get state after upgrade
const stateAfterUpgrade = env.getContractState('WeatherDataStorageV1');

console.log('State before upgrade:', stateBeforeUpgrade);
console.log('State after upgrade:', stateAfterUpgrade);

// Test new functionality in upgraded contract
const newFunctionResult = env.callContractFunction({
  caller: 'WRF1TestAdmin',
  function: 'getTemperatureInFahrenheit',
  params: {
    location: 'Berlin',
  },
});

console.log('New function result:', newFunctionResult);
```

These examples demonstrate how to use the smart contract testing utilities in the Wrfcoin Testing
Framework for various testing scenarios. You can adapt these examples to your specific contract
needs.
