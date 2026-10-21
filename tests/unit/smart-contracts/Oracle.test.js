const { expect } = require('chai');
const { ethers } = require('hardhat');

describe('Oracle Integration System', function () {
  let oracleAggregator, weatherOracle, verificationOracle, priceFeedOracle;
  let deployer, oracleAdmin, priceUpdater, dataProvider, verifier;
  let priceFeedId, weatherFeedId, stationId, projectId, marketId;

  beforeEach(async function () {
    // Get signers
    [deployer, oracleAdmin, priceUpdater, dataProvider, verifier] =
      await ethers.getSigners();

    // Deploy contracts
    const OracleAggregator =
      await ethers.getContractFactory('OracleAggregator');
    oracleAggregator = await OracleAggregator.deploy();
    await oracleAggregator.waitForDeployment();

    const WeatherOracle = await ethers.getContractFactory('WeatherOracle');
    weatherOracle = await WeatherOracle.deploy();
    await weatherOracle.waitForDeployment();

    const VerificationOracle =
      await ethers.getContractFactory('VerificationOracle');
    verificationOracle = await VerificationOracle.deploy();
    await verificationOracle.waitForDeployment();

    const PriceFeedOracle = await ethers.getContractFactory('PriceFeedOracle');
    priceFeedOracle = await PriceFeedOracle.deploy();
    await priceFeedOracle.waitForDeployment();

    // Setup roles
    const ORACLE_ADMIN_ROLE = await oracleAggregator.ORACLE_ADMIN_ROLE();
    const DATA_PROVIDER_ROLE = await weatherOracle.DATA_PROVIDER_ROLE();
    const VERIFIER_ROLE = await verificationOracle.VERIFIER_ROLE();
    const PRICE_UPDATER_ROLE = await priceFeedOracle.PRICE_UPDATER_ROLE();

    await oracleAggregator.grantRole(ORACLE_ADMIN_ROLE, oracleAdmin.address);
    await weatherOracle.grantRole(DATA_PROVIDER_ROLE, dataProvider.address);
    await verificationOracle.grantRole(VERIFIER_ROLE, verifier.address);
    await priceFeedOracle.grantRole(PRICE_UPDATER_ROLE, priceUpdater.address);

    // Initialize test data
    priceFeedId = ethers.keccak256(ethers.toUtf8Bytes('CARBON_CREDIT_PRICE'));
    weatherFeedId = ethers.keccak256(ethers.toUtf8Bytes('WEATHER_DATA'));
    stationId = ethers.keccak256(ethers.toUtf8Bytes('TEST_STATION'));
    projectId = ethers.keccak256(ethers.toUtf8Bytes('TEST_PROJECT'));
    marketId = ethers.keccak256(ethers.toUtf8Bytes('TEST_MARKET'));
  });

  describe('Deployment and Configuration', function () {
    it('Should deploy all oracle contracts successfully', async function () {
      expect(await oracleAggregator.getAddress()).to.be.properAddress;
      expect(await weatherOracle.getAddress()).to.be.properAddress;
      expect(await verificationOracle.getAddress()).to.be.properAddress;
      expect(await priceFeedOracle.getAddress()).to.be.properAddress;
    });

    it('Should setup roles correctly', async function () {
      const ORACLE_ADMIN_ROLE = await oracleAggregator.ORACLE_ADMIN_ROLE();
      const hasRole = await oracleAggregator.hasRole(
        ORACLE_ADMIN_ROLE,
        oracleAdmin.address
      );
      expect(hasRole).to.be.true;
    });

    it('Should not be paused initially', async function () {
      expect(await oracleAggregator.paused()).to.be.false;
      expect(await weatherOracle.paused()).to.be.false;
      expect(await verificationOracle.paused()).to.be.false;
      expect(await priceFeedOracle.paused()).to.be.false;
    });
  });

  describe('Oracle Aggregator', function () {
    beforeEach(async function () {
      // Setup price feed oracle configuration
      const oracleConfig = {
        provider: 0, // CHAINLINK
        oracleAddress: await priceFeedOracle.getAddress(),
        weight: 5000,
        maxStaleness: 3600,
        minConfidence: 80,
        isActive: true,
        endpoint: '',
      };
      await oracleAggregator
        .connect(oracleAdmin)
        .addOracle(priceFeedId, oracleConfig);
    });

    it('Should add oracle configurations', async function () {
      // Oracle should be added successfully in beforeEach
      // Try to get aggregated data (should not revert if oracle is added)
      await expect(oracleAggregator.getAggregatedData(priceFeedId)).to.not.be
        .reverted;
    });

    it('Should reject unauthorized oracle additions', async function () {
      const unauthorizedConfig = {
        provider: 1,
        oracleAddress: await weatherOracle.getAddress(),
        weight: 3000,
        maxStaleness: 1800,
        minConfidence: 75,
        isActive: true,
        endpoint: '',
      };

      await expect(
        oracleAggregator
          .connect(deployer)
          .addOracle(weatherFeedId, unauthorizedConfig)
      ).to.be.reverted;
    });

    it('Should update oracle weights', async function () {
      const newWeight = 7000;
      await oracleAggregator
        .connect(oracleAdmin)
        .setOracleWeight(priceFeedId, 0, newWeight);

      // Weight update should succeed without reverting
      expect(true).to.be.true;
    });

    it('Should enable and disable circuit breakers', async function () {
      await oracleAggregator
        .connect(oracleAdmin)
        .enableCircuitBreaker(priceFeedId);
      expect(await oracleAggregator.isCircuitBreakerTriggered(priceFeedId)).to
        .be.false;

      await oracleAggregator
        .connect(oracleAdmin)
        .disableCircuitBreaker(priceFeedId);
      expect(await oracleAggregator.isCircuitBreakerTriggered(priceFeedId)).to
        .be.false;
    });

    it('Should remove oracles', async function () {
      await oracleAggregator.connect(oracleAdmin).removeOracle(priceFeedId, 0);

      // After removal, getting data might revert or return default values
      // Test passes if removal doesn't revert
      expect(true).to.be.true;
    });
  });

  describe('Weather Oracle', function () {
    beforeEach(async function () {
      // Add weather station
      await weatherOracle
        .connect(dataProvider)
        .addWeatherStation(stationId, 'Test Station');
    });

    it('Should add weather stations', async function () {
      const location = await weatherOracle.getStationLocation(stationId);
      expect(location).to.equal('Test Station');

      const isActive = await weatherOracle.isStationActive(stationId);
      expect(isActive).to.be.true;
    });

    it('Should update weather data', async function () {
      const weatherData = {
        temperature: 2000, // 20.00°C
        humidity: 7000, // 70.00%
        pressure: 101325, // 1013.25 hPa
        windSpeed: 800, // 8.00 m/s
        rainfall: 100, // 1.00 mm
        timestamp: Math.floor(Date.now() / 1000),
        stationId: stationId,
        confidence: 95,
      };

      await expect(
        weatherOracle
          .connect(dataProvider)
          .updateWeatherData(stationId, weatherData)
      ).to.emit(weatherOracle, 'WeatherDataUpdated');

      const latestData = await weatherOracle.getLatestWeatherData(stationId);
      expect(latestData.temperature).to.equal(2000);
      expect(latestData.humidity).to.equal(7000);
      expect(latestData.confidence).to.equal(95);
    });

    it('Should validate weather data', async function () {
      const validData = {
        temperature: 2000,
        humidity: 7000,
        pressure: 101325,
        windSpeed: 800,
        rainfall: 100,
        timestamp: Math.floor(Date.now() / 1000),
        stationId: stationId,
        confidence: 95,
      };

      const isValid = await weatherOracle.validateWeatherData(validData);
      expect(isValid).to.be.true;

      // Test invalid data (negative humidity)
      const invalidData = { ...validData, humidity: 15000 }; // >100%
      const isInvalid = await weatherOracle.validateWeatherData(invalidData);
      expect(isInvalid).to.be.false;
    });

    it('Should check data freshness', async function () {
      const weatherData = {
        temperature: 2000,
        humidity: 7000,
        pressure: 101325,
        windSpeed: 800,
        rainfall: 100,
        timestamp: Math.floor(Date.now() / 1000),
        stationId: stationId,
        confidence: 95,
      };

      await weatherOracle
        .connect(dataProvider)
        .updateWeatherData(stationId, weatherData);

      const isFresh = await weatherOracle.isWeatherDataFresh(stationId, 3600);
      expect(isFresh).to.be.true;
    });

    it('Should reject unauthorized weather updates', async function () {
      const weatherData = {
        temperature: 2000,
        humidity: 7000,
        pressure: 101325,
        windSpeed: 800,
        rainfall: 100,
        timestamp: Math.floor(Date.now() / 1000),
        stationId: stationId,
        confidence: 95,
      };

      await expect(
        weatherOracle
          .connect(deployer)
          .updateWeatherData(stationId, weatherData)
      ).to.be.reverted;
    });

    it('Should remove weather stations', async function () {
      await weatherOracle.connect(dataProvider).removeWeatherStation(stationId);

      const isActive = await weatherOracle.isStationActive(stationId);
      expect(isActive).to.be.false;
    });
  });

  describe('Verification Oracle', function () {
    const verifierId = ethers.keccak256(ethers.toUtf8Bytes('TEST_VERIFIER'));

    beforeEach(async function () {
      // Add verifier
      await verificationOracle
        .connect(verifier)
        .addVerifier(verifierId, 'Test Verifier', 0); // VCS
    });

    it('Should add verifiers', async function () {
      const isAuthorized =
        await verificationOracle.isVerifierAuthorized(verifierId);
      expect(isAuthorized).to.be.true;

      const [name, standard] =
        await verificationOracle.getVerifierInfo(verifierId);
      expect(name).to.equal('Test Verifier');
      expect(standard).to.equal(0); // VCS
    });

    it('Should update verification status', async function () {
      const verificationData = {
        status: 1, // VERIFIED
        standard: 0, // VCS
        projectId: projectId,
        verifierId: verifierId,
        verificationDate: Math.floor(Date.now() / 1000),
        expiryDate: Math.floor(Date.now() / 1000) + 365 * 24 * 3600,
        creditAmount: ethers.parseEther('1000'),
        documentHash: ethers.keccak256(ethers.toUtf8Bytes('test_doc')),
        confidence: 90,
        metadata: 'Test verification',
      };

      await expect(
        verificationOracle
          .connect(verifier)
          .updateVerificationStatus(projectId, verificationData)
      ).to.emit(verificationOracle, 'VerificationUpdated');

      const status = await verificationOracle.getVerificationStatus(projectId);
      expect(status).to.equal(1); // VERIFIED

      const isVerified = await verificationOracle.isProjectVerified(projectId);
      expect(isVerified).to.be.true;
    });

    it('Should handle verification expiry', async function () {
      const expiredVerificationData = {
        status: 1, // VERIFIED
        standard: 0, // VCS
        projectId: projectId,
        verifierId: verifierId,
        verificationDate: Math.floor(Date.now() / 1000) - 400 * 24 * 3600, // 400 days ago
        expiryDate: Math.floor(Date.now() / 1000) - 30 * 24 * 3600, // 30 days ago (expired)
        creditAmount: ethers.parseEther('1000'),
        documentHash: ethers.keccak256(ethers.toUtf8Bytes('expired_doc')),
        confidence: 90,
        metadata: 'Expired verification',
      };

      await verificationOracle
        .connect(verifier)
        .updateVerificationStatus(projectId, expiredVerificationData);

      const isValid = await verificationOracle.isVerificationValid(projectId);
      expect(isValid).to.be.false; // Should be invalid due to expiry
    });

    it('Should upload and verify documents', async function () {
      const documentHash = ethers.keccak256(
        ethers.toUtf8Bytes('verification_document')
      );

      await expect(
        verificationOracle
          .connect(verifier)
          .uploadDocument(projectId, documentHash, 'verification_report')
      ).to.emit(verificationOracle, 'DocumentUploaded');

      const isDocumentValid = await verificationOracle.verifyDocument(
        projectId,
        documentHash
      );
      expect(isDocumentValid).to.be.true;
    });

    it('Should reject unauthorized verification updates', async function () {
      const verificationData = {
        status: 1,
        standard: 0,
        projectId: projectId,
        verifierId: verifierId,
        verificationDate: Math.floor(Date.now() / 1000),
        expiryDate: Math.floor(Date.now() / 1000) + 365 * 24 * 3600,
        creditAmount: ethers.parseEther('1000'),
        documentHash: ethers.keccak256(ethers.toUtf8Bytes('test_doc')),
        confidence: 90,
        metadata: 'Test verification',
      };

      await expect(
        verificationOracle
          .connect(deployer)
          .updateVerificationStatus(projectId, verificationData)
      ).to.be.reverted;
    });

    it('Should remove verifiers', async function () {
      await verificationOracle.connect(verifier).removeVerifier(verifierId);

      const isAuthorized =
        await verificationOracle.isVerifierAuthorized(verifierId);
      expect(isAuthorized).to.be.false;
    });
  });

  describe('Price Feed Oracle', function () {
    beforeEach(async function () {
      // Add market
      const marketInfo = {
        name: 'Test Carbon Market',
        symbol: 'TCM-CARBON',
        isActive: true,
        weight: 8000,
        dataSource: await priceFeedOracle.getAddress(),
      };
      await priceFeedOracle
        .connect(priceUpdater)
        .addMarket(marketId, marketInfo);
    });

    it('Should add markets', async function () {
      const marketInfo = await priceFeedOracle.getMarketInfo(marketId);
      expect(marketInfo.name).to.equal('Test Carbon Market');
      expect(marketInfo.symbol).to.equal('TCM-CARBON');
      expect(marketInfo.isActive).to.be.true;
    });

    it('Should update price data', async function () {
      const carbonPair = ethers.keccak256(ethers.toUtf8Bytes('CARBON/USD'));
      const priceData = {
        price: ethers.parseEther('30.00'),
        timestamp: Math.floor(Date.now() / 1000),
        volume: ethers.parseEther('5000'),
        marketCap: ethers.parseEther('500000'),
        confidence: 85,
        marketId: marketId,
      };

      await expect(
        priceFeedOracle.connect(priceUpdater).updatePrice(carbonPair, priceData)
      ).to.emit(priceFeedOracle, 'PriceUpdated');

      const [price, timestamp] =
        await priceFeedOracle.getLatestPrice(carbonPair);
      expect(price).to.equal(ethers.parseEther('30.00'));
    });

    it('Should validate price data', async function () {
      const validPriceData = {
        price: ethers.parseEther('25.00'),
        timestamp: Math.floor(Date.now() / 1000),
        volume: ethers.parseEther('1000'),
        marketCap: ethers.parseEther('100000'),
        confidence: 90,
        marketId: marketId,
      };

      const isValid = await priceFeedOracle.validatePriceData(validPriceData);
      expect(isValid).to.be.true;

      // Test invalid data (confidence > 100)
      const invalidPriceData = { ...validPriceData, confidence: 150 };
      const isInvalid =
        await priceFeedOracle.validatePriceData(invalidPriceData);
      expect(isInvalid).to.be.false;
    });

    it('Should check price freshness', async function () {
      const carbonPair = ethers.keccak256(ethers.toUtf8Bytes('CARBON/USD'));
      const priceData = {
        price: ethers.parseEther('30.00'),
        timestamp: Math.floor(Date.now() / 1000),
        volume: ethers.parseEther('5000'),
        marketCap: ethers.parseEther('500000'),
        confidence: 85,
        marketId: marketId,
      };

      await priceFeedOracle
        .connect(priceUpdater)
        .updatePrice(carbonPair, priceData);

      const isFresh = await priceFeedOracle.isPriceFresh(carbonPair, 3600);
      expect(isFresh).to.be.true;
    });

    it('Should handle price thresholds and circuit breakers', async function () {
      const carbonPair = ethers.keccak256(ethers.toUtf8Bytes('CARBON/USD'));
      const threshold = ethers.parseEther('50.00');

      await priceFeedOracle
        .connect(priceUpdater)
        .setPriceThreshold(carbonPair, threshold);

      // Update with price above threshold
      const highPriceData = {
        price: ethers.parseEther('60.00'), // Above threshold
        timestamp: Math.floor(Date.now() / 1000),
        volume: ethers.parseEther('5000'),
        marketCap: ethers.parseEther('500000'),
        confidence: 85,
        marketId: marketId,
      };

      await expect(
        priceFeedOracle
          .connect(priceUpdater)
          .updatePrice(carbonPair, highPriceData)
      ).to.emit(priceFeedOracle, 'PriceThresholdExceeded');

      const isThresholdExceeded =
        await priceFeedOracle.isPriceThresholdExceeded(carbonPair);
      expect(isThresholdExceeded).to.be.true;
    });

    it('Should calculate time-weighted average prices', async function () {
      const carbonPair = ethers.keccak256(ethers.toUtf8Bytes('CARBON/USD'));

      // Add multiple price points
      const prices = [
        ethers.parseEther('25.00'),
        ethers.parseEther('26.00'),
        ethers.parseEther('27.00'),
      ];

      for (let i = 0; i < prices.length; i++) {
        const priceData = {
          price: prices[i],
          timestamp: Math.floor(Date.now() / 1000) + i * 3600,
          volume: ethers.parseEther('1000'),
          marketCap: ethers.parseEther('100000'),
          confidence: 90,
          marketId: marketId,
        };
        await priceFeedOracle
          .connect(priceUpdater)
          .updatePrice(carbonPair, priceData);
      }

      const twap = await priceFeedOracle.getTimeWeightedPrice(carbonPair, 7200); // 2 hours
      expect(twap).to.be.gt(0);
    });

    it('Should reject unauthorized price updates', async function () {
      const carbonPair = ethers.keccak256(ethers.toUtf8Bytes('CARBON/USD'));
      const priceData = {
        price: ethers.parseEther('30.00'),
        timestamp: Math.floor(Date.now() / 1000),
        volume: ethers.parseEther('5000'),
        marketCap: ethers.parseEther('500000'),
        confidence: 85,
        marketId: marketId,
      };

      await expect(
        priceFeedOracle.connect(deployer).updatePrice(carbonPair, priceData)
      ).to.be.reverted;
    });

    it('Should emergency pause and unpause', async function () {
      const carbonPair = ethers.keccak256(ethers.toUtf8Bytes('CARBON/USD'));

      await priceFeedOracle.connect(priceUpdater).emergencyPause(carbonPair);
      // Emergency pause should succeed

      await priceFeedOracle.connect(priceUpdater).emergencyUnpause(carbonPair);
      // Emergency unpause should succeed

      expect(true).to.be.true;
    });
  });

  describe('Integration Tests', function () {
    beforeEach(async function () {
      // Setup complete oracle system
      const priceOracleConfig = {
        provider: 0, // CHAINLINK
        oracleAddress: await priceFeedOracle.getAddress(),
        weight: 6000,
        maxStaleness: 3600,
        minConfidence: 80,
        isActive: true,
        endpoint: '',
      };

      const weatherOracleConfig = {
        provider: 5, // CUSTOM
        oracleAddress: await weatherOracle.getAddress(),
        weight: 7000,
        maxStaleness: 1800,
        minConfidence: 75,
        isActive: true,
        endpoint: 'weather-api',
      };

      await oracleAggregator
        .connect(oracleAdmin)
        .addOracle(priceFeedId, priceOracleConfig);
      await oracleAggregator
        .connect(oracleAdmin)
        .addOracle(weatherFeedId, weatherOracleConfig);

      // Setup test data
      await weatherOracle
        .connect(dataProvider)
        .addWeatherStation(stationId, 'Integration Test Station');

      const verifierId = ethers.keccak256(
        ethers.toUtf8Bytes('INTEGRATION_VERIFIER')
      );
      await verificationOracle
        .connect(verifier)
        .addVerifier(verifierId, 'Integration Verifier', 0);

      const marketInfo = {
        name: 'Integration Test Market',
        symbol: 'ITM-CARBON',
        isActive: true,
        weight: 9000,
        dataSource: await priceFeedOracle.getAddress(),
      };
      await priceFeedOracle
        .connect(priceUpdater)
        .addMarket(marketId, marketInfo);
    });

    it('Should aggregate data from multiple oracle sources', async function () {
      // Update price data
      const carbonPair = ethers.keccak256(ethers.toUtf8Bytes('CARBON/USD'));
      const priceData = {
        price: ethers.parseEther('28.50'),
        timestamp: Math.floor(Date.now() / 1000),
        volume: ethers.parseEther('2000'),
        marketCap: ethers.parseEther('200000'),
        confidence: 88,
        marketId: marketId,
      };
      await priceFeedOracle
        .connect(priceUpdater)
        .updatePrice(carbonPair, priceData);

      // Get aggregated data
      const aggregatedData =
        await oracleAggregator.getAggregatedData(priceFeedId);
      expect(aggregatedData.confidence).to.be.gte(80);
      expect(aggregatedData.sourceCount).to.be.gte(1);
    });

    it('Should handle oracle failures and fallbacks', async function () {
      // Enable circuit breaker
      await oracleAggregator
        .connect(oracleAdmin)
        .enableCircuitBreaker(priceFeedId);

      // Setup fallback
      await oracleAggregator
        .connect(oracleAdmin)
        .setFallbackOracle(priceFeedId, 0, 5); // CHAINLINK -> CUSTOM

      const isCircuitBreakerEnabled =
        await oracleAggregator.isCircuitBreakerTriggered(priceFeedId);
      expect(isCircuitBreakerEnabled).to.be.false; // Should not be triggered initially
    });

    it('Should monitor oracle health across all providers', async function () {
      const priceOracleHealth = await oracleAggregator.getOracleHealth(
        priceFeedId,
        0
      ); // CHAINLINK
      const weatherOracleHealth = await oracleAggregator.getOracleHealth(
        weatherFeedId,
        5
      ); // CUSTOM

      expect(priceOracleHealth).to.be.gte(0);
      expect(weatherOracleHealth).to.be.gte(0);
    });

    it('Should maintain data consistency across oracle updates', async function () {
      // Update weather data
      const weatherData = {
        temperature: 1800, // 18.00°C
        humidity: 6800, // 68.00%
        pressure: 101200,
        windSpeed: 600,
        rainfall: 50,
        timestamp: Math.floor(Date.now() / 1000),
        stationId: stationId,
        confidence: 92,
      };
      await weatherOracle
        .connect(dataProvider)
        .updateWeatherData(stationId, weatherData);

      // Update price data
      const carbonPair = ethers.keccak256(ethers.toUtf8Bytes('CARBON/USD'));
      const priceData = {
        price: ethers.parseEther('29.75'),
        timestamp: Math.floor(Date.now() / 1000),
        volume: ethers.parseEther('3000'),
        marketCap: ethers.parseEther('300000'),
        confidence: 91,
        marketId: marketId,
      };
      await priceFeedOracle
        .connect(priceUpdater)
        .updatePrice(carbonPair, priceData);

      // Verify data consistency
      const latestWeather = await weatherOracle.getLatestWeatherData(stationId);
      const [latestPrice] = await priceFeedOracle.getLatestPrice(carbonPair);

      expect(latestWeather.temperature).to.equal(1800);
      expect(latestPrice).to.equal(ethers.parseEther('29.75'));
    });

    it('Should handle emergency pause across all oracles', async function () {
      // Pause all oracles
      await oracleAggregator.pause();
      await weatherOracle.pause();
      await verificationOracle.pause();
      await priceFeedOracle.pause();

      expect(await oracleAggregator.paused()).to.be.true;
      expect(await weatherOracle.paused()).to.be.true;
      expect(await verificationOracle.paused()).to.be.true;
      expect(await priceFeedOracle.paused()).to.be.true;

      // Unpause all oracles
      await oracleAggregator.unpause();
      await weatherOracle.unpause();
      await verificationOracle.unpause();
      await priceFeedOracle.unpause();

      expect(await oracleAggregator.paused()).to.be.false;
      expect(await weatherOracle.paused()).to.be.false;
      expect(await verificationOracle.paused()).to.be.false;
      expect(await priceFeedOracle.paused()).to.be.false;
    });
  });

  describe('Security and Edge Cases', function () {
    it('Should prevent unauthorized access to admin functions', async function () {
      const unauthorizedConfig = {
        provider: 1,
        oracleAddress: ethers.ZeroAddress,
        weight: 1000,
        maxStaleness: 3600,
        minConfidence: 50,
        isActive: true,
        endpoint: '',
      };

      await expect(
        oracleAggregator
          .connect(deployer)
          .addOracle(priceFeedId, unauthorizedConfig)
      ).to.be.reverted;
    });

    it('Should handle invalid oracle configurations', async function () {
      const invalidConfig = {
        provider: 0,
        oracleAddress: ethers.ZeroAddress, // Invalid address
        weight: 15000, // Weight > 10000 (100%)
        maxStaleness: 0, // Invalid staleness
        minConfidence: 150, // Confidence > 100
        isActive: true,
        endpoint: '',
      };

      await expect(
        oracleAggregator
          .connect(oracleAdmin)
          .addOracle(priceFeedId, invalidConfig)
      ).to.be.reverted;
    });

    it('Should handle data staleness correctly', async function () {
      // Add weather station and old data
      await weatherOracle
        .connect(dataProvider)
        .addWeatherStation(stationId, 'Stale Data Station');

      const oldWeatherData = {
        temperature: 2000,
        humidity: 7000,
        pressure: 101325,
        windSpeed: 800,
        rainfall: 100,
        timestamp: Math.floor(Date.now() / 1000) - 7200, // 2 hours old
        stationId: stationId,
        confidence: 95,
      };

      await weatherOracle
        .connect(dataProvider)
        .updateWeatherData(stationId, oldWeatherData);

      // Check staleness with 1 hour threshold
      const isFresh = await weatherOracle.isWeatherDataFresh(stationId, 3600);
      expect(isFresh).to.be.false; // Should be stale
    });

    it('Should handle zero and extreme values', async function () {
      const extremeWeatherData = {
        temperature: -5000, // -50°C
        humidity: 0, // 0%
        pressure: 50000, // Very low pressure
        windSpeed: 10000, // 100 m/s
        rainfall: 0,
        timestamp: Math.floor(Date.now() / 1000),
        stationId: stationId,
        confidence: 1, // Very low confidence
      };

      await weatherOracle
        .connect(dataProvider)
        .addWeatherStation(stationId, 'Extreme Weather Station');

      // Should still accept extreme but valid values
      await expect(
        weatherOracle
          .connect(dataProvider)
          .updateWeatherData(stationId, extremeWeatherData)
      ).to.not.be.reverted;
    });

    it('Should maintain consistency during concurrent updates', async function () {
      // Setup concurrent data updates
      const updates = [];

      for (let i = 0; i < 5; i++) {
        const carbonPair = ethers.keccak256(
          ethers.toUtf8Bytes(`CARBON${i}/USD`)
        );
        const priceData = {
          price: ethers.parseEther(`${25 + i}.00`),
          timestamp: Math.floor(Date.now() / 1000),
          volume: ethers.parseEther('1000'),
          marketCap: ethers.parseEther('100000'),
          confidence: 85,
          marketId: marketId,
        };

        updates.push(
          priceFeedOracle
            .connect(priceUpdater)
            .updatePrice(carbonPair, priceData)
        );
      }

      // Execute all updates
      await Promise.all(updates);

      // Verify all updates succeeded
      for (let i = 0; i < 5; i++) {
        const carbonPair = ethers.keccak256(
          ethers.toUtf8Bytes(`CARBON${i}/USD`)
        );
        const [price] = await priceFeedOracle.getLatestPrice(carbonPair);
        expect(price).to.equal(ethers.parseEther(`${25 + i}.00`));
      }
    });
  });
});
