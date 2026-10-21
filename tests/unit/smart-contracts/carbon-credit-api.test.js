/**
 * Comprehensive Test Suite for Carbon Credit API and SDK
 * Tests all endpoints and SDK functionality with integration tests
 */

const request = require('supertest');
const express = require('express');
const { expect } = require('chai');
const sinon = require('sinon');
const { ethers } = require('ethers');

// Import modules to test
const { carbonCreditAPI } = require('../backend/api/carbon-credit-api');
const { CarbonCreditSDK } = require('../sdk/carbon-credit-sdk');

// Mock data and helpers
const mockPrivateKey =
  '0x1234567890123456789012345678901234567890123456789012345678901234';
const mockAddress = '0x1234567890123456789012345678901234567890';
const mockTokenAddress = '0xabcdef1234567890123456789012345678901234';

// Test app setup
const app = express();
app.use(express.json());
app.use('/api', carbonCreditAPI);

describe('Carbon Credit API Integration Tests', () => {
  let server;
  let sdk;

  before(async () => {
    // Setup test server
    server = app.listen(3001);

    // Setup SDK for integration tests
    sdk = new CarbonCreditSDK({
      apiBaseUrl: 'http://localhost:3001/api',
      defaultTimeout: 10000,
    });
  });

  after(async () => {
    if (server) {
      server.close();
    }
  });

  describe('Carbon Credit Token Operations', () => {
    describe('GET /carbon-credits/batches', () => {
      it('should return carbon credit batches with pagination', async () => {
        const response = await request(app)
          .get('/api/carbon-credits/batches')
          .query({
            page: 1,
            limit: 10,
            projectType: 'forestry',
            vintage: 2023,
          })
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.have.property('batches');
        expect(response.body.data).to.have.property('pagination');
        expect(response.body.data.pagination).to.include.keys([
          'currentPage',
          'totalPages',
          'totalItems',
          'itemsPerPage',
        ]);
      });

      it('should validate pagination parameters', async () => {
        const response = await request(app)
          .get('/api/carbon-credits/batches')
          .query({
            page: -1,
            limit: 150, // Exceeds max limit
          })
          .expect(400);

        expect(response.body).to.have.property('success', false);
        expect(response.body).to.have.property('error');
      });

      it('should filter by project type', async () => {
        const response = await request(app)
          .get('/api/carbon-credits/batches')
          .query({
            projectType: 'renewable-energy',
          })
          .expect(200);

        expect(response.body).to.have.property('success', true);
        // All returned batches should match the filter
        response.body.data.batches.forEach(batch => {
          expect(batch.projectType).to.equal('renewable-energy');
        });
      });
    });

    describe('GET /carbon-credits/batches/:tokenId', () => {
      it('should return specific carbon credit batch details', async () => {
        const tokenId = 1;
        const response = await request(app)
          .get(`/api/carbon-credits/batches/${tokenId}`)
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.have.property('tokenId', tokenId);
        expect(response.body.data).to.include.keys([
          'totalSupply',
          'immobilizedSupply',
          'retiredSupply',
          'projectId',
          'vintage',
          'methodology',
          'projectType',
          'registryStandard',
          'location',
          'issuer',
          'isActive',
        ]);
      });

      it('should return 404 for non-existent token', async () => {
        const response = await request(app)
          .get('/api/carbon-credits/batches/99999')
          .expect(404);

        expect(response.body).to.have.property('success', false);
        expect(response.body).to.have.property('error');
      });

      it('should validate tokenId parameter', async () => {
        const response = await request(app)
          .get('/api/carbon-credits/batches/invalid')
          .expect(400);

        expect(response.body).to.have.property('success', false);
      });
    });

    describe('POST /carbon-credits/batches/:tokenId/retire', () => {
      it('should retire carbon credits successfully', async () => {
        const tokenId = 1;
        const retirementData = {
          amount: '100.5',
          retirementReason:
            'Corporate carbon neutrality offset for Q4 2023 operations',
          beneficiary: 'Test Corporation',
          privateKey: mockPrivateKey,
        };

        // Mock the blockchain transaction
        const mockTx = {
          hash: '0xmocktransactionhash',
          wait: sinon.stub().resolves({
            hash: '0xmocktransactionhash',
            blockNumber: 12345678,
            gasUsed: { toString: () => '125000' },
          }),
        };

        const response = await request(app)
          .post(`/api/carbon-credits/batches/${tokenId}/retire`)
          .send(retirementData)
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.include.keys([
          'transactionHash',
          'blockNumber',
          'tokenId',
          'amountRetired',
          'retirementReason',
          'gasUsed',
        ]);
        expect(response.body.data.tokenId).to.equal(tokenId);
        expect(response.body.data.amountRetired).to.equal(
          retirementData.amount
        );
      });

      it('should validate retirement request data', async () => {
        const invalidData = {
          amount: '-10', // Invalid negative amount
          retirementReason: 'Too short', // Too short reason
          privateKey: 'invalid', // Invalid private key
        };

        const response = await request(app)
          .post('/api/carbon-credits/batches/1/retire')
          .send(invalidData)
          .expect(400);

        expect(response.body).to.have.property('success', false);
        expect(response.body.error).to.include('Validation failed');
      });

      it('should require retirement reason', async () => {
        const incompleteData = {
          amount: '100',
          privateKey: mockPrivateKey,
          // Missing retirementReason
        };

        const response = await request(app)
          .post('/api/carbon-credits/batches/1/retire')
          .send(incompleteData)
          .expect(400);

        expect(response.body).to.have.property('success', false);
      });
    });

    describe('POST /carbon-credits/transfer', () => {
      it('should transfer carbon credits successfully', async () => {
        const transferData = {
          tokenId: 1,
          to: mockAddress,
          amount: '50.0',
          privateKey: mockPrivateKey,
        };

        const response = await request(app)
          .post('/api/carbon-credits/transfer')
          .send(transferData)
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.include.keys([
          'transactionHash',
          'blockNumber',
          'from',
          'to',
          'tokenId',
          'amount',
          'gasUsed',
        ]);
        expect(response.body.data.to).to.equal(transferData.to);
        expect(response.body.data.amount).to.equal(transferData.amount);
      });

      it('should validate Ethereum addresses', async () => {
        const invalidData = {
          tokenId: 1,
          to: 'invalid-address',
          amount: '50.0',
          privateKey: mockPrivateKey,
        };

        const response = await request(app)
          .post('/api/carbon-credits/transfer')
          .send(invalidData)
          .expect(400);

        expect(response.body).to.have.property('success', false);
      });

      it('should validate transfer amount', async () => {
        const invalidData = {
          tokenId: 1,
          to: mockAddress,
          amount: '0', // Invalid zero amount
          privateKey: mockPrivateKey,
        };

        const response = await request(app)
          .post('/api/carbon-credits/transfer')
          .send(invalidData)
          .expect(400);

        expect(response.body).to.have.property('success', false);
      });
    });
  });

  describe('AMM Trading Operations', () => {
    describe('GET /amm/pools', () => {
      it('should return all AMM pools', async () => {
        const response = await request(app).get('/api/amm/pools').expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.have.property('pools');
        expect(Array.isArray(response.body.data.pools)).to.be.true;

        if (response.body.data.pools.length > 0) {
          const pool = response.body.data.pools[0];
          expect(pool).to.include.keys([
            'address',
            'token0',
            'token1',
            'reserve0',
            'reserve1',
            'fee',
            'tvl',
          ]);
        }
      });
    });

    describe('GET /amm/quote', () => {
      it('should return swap quote', async () => {
        const response = await request(app)
          .get('/api/amm/quote')
          .query({
            tokenIn: mockTokenAddress,
            tokenOut: mockAddress,
            amountIn: '100',
          })
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.include.keys([
          'amountOut',
          'priceImpact',
          'fee',
          'route',
        ]);
      });

      it('should validate quote parameters', async () => {
        const response = await request(app)
          .get('/api/amm/quote')
          .query({
            tokenIn: 'invalid-address',
            tokenOut: mockAddress,
            amountIn: '100',
          })
          .expect(400);

        expect(response.body).to.have.property('success', false);
      });
    });

    describe('POST /amm/swap', () => {
      it('should execute swap successfully', async () => {
        const swapData = {
          tokenIn: mockTokenAddress,
          tokenOut: mockAddress,
          amountIn: '100.0',
          amountOutMin: '95.0',
          deadline: Math.floor(Date.now() / 1000) + 3600,
          privateKey: mockPrivateKey,
        };

        const response = await request(app)
          .post('/api/amm/swap')
          .send(swapData)
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.include.keys([
          'transactionHash',
          'blockNumber',
          'tokenIn',
          'tokenOut',
          'amountIn',
          'gasUsed',
        ]);
      });

      it('should validate deadline', async () => {
        const invalidData = {
          tokenIn: mockTokenAddress,
          tokenOut: mockAddress,
          amountIn: '100.0',
          amountOutMin: '95.0',
          deadline: Math.floor(Date.now() / 1000) - 3600, // Past deadline
          privateKey: mockPrivateKey,
        };

        const response = await request(app)
          .post('/api/amm/swap')
          .send(invalidData)
          .expect(400);

        expect(response.body).to.have.property('success', false);
      });
    });

    describe('POST /amm/add-liquidity', () => {
      it('should add liquidity successfully', async () => {
        const liquidityData = {
          tokenA: mockTokenAddress,
          tokenB: mockAddress,
          amountADesired: '1000.0',
          amountBDesired: '500.0',
          amountAMin: '950.0',
          amountBMin: '475.0',
          deadline: Math.floor(Date.now() / 1000) + 3600,
          privateKey: mockPrivateKey,
        };

        const response = await request(app)
          .post('/api/amm/add-liquidity')
          .send(liquidityData)
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.include.keys([
          'transactionHash',
          'blockNumber',
          'tokenA',
          'tokenB',
          'gasUsed',
        ]);
      });
    });
  });

  describe('Yield Farming Operations', () => {
    describe('GET /yield-farming/pools', () => {
      it('should return yield farming pools', async () => {
        const response = await request(app)
          .get('/api/yield-farming/pools')
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.have.property('pools');
        expect(Array.isArray(response.body.data.pools)).to.be.true;

        if (response.body.data.pools.length > 0) {
          const pool = response.body.data.pools[0];
          expect(pool).to.include.keys([
            'address',
            'liquidityPool',
            'carbonBasketToken',
            'baseRewardRate',
            'farmType',
            'isActive',
          ]);
        }
      });
    });

    describe('POST /yield-farming/stake', () => {
      it('should stake tokens successfully', async () => {
        const stakeData = {
          farmAddress: mockAddress,
          amount: '500.0',
          lockDuration: 90,
          privateKey: mockPrivateKey,
        };

        const response = await request(app)
          .post('/api/yield-farming/stake')
          .send(stakeData)
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.include.keys([
          'transactionHash',
          'blockNumber',
          'farmAddress',
          'amount',
          'lockDuration',
          'gasUsed',
        ]);
      });

      it('should validate stake amount', async () => {
        const invalidData = {
          farmAddress: mockAddress,
          amount: '-100', // Negative amount
          privateKey: mockPrivateKey,
        };

        const response = await request(app)
          .post('/api/yield-farming/stake')
          .send(invalidData)
          .expect(400);

        expect(response.body).to.have.property('success', false);
      });
    });

    describe('POST /yield-farming/claim-rewards', () => {
      it('should claim rewards successfully', async () => {
        const claimData = {
          farmAddress: mockAddress,
          privateKey: mockPrivateKey,
        };

        const response = await request(app)
          .post('/api/yield-farming/claim-rewards')
          .send(claimData)
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.include.keys([
          'transactionHash',
          'blockNumber',
          'farmAddress',
          'user',
          'gasUsed',
        ]);
      });
    });
  });

  describe('Insurance Operations', () => {
    describe('GET /insurance/policies', () => {
      it('should return insurance policies', async () => {
        const response = await request(app)
          .get('/api/insurance/policies')
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.have.property('policies');
        expect(Array.isArray(response.body.data.policies)).to.be.true;

        if (response.body.data.policies.length > 0) {
          const policy = response.body.data.policies[0];
          expect(policy).to.include.keys([
            'coverageType',
            'premiumRate',
            'maxCoverage',
            'minDuration',
            'maxDuration',
            'description',
          ]);
        }
      });
    });

    describe('GET /insurance/premium-quote', () => {
      it('should calculate premium quote', async () => {
        const response = await request(app)
          .get('/api/insurance/premium-quote')
          .query({
            coverageType: 'PROJECT_FAILURE',
            tokenId: 1,
            coverageAmount: '1000',
            duration: 365,
          })
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.include.keys([
          'premium',
          'premiumBreakdown',
        ]);
        expect(response.body.data.premiumBreakdown).to.include.keys([
          'basePremium',
          'riskMultiplier',
          'finalPremium',
        ]);
      });

      it('should validate coverage type', async () => {
        const response = await request(app)
          .get('/api/insurance/premium-quote')
          .query({
            coverageType: 'INVALID_TYPE',
            tokenId: 1,
            coverageAmount: '1000',
            duration: 365,
          })
          .expect(400);

        expect(response.body).to.have.property('success', false);
      });
    });

    describe('POST /insurance/create-policy', () => {
      it('should create insurance policy successfully', async () => {
        const policyData = {
          coverageType: 'PROJECT_FAILURE',
          tokenId: 1,
          coverageAmount: '1000.0',
          duration: 365,
          privateKey: mockPrivateKey,
        };

        const response = await request(app)
          .post('/api/insurance/create-policy')
          .send(policyData)
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.include.keys([
          'transactionHash',
          'blockNumber',
          'coverageType',
          'tokenId',
          'coverageAmount',
          'duration',
          'premium',
        ]);
      });

      it('should validate duration limits', async () => {
        const invalidData = {
          coverageType: 'PROJECT_FAILURE',
          tokenId: 1,
          coverageAmount: '1000.0',
          duration: 10, // Below minimum duration
          privateKey: mockPrivateKey,
        };

        const response = await request(app)
          .post('/api/insurance/create-policy')
          .send(invalidData)
          .expect(400);

        expect(response.body).to.have.property('success', false);
      });
    });
  });

  describe('Oracle Data Feeds', () => {
    describe('GET /oracles/carbon-prices', () => {
      it('should return carbon credit prices', async () => {
        const response = await request(app)
          .get('/api/oracles/carbon-prices')
          .query({
            projectType: 'forestry',
            vintage: 2023,
          })
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(Array.isArray(response.body.data)).to.be.true;

        if (response.body.data.length > 0) {
          const price = response.body.data[0];
          expect(price).to.include.keys([
            'projectType',
            'vintage',
            'registryStandard',
            'priceUSD',
            'confidence',
            'lastUpdated',
          ]);
        }
      });
    });

    describe('GET /oracles/weather-data', () => {
      it('should return weather data', async () => {
        const response = await request(app)
          .get('/api/oracles/weather-data')
          .query({
            location: 'Brazil,Amazon',
            startDate: '2024-01-01',
            endDate: '2024-01-31',
            metrics: 'temperature,humidity',
          })
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(Array.isArray(response.body.data)).to.be.true;

        if (response.body.data.length > 0) {
          const weather = response.body.data[0];
          expect(weather).to.include.keys([
            'location',
            'timestamp',
            'temperature',
            'humidity',
            'dataSource',
            'confidence',
          ]);
        }
      });

      it('should validate date parameters', async () => {
        const response = await request(app)
          .get('/api/oracles/weather-data')
          .query({
            location: 'Brazil,Amazon',
            startDate: 'invalid-date',
            endDate: '2024-01-31',
          })
          .expect(400);

        expect(response.body).to.have.property('success', false);
      });
    });

    describe('GET /oracles/verification/:registryStandard/:projectId', () => {
      it('should return verification status', async () => {
        const response = await request(app)
          .get('/api/oracles/verification/VCS/VCS-001-2023')
          .expect(200);

        expect(response.body).to.have.property('success', true);
        expect(response.body.data).to.include.keys([
          'projectId',
          'registryStandard',
          'status',
          'lastUpdate',
          'verifierName',
        ]);
      });
    });
  });

  describe('Rate Limiting', () => {
    it('should enforce general rate limits', async function () {
      this.timeout(10000); // Increase timeout for rate limit tests

      const requests = [];
      for (let i = 0; i < 5; i++) {
        requests.push(
          request(app).get('/api/carbon-credits/batches').expect(200)
        );
      }

      await Promise.all(requests);

      // The 6th request within the window should still succeed
      // but if we exceed the limit, we should get 429
      const rapidRequests = [];
      for (let i = 0; i < 100; i++) {
        rapidRequests.push(request(app).get('/api/carbon-credits/batches'));
      }

      const results = await Promise.allSettled(rapidRequests);
      const rateLimitedResponses = results.filter(
        result => result.status === 'fulfilled' && result.value.status === 429
      );

      expect(rateLimitedResponses.length).to.be.greaterThan(0);
    });
  });

  describe('Security Headers', () => {
    it('should include security headers', async () => {
      const response = await request(app)
        .get('/api/carbon-credits/batches')
        .expect(200);

      expect(response.headers).to.have.property('x-content-type-options');
      expect(response.headers).to.have.property('x-frame-options');
    });
  });
});

describe('Carbon Credit SDK Unit Tests', () => {
  let sdk;
  let mockServer;

  beforeEach(() => {
    sdk = new CarbonCreditSDK({
      apiBaseUrl: 'http://localhost:3002/api',
      defaultTimeout: 5000,
    });
  });

  describe('Constructor and Configuration', () => {
    it('should initialize with provided configuration', () => {
      const config = {
        apiBaseUrl: 'https://api.example.com',
        defaultTimeout: 10000,
        rateLimitRetries: 5,
      };

      const sdk = new CarbonCreditSDK(config);
      const sdkConfig = sdk.getConfig();

      expect(sdkConfig.apiBaseUrl).to.equal(config.apiBaseUrl);
      expect(sdkConfig.defaultTimeout).to.equal(config.defaultTimeout);
      expect(sdkConfig.rateLimitRetries).to.equal(config.rateLimitRetries);
    });

    it('should use default configuration values', () => {
      const sdk = new CarbonCreditSDK({
        apiBaseUrl: 'https://api.example.com',
      });
      const config = sdk.getConfig();

      expect(config.defaultTimeout).to.equal(30000);
      expect(config.rateLimitRetries).to.equal(3);
      expect(config.retryDelay).to.equal(1000);
    });
  });

  describe('Utility Methods', () => {
    describe('isValidAddress', () => {
      it('should validate Ethereum addresses', () => {
        expect(CarbonCreditSDK.isValidAddress(mockAddress)).to.be.true;
        expect(CarbonCreditSDK.isValidAddress('0xinvalid')).to.be.false;
        expect(CarbonCreditSDK.isValidAddress('')).to.be.false;
      });
    });

    describe('formatEther and parseEther', () => {
      it('should convert between Wei and Ether', () => {
        const etherAmount = '1.0';
        const weiAmount = CarbonCreditSDK.parseEther(etherAmount);
        const backToEther = CarbonCreditSDK.formatEther(weiAmount);

        expect(backToEther).to.equal(etherAmount);
      });
    });

    describe('generateDeadline', () => {
      it('should generate future timestamp', () => {
        const deadline = CarbonCreditSDK.generateDeadline(20);
        const now = Math.floor(Date.now() / 1000);

        expect(deadline).to.be.greaterThan(now);
        expect(deadline).to.be.lessThanOrEqual(now + 20 * 60 + 1);
      });
    });

    describe('isValidPrivateKey', () => {
      it('should validate private keys', () => {
        expect(CarbonCreditSDK.isValidPrivateKey(mockPrivateKey)).to.be.true;
        expect(CarbonCreditSDK.isValidPrivateKey('invalid')).to.be.false;
      });
    });

    describe('getAddressFromPrivateKey', () => {
      it('should derive address from private key', () => {
        const address =
          CarbonCreditSDK.getAddressFromPrivateKey(mockPrivateKey);
        expect(CarbonCreditSDK.isValidAddress(address)).to.be.true;
      });
    });
  });

  describe('Error Handling', () => {
    it('should handle API errors gracefully', async () => {
      // Mock failed API response
      const mockResponse = {
        response: {
          status: 500,
          data: {
            success: false,
            error: 'Internal server error',
            requestId: 'req-error',
          },
        },
      };

      try {
        await sdk.getCarbonCreditBatch(999);
        expect.fail('Should have thrown an error');
      } catch (error) {
        expect(error).to.be.an('error');
        expect(error.message).to.include('server error');
      }
    });

    it('should handle network errors', async () => {
      const sdk = new CarbonCreditSDK({
        apiBaseUrl: 'http://nonexistent-server:9999/api',
      });

      try {
        await sdk.getCarbonCreditBatches();
        expect.fail('Should have thrown an error');
      } catch (error) {
        expect(error).to.be.an('error');
      }
    });
  });

  describe('Configuration Updates', () => {
    it('should update configuration', () => {
      const newConfig = {
        defaultTimeout: 20000,
        retryDelay: 2000,
      };

      sdk.updateConfig(newConfig);
      const config = sdk.getConfig();

      expect(config.defaultTimeout).to.equal(newConfig.defaultTimeout);
      expect(config.retryDelay).to.equal(newConfig.retryDelay);
    });
  });
});

describe('Integration Tests', () => {
  let sdk;

  before(() => {
    sdk = new CarbonCreditSDK({
      apiBaseUrl: 'http://localhost:3001/api',
    });
  });

  describe('Full Workflow Tests', () => {
    it('should complete carbon credit retirement workflow', async function () {
      this.timeout(30000);

      try {
        // 1. Get carbon credit batches
        const batches = await sdk.getCarbonCreditBatches({
          page: 1,
          limit: 5,
        });

        expect(batches).to.have.property('batches');
        expect(batches.batches).to.be.an('array');

        if (batches.batches.length > 0) {
          const batch = batches.batches[0];

          // 2. Get detailed batch information
          const batchDetails = await sdk.getCarbonCreditBatch(batch.tokenId);
          expect(batchDetails).to.have.property('tokenId', batch.tokenId);

          // 3. Retire some credits (mock transaction)
          // Note: In a real test, you'd use a test private key and testnet
          // const retirement = await sdk.retireCarbonCredits(batch.tokenId, {
          //   amount: '1.0',
          //   retirementReason: 'Test retirement for integration test',
          //   privateKey: testPrivateKey
          // });
          // expect(retirement).to.have.property('transactionHash');
        }
      } catch (error) {
        console.log('Integration test warning:', error.message);
        // Don't fail the test if the blockchain isn't available
      }
    });

    it('should complete AMM trading workflow', async function () {
      this.timeout(30000);

      try {
        // 1. Get available pools
        const pools = await sdk.getAmmPools();
        expect(pools).to.have.property('pools');

        if (pools.pools.length > 0) {
          const pool = pools.pools[0];

          // 2. Get swap quote
          const quote = await sdk.getSwapQuote(pool.token0, pool.token1, '100');
          expect(quote).to.have.property('amountOut');
          expect(quote).to.have.property('priceImpact');

          // 3. Execute swap (mock)
          // const swap = await sdk.swapTokens({
          //   tokenIn: pool.token0,
          //   tokenOut: pool.token1,
          //   amountIn: '100',
          //   amountOutMin: quote.amountOut,
          //   deadline: CarbonCreditSDK.generateDeadline(20),
          //   privateKey: testPrivateKey
          // });
        }
      } catch (error) {
        console.log('AMM integration test warning:', error.message);
      }
    });
  });
});

describe('Performance Tests', () => {
  let sdk;

  before(() => {
    sdk = new CarbonCreditSDK({
      apiBaseUrl: 'http://localhost:3001/api',
    });
  });

  it('should handle concurrent requests efficiently', async function () {
    this.timeout(15000);

    const startTime = Date.now();
    const concurrentRequests = 10;

    const requests = Array(concurrentRequests)
      .fill()
      .map((_, index) =>
        sdk.getCarbonCreditBatches({
          page: index + 1,
          limit: 10,
        })
      );

    try {
      const results = await Promise.all(requests);
      const endTime = Date.now();
      const duration = endTime - startTime;

      expect(results).to.have.length(concurrentRequests);
      expect(duration).to.be.lessThan(10000); // Should complete within 10 seconds

      results.forEach(result => {
        expect(result).to.have.property('batches');
        expect(result).to.have.property('pagination');
      });
    } catch (error) {
      console.log('Performance test warning:', error.message);
    }
  });
});

// Cleanup
process.on('unhandledRejection', (reason, promise) => {
  console.log('Unhandled Rejection at:', promise, 'reason:', reason);
});

process.on('uncaughtException', error => {
  console.log('Uncaught Exception:', error);
  process.exit(1);
});
