require('@nomicfoundation/hardhat-ethers');
require('@nomicfoundation/hardhat-chai-matchers');
require('@nomicfoundation/hardhat-network-helpers');
require('@nomicfoundation/hardhat-ignition-ethers');
require('@nomiclabs/hardhat-solhint');
require('hardhat-slither');
// Note: hardhat-docgen removed due to Vue 2 EOL vulnerabilities (GHSA-5j4c-8p2g-v4jx, GHSA-g3ch-rx76-35fx)
// Use solidity-docgen instead for contract documentation
require('@nomicfoundation/hardhat-verify');

// Custom task to compile templates
task('compile-templates', 'Compiles smart contract templates').setAction(
  async (taskArgs, hre) => {
    const originalSourcesPath = hre.config.paths.sources;
    const originalArtifactsPath = hre.config.paths.artifacts;

    try {
      // Temporarily change paths to compile templates
      hre.config.paths.sources = './templates';
      hre.config.paths.artifacts = './artifacts/templates';

      // Ensure artifacts/templates directory exists
      const fs = require('fs');
      const path = require('path');
      const templatesArtifactsDir = path.join(
        __dirname,
        'artifacts',
        'templates'
      );
      if (!fs.existsSync(templatesArtifactsDir)) {
        fs.mkdirSync(templatesArtifactsDir, { recursive: true });
      }

      console.log('Compiling templates from ./templates directory...');

      // Run compilation
      await hre.run('compile', { quiet: false });

      console.log('Templates compilation completed successfully!');
    } catch (error) {
      console.error('Template compilation failed:', error.message);
      throw error;
    } finally {
      // Restore original paths
      hre.config.paths.sources = originalSourcesPath;
      hre.config.paths.artifacts = originalArtifactsPath;
    }
  }
);

// Custom task to compile both contracts and templates
task('compile-all', 'Compiles both contracts and templates').setAction(
  async (taskArgs, hre) => {
    console.log('Compiling main contracts...');
    await hre.run('compile');

    console.log('Compiling templates...');
    await hre.run('compile-templates');

    console.log('All compilations completed successfully!');
  }
);

// Security-focused tasks for WRFCoin platform
task('security-audit', 'Runs comprehensive security audit')
  .addOptionalParam('contract', 'Specific contract to audit')
  .setAction(async (taskArgs, hre) => {
    console.log('🔐 Running WRFCoin Platform Security Audit...');

    try {
      // 1. Compile contracts first
      console.log('1. Compiling contracts...');
      await hre.run('compile');

      // 2. Run Solhint linting
      console.log('2. Running Solhint security linting...');
      await hre.run('check');

      // 3. Run Slither static analysis
      console.log('3. Running Slither static analysis...');
      await hre.run('slither');

      console.log('✅ Security audit completed successfully!');
      console.log('📊 Check slither-report.json for detailed results');
    } catch (error) {
      console.error('❌ Security audit failed:', error.message);
      process.exit(1);
    }
  });

// Gas optimization analysis task
task('gas-analysis', 'Analyzes gas usage for WRFCoin contracts').setAction(
  async (taskArgs, hre) => {
    console.log('⛽ Running Gas Analysis for WRFCoin Platform...');

    const fs = require('fs');
    const path = require('path');

    // Run compilation to get gas estimates
    await hre.run('compile');

    const artifactsPath = path.join(__dirname, 'artifacts', 'contracts');
    const gasReport = {};

    function analyzeArtifacts(dir) {
      const files = fs.readdirSync(dir);

      for (const file of files) {
        const filePath = path.join(dir, file);
        const stat = fs.statSync(filePath);

        if (stat.isDirectory()) {
          analyzeArtifacts(filePath);
        } else if (file.endsWith('.json') && !file.endsWith('.dbg.json')) {
          try {
            const artifact = JSON.parse(fs.readFileSync(filePath, 'utf8'));
            if (artifact.bytecode && artifact.bytecode.length > 2) {
              const contractName = artifact.contractName;
              const bytecodeSize = (artifact.bytecode.length - 2) / 2; // Remove 0x and divide by 2

              gasReport[contractName] = {
                bytecodeSize: bytecodeSize,
                deploymentGas: Math.ceil(bytecodeSize * 200 + 32000), // Rough estimate
                maxContractSize: 24576, // EIP-170 limit
                sizeUtilization:
                  ((bytecodeSize / 24576) * 100).toFixed(2) + '%',
              };
            }
          } catch (error) {
            // Skip invalid JSON files
          }
        }
      }
    }

    analyzeArtifacts(artifactsPath);

    // Output gas report
    console.log('\n📊 WRFCoin Contract Gas Analysis:');
    console.log('='.repeat(80));

    for (const [contract, data] of Object.entries(gasReport)) {
      console.log(`\n📋 Contract: ${contract}`);
      console.log(`   Bytecode Size: ${data.bytecodeSize} bytes`);
      console.log(`   Est. Deploy Gas: ${data.deploymentGas.toLocaleString()}`);
      console.log(`   Size Utilization: ${data.sizeUtilization}`);

      if (data.bytecodeSize > 20000) {
        console.log(`   ⚠️  WARNING: Large contract size`);
      }
      if (data.bytecodeSize > 24000) {
        console.log(`   🚨 CRITICAL: Approaching EIP-170 limit!`);
      }
    }

    // Save report to file
    const reportPath = path.join(__dirname, 'gas-analysis-report.json');
    fs.writeFileSync(reportPath, JSON.stringify(gasReport, null, 2));
    console.log(`\n💾 Detailed report saved to: ${reportPath}`);
  }
);

// Multi-signature wallet security test task
task(
  'test-multisig-security',
  'Runs comprehensive security tests for SecureMultiSigWallet'
).setAction(async (taskArgs, hre) => {
  console.log('🔐 Testing SecureMultiSigWallet Security...');

  // Set environment variable for security testing
  process.env.SECURITY_TEST_MODE = 'true';

  try {
    await hre.run('test', {
      testFiles: ['test/security/SecureMultiSigWallet.security.test.js'],
    });
    console.log('✅ Multi-signature wallet security tests passed!');
  } catch (error) {
    console.error(
      '❌ Multi-signature wallet security tests failed:',
      error.message
    );
    process.exit(1);
  }
});

// Staking reward calculator security test task
task(
  'test-staking-security',
  'Runs comprehensive security tests for SecureStakingRewardCalculator'
).setAction(async (taskArgs, hre) => {
  console.log('🔐 Testing SecureStakingRewardCalculator Security...');

  // Set environment variable for security testing
  process.env.SECURITY_TEST_MODE = 'true';

  try {
    await hre.run('test', {
      testFiles: [
        'test/security/SecureStakingRewardCalculator.security.test.js',
      ],
    });
    console.log('✅ Staking reward calculator security tests passed!');
  } catch (error) {
    console.error(
      '❌ Staking reward calculator security tests failed:',
      error.message
    );
    process.exit(1);
  }
});

/** @type import('hardhat/config').HardhatUserConfig */
/**
 * Require account configuration from environment variables for non-ephemeral networks.
 * Throws at config load time if neither private key nor mnemonic env var is set.
 */
function requireAccounts(networkName, privateKeyEnv, mnemonicEnv, accountCount = 20) {
  if (process.env[privateKeyEnv]) {
    return [process.env[privateKeyEnv]];
  }
  if (process.env[mnemonicEnv]) {
    return { mnemonic: process.env[mnemonicEnv], count: accountCount };
  }
  throw new Error(
    `${networkName} requires account configuration. Set ${privateKeyEnv} or ${mnemonicEnv} environment variable.`
  );
}

module.exports = {
  solidity: {
    version: '0.8.28',
    settings: {
      optimizer: {
        enabled: true,
        runs: 200,
      },
      evmVersion: 'paris', // Latest stable EVM version
      viaIR: true, // Enable IR optimization to resolve stack depth issues
      outputSelection: {
        '*': {
          '*': ['storageLayout', 'metadata'],
        },
      },
      metadata: {
        // Append the swarm hash to the bytecode for verification
        appendCBOR: true,
        // Include sources in metadata for better debugging
        useLiteralContent: true,
      },
    },
  },
  networks: {
    hardhat: {
      chainId: 1337,
      hardfork: 'cancun', // Use cancun to avoid EIP-7825 per-tx gas cap (16M) from osaka/fusaka
      gas: 30000000, // Increased for complex security contracts
      blockGasLimit: 30000000,
      gasPrice: 8000000000, // 8 gwei for testing
      allowUnlimitedContractSize: true,
      accounts: {
        mnemonic: 'test test test test test test test test test test test junk',
        count: 50, // More accounts for comprehensive testing
        accountsBalance: '10000000000000000000000000', // 10,000,000 ETH per account for validator staking tests
      },
    },
    localhost: {
      url: 'http://127.0.0.1:8545',
      gas: 30000000,
      blockGasLimit: 30000000,
      gasPrice: 8000000000,
      timeout: 60000, // 60 second timeout
    },
    // WRFCoin development network
    wrfcoin_dev: {
      url: 'http://localhost:4001', // Main API server port
      chainId: 31337,
      gas: 15000000,
      gasPrice: 20000000000, // 20 gwei
      timeout: 60000,
      accounts: requireAccounts('wrfcoin_dev', 'WRFCOIN_DEV_PRIVATE_KEY', 'WRFCOIN_DEV_MNEMONIC', 20),
    },
    // WRFCoin testnet configuration
    wrfcoin_testnet: {
      url: process.env.WRFCOIN_TESTNET_URL || 'http://localhost:4001',
      chainId: 31338,
      gas: 12000000,
      gasPrice: 30000000000, // 30 gwei
      timeout: 120000, // 2 minute timeout for testnet
      accounts: requireAccounts('wrfcoin_testnet', 'WRFCOIN_TESTNET_PRIVATE_KEY', 'WRFCOIN_TESTNET_MNEMONIC', 10),
    },
    // Production network (placeholder)
    wrfcoin_mainnet: {
      url: process.env.WRFCOIN_MAINNET_URL || '',
      chainId: 31339,
      gas: 8000000,
      gasPrice: 40000000000, // 40 gwei
      timeout: 300000, // 5 minute timeout for mainnet
      accounts: process.env.WRFCOIN_MAINNET_PRIVATE_KEY
        ? [process.env.WRFCOIN_MAINNET_PRIVATE_KEY]
        : [],
    },
  },
  paths: {
    sources: './contracts/contracts',
    tests: './test',
    cache: './cache',
    artifacts: './artifacts',
    root: './',
    coverage: './coverage',
    coverageJson: './coverage.json',
  },
  mocha: {
    timeout: 300000, // 5 minutes for comprehensive security tests
    grep: '@skip-on-coverage',
    invert: true,
    reporter: 'spec', // Detailed test output
    slow: 10000, // Tests slower than 10s are marked as slow
    bail: false, // Continue testing after failures
  },
  solhint: {
    config: {
      extends: 'solhint:recommended',
      rules: {
        'compiler-version': ['error', '^0.8.28'],
        'func-visibility': ['warn', { ignoreConstructors: true }],
        'max-line-length': ['error', 120],
        'bracket-align': 'error',
        'code-complexity': ['error', 8],
        'function-max-lines': ['error', 50],
        'max-states-count': ['error', 15],
        'no-unused-vars': 'error',
        'avoid-suicide': 'error',
        'avoid-sha3': 'error',
        'avoid-tx-origin': 'error',
        'check-send-result': 'error',
        'func-param-name-mixedcase': 'error',
        'var-name-mixedcase': 'error',
        'event-name-camelcase': 'error',
        'modifier-name-mixedcase': 'error',
        'private-vars-leading-underscore': 'error',
        'use-forbidden-name': 'error',
        'visibility-modifier-order': 'error',
        'imports-on-top': 'error',
        ordering: 'error',
        'no-mix-tabs-and-spaces': 'error',
        quotes: ['error', 'double'],
        'no-console': 'warn',
        'avoid-low-level-calls': 'warn',
        'avoid-call-value': 'warn',
        'compiler-gt-0_4': 'error',
        'payable-fallback': 'error',
        'reason-string': ['warn', { maxLength: 64 }],
        'constructor-syntax': 'error',
      },
    },
  },
  slither: {
    // Enhanced Slither configuration for WRFCoin security analysis
    exclude: [
      'external-dependency',
      'low-level-calls', // Too noisy for legitimate use cases
      'timestamp', // Handled by our timestamp validation
    ],
    detectors: [
      'all', // Run all detectors
      '!unused-state', // Exclude if too noisy
      '!similar-names', // Exclude if too noisy
      '!too-many-digits', // Exclude if too noisy
    ],
    compilationPath: './artifacts/contracts',
    excludeContracts: [
      'Migrations.sol',
      'Mock*.sol',
      'Test*.sol',
      '*Mock.sol',
      '*Test.sol',
    ],
    filterPaths: 'lib/,node_modules/,test/',
    // Enhanced security analysis for high-value contracts
    highSeverityOnly: false,
    failOnDetectionResults: process.env.CI === 'true', // Fail CI on security issues
    outputFormat: 'json', // For automated processing
    outputFile: './slither-report.json',
  },
  etherscan: {
    apiKey: {
      mainnet: process.env.ETHERSCAN_API_KEY,
      polygon: process.env.POLYGONSCAN_API_KEY,
      bsc: process.env.BSCSCAN_API_KEY,
      avalanche: process.env.SNOWTRACE_API_KEY,
    },
  },
  sourcify: {
    enabled: true,
  },
};
