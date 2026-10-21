import { test, expect } from '../fixtures/wallet-fixtures';
import { TEST_CONSTANTS } from '../test-data/test-constants';
import {
  WeatherTestUtils,
  WaitUtils,
  AssertionUtils,
  APIHelper,
} from '../utils/test-helpers';
import { faker } from '@faker-js/faker';

/**
 * WRFCoin E2E Test Suite: Data Submission to Reward Calculation Flow
 *
 * This test suite covers the core value proposition - weather data submission earning rewards:
 * 1. Weather Data Submission (IoT sensor data, manual entry, batch uploads, validation)
 * 2. Reward Calculation Engine (quality-based rewards, geographic multipliers, frequency bonuses)
 * 3. Dashboard Integration (real-time updates, history visualization, analytics)
 * 4. Cross-Service Integration (compute rewards, carbon credits, staking rewards)
 */

test.describe('Data Submission to Reward Calculation Flow', () => {
  let testData: any;
  let apiHelper: APIHelper;

  test.beforeEach(async ({ page, walletPage }) => {
    // Initialize API helper and test data
    apiHelper = new APIHelper();

    testData = {
      weatherData: WeatherTestUtils.generateWeatherData(),
      iotDevice: {
        id: `test_device_${Date.now()}`,
        name: 'Test Weather Station',
        location: 'New York, NY',
        type: 'ambient_weather',
      },
      rewardSession: {
        startTime: Date.now(),
        submissionCount: 0,
        expectedRewards: 0,
      },
    };

    // Connect wallet and navigate to dashboard
    await walletPage.gotoWallet();
    await walletPage.connectWallet();
    await page.goto('/dashboard');
    await WaitUtils.waitForPageLoad(page);
  });

  test.describe('Weather Data Submission', () => {
    test('should submit IoT sensor data via Port 3002 integration', async ({
      page,
    }) => {
      await test.step('Navigate to IoT sensor integration', async () => {
        await page.click('[data-testid="nav-weather-data"]');
        await page.click('[data-testid="iot-integration-tab"]');
        await expect(
          page.locator('[data-testid="iot-dashboard"]')
        ).toBeVisible();
      });

      await test.step('Register new IoT device', async () => {
        await page.click('[data-testid="add-iot-device-button"]');
        await expect(
          page.locator('[data-testid="device-registration-modal"]')
        ).toBeVisible();

        // Fill device registration form
        await page.fill(
          '[data-testid="device-name-input"]',
          testData.iotDevice.name
        );
        await page.fill(
          '[data-testid="device-id-input"]',
          testData.iotDevice.id
        );
        await page.fill(
          '[data-testid="device-location-input"]',
          testData.iotDevice.location
        );
        await page.selectOption(
          '[data-testid="device-type-select"]',
          testData.iotDevice.type
        );

        // Configure data collection parameters
        await page.check('[data-testid="collect-temperature"]');
        await page.check('[data-testid="collect-humidity"]');
        await page.check('[data-testid="collect-pressure"]');
        await page.check('[data-testid="collect-wind"]');

        await page.selectOption('[data-testid="collection-interval"]', '300'); // 5 minutes

        await page.click('[data-testid="register-device-button"]');
        await expect(
          page.locator('[data-testid="device-registered-success"]')
        ).toBeVisible();
      });

      await test.step('Configure IoT data submission endpoint', async () => {
        const deviceCard = page.locator(
          `[data-testid="device-card-${testData.iotDevice.id}"]`
        );
        await expect(deviceCard).toBeVisible();

        await deviceCard.click();
        await page.click('[data-testid="configure-endpoint-button"]');

        // Verify API endpoint is displayed
        const apiEndpoint = await page.textContent(
          '[data-testid="api-endpoint"]'
        );
        expect(apiEndpoint).toContain(
          TEST_CONSTANTS.API_ENDPOINTS.IOT_SENSOR_NETWORKS
        );
        expect(apiEndpoint).toContain(testData.iotDevice.id);

        // Get API key for device
        await page.click('[data-testid="generate-api-key-button"]');
        const apiKey = await page.textContent('[data-testid="device-api-key"]');
        expect(apiKey).toMatch(/^[a-zA-Z0-9]{32,}$/);

        testData.iotDevice.apiKey = apiKey;
      });

      await test.step('Simulate IoT data submission to Port 3002', async () => {
        // Prepare weather data payload
        const weatherPayload = {
          deviceId: testData.iotDevice.id,
          timestamp: new Date().toISOString(),
          data: {
            temperature: testData.weatherData.temperature,
            humidity: testData.weatherData.humidity,
            pressure: testData.weatherData.pressure,
            windSpeed: testData.weatherData.windSpeed,
          },
          location: testData.weatherData.location,
          quality: {
            accuracy: 0.95,
            completeness: 1.0,
            timeliness: 1.0,
          },
        };

        // Submit data via API (simulating IoT device)
        const response = await fetch(
          `${TEST_CONSTANTS.API_ENDPOINTS.IOT_SENSOR_NETWORKS}/data/submit`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              'X-API-Key': testData.iotDevice.apiKey,
            },
            body: JSON.stringify(weatherPayload),
          }
        );

        expect(response.ok).toBeTruthy();
        const result = await response.json();
        expect(result.success).toBe(true);
        expect(result.submissionId).toBeTruthy();

        testData.submissionId = result.submissionId;
      });

      await test.step('Verify data appears in real-time dashboard', async () => {
        await page.reload();
        await page.waitForTimeout(2000); // Allow for data processing

        // Check device shows active status
        const deviceCard = page.locator(
          `[data-testid="device-card-${testData.iotDevice.id}"]`
        );
        await expect(
          deviceCard.locator('[data-testid="device-status"]')
        ).toContainText('Active');
        await expect(
          deviceCard.locator('[data-testid="last-reading"]')
        ).toBeVisible();

        // Verify latest data readings
        const latestTemp = await deviceCard
          .locator('[data-testid="latest-temperature"]')
          .textContent();
        expect(latestTemp).toContain(
          testData.weatherData.temperature.toString()
        );
      });

      await test.step('Verify data quality assessment', async () => {
        await page.click(
          `[data-testid="device-card-${testData.iotDevice.id}"]`
        );
        await page.click('[data-testid="data-quality-tab"]');

        await expect(
          page.locator('[data-testid="quality-score"]')
        ).toBeVisible();
        const qualityScore = await page.textContent(
          '[data-testid="quality-score"]'
        );
        expect(parseFloat(qualityScore!.split('%')[0])).toBeGreaterThan(90); // High quality data

        // Verify quality components
        await expect(
          page.locator('[data-testid="accuracy-score"]')
        ).toContainText('95%');
        await expect(
          page.locator('[data-testid="completeness-score"]')
        ).toContainText('100%');
        await expect(
          page.locator('[data-testid="timeliness-score"]')
        ).toContainText('100%');
      });
    });

    test('should handle manual weather data entry via dashboard', async ({
      page,
    }) => {
      await test.step('Navigate to manual data entry', async () => {
        await page.click('[data-testid="nav-weather-data"]');
        await page.click('[data-testid="manual-entry-tab"]');
        await expect(
          page.locator('[data-testid="manual-entry-form"]')
        ).toBeVisible();
      });

      await test.step('Fill weather data entry form', async () => {
        const manualData = WeatherTestUtils.generateWeatherData();

        await page.fill(
          '[data-testid="temperature-input"]',
          manualData.temperature.toString()
        );
        await page.fill(
          '[data-testid="humidity-input"]',
          manualData.humidity.toString()
        );
        await page.fill(
          '[data-testid="pressure-input"]',
          manualData.pressure.toString()
        );
        await page.fill(
          '[data-testid="wind-speed-input"]',
          manualData.windSpeed.toString()
        );

        // Set location
        await page.fill(
          '[data-testid="location-input"]',
          manualData.location.name
        );
        await page.fill(
          '[data-testid="latitude-input"]',
          manualData.location.lat.toString()
        );
        await page.fill(
          '[data-testid="longitude-input"]',
          manualData.location.lon.toString()
        );

        // Add observation notes
        await page.fill(
          '[data-testid="observation-notes"]',
          'Manual weather observation from rooftop station'
        );

        testData.manualEntry = manualData;
      });

      await test.step('Validate and submit manual entry', async () => {
        // Form should validate inputs
        await page.click('[data-testid="validate-data-button"]');
        await expect(
          page.locator('[data-testid="validation-success"]')
        ).toBeVisible();

        // Submit data
        await page.click('[data-testid="submit-manual-data-button"]');
        await expect(
          page.locator('[data-testid="manual-submission-success"]')
        ).toBeVisible();

        const submissionId = await page.textContent(
          '[data-testid="submission-id"]'
        );
        expect(submissionId).toBeTruthy();
        testData.manualSubmissionId = submissionId;
      });

      await test.step('Verify manual entry appears in submissions list', async () => {
        await page.click('[data-testid="my-submissions-tab"]');

        const submissionRow = page.locator(
          `[data-testid="submission-${testData.manualSubmissionId}"]`
        );
        await expect(submissionRow).toBeVisible();

        await expect(
          submissionRow.locator('[data-testid="submission-type"]')
        ).toContainText('Manual');
        await expect(
          submissionRow.locator('[data-testid="submission-status"]')
        ).toContainText('Processing');
      });

      await test.step('Check data quality scoring for manual entry', async () => {
        // Manual entries typically have lower automatic quality scores
        const submissionRow = page.locator(
          `[data-testid="submission-${testData.manualSubmissionId}"]`
        );
        await submissionRow.click();

        await expect(
          page.locator('[data-testid="manual-entry-quality"]')
        ).toBeVisible();
        const qualityScore = await page.textContent(
          '[data-testid="quality-score"]'
        );
        expect(parseFloat(qualityScore!.split('%')[0])).toBeGreaterThan(60); // Reasonable for manual entry
      });
    });

    test('should process batch weather data uploads', async ({ page }) => {
      await test.step('Navigate to batch upload section', async () => {
        await page.click('[data-testid="nav-weather-data"]');
        await page.click('[data-testid="batch-upload-tab"]');
        await expect(
          page.locator('[data-testid="batch-upload-interface"]')
        ).toBeVisible();
      });

      await test.step('Prepare batch data file', async () => {
        // Generate CSV data for batch upload
        const batchData = [];
        const header =
          'timestamp,temperature,humidity,pressure,wind_speed,latitude,longitude,location';
        batchData.push(header);

        // Generate 24 hours of data (hourly intervals)
        for (let i = 0; i < 24; i++) {
          const timestamp = new Date(Date.now() - i * 60 * 60 * 1000);
          const weatherPoint = WeatherTestUtils.generateWeatherData();

          const row = [
            timestamp.toISOString(),
            weatherPoint.temperature,
            weatherPoint.humidity,
            weatherPoint.pressure,
            weatherPoint.windSpeed,
            weatherPoint.location.lat,
            weatherPoint.location.lon,
            weatherPoint.location.name,
          ].join(',');

          batchData.push(row);
        }

        testData.batchCsvContent = batchData.join('\n');
      });

      await test.step('Upload batch data file', async () => {
        // Create a temporary file for upload
        const file = new File([testData.batchCsvContent], 'weather_batch.csv', {
          type: 'text/csv',
        });

        // Set up file input
        const fileInput = page.locator('[data-testid="batch-file-input"]');
        await fileInput.setInputFiles([
          {
            name: 'weather_batch.csv',
            mimeType: 'text/csv',
            buffer: Buffer.from(testData.batchCsvContent),
          },
        ]);

        // Verify file validation
        await expect(
          page.locator('[data-testid="file-validation-success"]')
        ).toBeVisible();

        const recordCount = await page.textContent(
          '[data-testid="record-count"]'
        );
        expect(recordCount).toContain('24'); // 24 data points
      });

      await test.step('Configure batch upload parameters', async () => {
        await page.selectOption(
          '[data-testid="data-source-select"]',
          'personal_station'
        );
        await page.fill(
          '[data-testid="data-source-description"]',
          'Historical data from personal weather station'
        );

        await page.selectOption(
          '[data-testid="time-zone-select"]',
          'America/New_York'
        );
        await page.check('[data-testid="validate-coordinates"]');
        await page.check('[data-testid="detect-duplicates"]');
      });

      await test.step('Process batch upload', async () => {
        await page.click('[data-testid="start-batch-upload-button"]');

        // Monitor upload progress
        await expect(
          page.locator('[data-testid="upload-progress"]')
        ).toBeVisible();

        // Wait for processing completion
        await page.waitForFunction(
          () => {
            const progress = document.querySelector(
              '[data-testid="upload-progress"]'
            );
            return progress?.textContent?.includes('100%');
          },
          { timeout: 60000 }
        );

        await expect(
          page.locator('[data-testid="batch-upload-complete"]')
        ).toBeVisible();

        const batchId = await page.textContent('[data-testid="batch-id"]');
        expect(batchId).toBeTruthy();
        testData.batchId = batchId;
      });

      await test.step('Verify batch processing results', async () => {
        const processingResults = page.locator(
          '[data-testid="processing-summary"]'
        );
        await expect(processingResults).toBeVisible();

        const processedCount = await page.textContent(
          '[data-testid="processed-records"]'
        );
        const rejectedCount = await page.textContent(
          '[data-testid="rejected-records"]'
        );

        expect(parseInt(processedCount!)).toBeGreaterThan(20); // Most records should be processed
        expect(parseInt(rejectedCount!)).toBeLessThan(4); // Minimal rejections

        // Check for any data quality issues
        if (
          await page.locator('[data-testid="quality-warnings"]').isVisible()
        ) {
          const warnings = await page
            .locator('[data-testid="quality-warning-item"]')
            .count();
          expect(warnings).toBeLessThan(5); // Acceptable number of quality warnings
        }
      });
    });

    test('should validate data format and quality requirements', async ({
      page,
    }) => {
      await test.step('Test data format validation', async () => {
        await page.click('[data-testid="nav-weather-data"]');
        await page.click('[data-testid="manual-entry-tab"]');

        // Test temperature range validation
        await page.fill('[data-testid="temperature-input"]', '200'); // Unrealistic temperature
        await page.click('[data-testid="validate-data-button"]');

        await expect(
          page.locator('[data-testid="temperature-range-error"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="error-message"]')
        ).toContainText('temperature out of valid range');

        // Correct the temperature
        await page.fill('[data-testid="temperature-input"]', '25.5');

        // Test humidity validation
        await page.fill('[data-testid="humidity-input"]', '150'); // Invalid humidity > 100%
        await page.click('[data-testid="validate-data-button"]');

        await expect(
          page.locator('[data-testid="humidity-range-error"]')
        ).toBeVisible();

        // Correct humidity
        await page.fill('[data-testid="humidity-input"]', '65');
      });

      await test.step('Test coordinate validation', async () => {
        // Invalid latitude (> 90)
        await page.fill('[data-testid="latitude-input"]', '95.0');
        await page.click('[data-testid="validate-data-button"]');

        await expect(
          page.locator('[data-testid="coordinate-validation-error"]')
        ).toBeVisible();

        // Correct coordinates
        await page.fill('[data-testid="latitude-input"]', '40.7128');
        await page.fill('[data-testid="longitude-input"]', '-74.0060');
      });

      await test.step('Test data completeness validation', async () => {
        // Clear required field
        await page.fill('[data-testid="temperature-input"]', '');
        await page.click('[data-testid="validate-data-button"]');

        await expect(
          page.locator('[data-testid="required-field-error"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="error-message"]')
        ).toContainText('Temperature is required');

        // Fill all required fields
        await page.fill('[data-testid="temperature-input"]', '25.5');
        await page.fill('[data-testid="pressure-input"]', '1013.25');

        await page.click('[data-testid="validate-data-button"]');
        await expect(
          page.locator('[data-testid="validation-success"]')
        ).toBeVisible();
      });
    });
  });

  test.describe('Reward Calculation Engine', () => {
    test.beforeEach(async ({ page }) => {
      // Submit some test data to have baseline for reward calculations
      await page.goto('/dashboard/weather-data');

      // Quick IoT submission for reward calculation
      const quickData = WeatherTestUtils.generateWeatherData();
      await fetch(
        `${TEST_CONSTANTS.API_ENDPOINTS.IOT_SENSOR_NETWORKS}/data/submit`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'X-API-Key': 'test-api-key',
          },
          body: JSON.stringify({
            deviceId: testData.iotDevice.id,
            timestamp: new Date().toISOString(),
            data: quickData,
            quality: { accuracy: 0.9, completeness: 1.0, timeliness: 1.0 },
          }),
        }
      );

      testData.rewardSession.submissionCount++;
    });

    test('should calculate rewards based on data quality', async ({ page }) => {
      await test.step('Navigate to rewards dashboard', async () => {
        await page.click('[data-testid="nav-rewards"]');
        await expect(
          page.locator('[data-testid="rewards-dashboard"]')
        ).toBeVisible();
      });

      await test.step('Verify reward calculation components', async () => {
        await expect(
          page.locator('[data-testid="total-rewards-earned"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="data-quality-multiplier"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="frequency-bonus"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="geographic-bonus"]')
        ).toBeVisible();

        const totalRewards = await page.textContent(
          '[data-testid="total-rewards-earned"]'
        );
        expect(totalRewards).toMatch(/^\d+(\.\d+)?\s*(WRF|points)$/);

        testData.currentRewards = parseFloat(totalRewards!.split(' ')[0]);
      });

      await test.step('Submit high-quality data and verify reward increase', async () => {
        const initialRewards = testData.currentRewards;

        // Submit high-quality data
        const highQualityData = WeatherTestUtils.generateWeatherData();
        await fetch(
          `${TEST_CONSTANTS.API_ENDPOINTS.IOT_SENSOR_NETWORKS}/data/submit`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              'X-API-Key': 'test-api-key',
            },
            body: JSON.stringify({
              deviceId: testData.iotDevice.id,
              timestamp: new Date().toISOString(),
              data: highQualityData,
              quality: { accuracy: 0.98, completeness: 1.0, timeliness: 1.0 }, // Very high quality
            }),
          }
        );

        // Wait for reward processing
        await page.waitForTimeout(5000);
        await page.reload();

        const newTotalRewards = await page.textContent(
          '[data-testid="total-rewards-earned"]'
        );
        const newRewardAmount = parseFloat(newTotalRewards!.split(' ')[0]);

        expect(newRewardAmount).toBeGreaterThan(initialRewards);

        // Check quality bonus application
        const qualityBonus = await page.textContent(
          '[data-testid="quality-bonus-amount"]'
        );
        expect(parseFloat(qualityBonus!)).toBeGreaterThan(0);
      });

      await test.step('Submit lower-quality data and verify reduced rewards', async () => {
        const beforeLowQuality = parseFloat(
          (await page.textContent(
            '[data-testid="total-rewards-earned"]'
          ))!.split(' ')[0]
        );

        // Submit lower quality data
        const lowQualityData = WeatherTestUtils.generateWeatherData();
        await fetch(
          `${TEST_CONSTANTS.API_ENDPOINTS.IOT_SENSOR_NETWORKS}/data/submit`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              'X-API-Key': 'test-api-key',
            },
            body: JSON.stringify({
              deviceId: testData.iotDevice.id,
              timestamp: new Date().toISOString(),
              data: lowQualityData,
              quality: { accuracy: 0.7, completeness: 0.8, timeliness: 0.9 }, // Lower quality
            }),
          }
        );

        await page.waitForTimeout(5000);
        await page.reload();

        const afterLowQuality = parseFloat(
          (await page.textContent(
            '[data-testid="total-rewards-earned"]'
          ))!.split(' ')[0]
        );

        // Reward increase should be smaller for lower quality data
        const rewardIncrease = afterLowQuality - beforeLowQuality;
        expect(rewardIncrease).toBeGreaterThan(0); // Still some reward
        expect(rewardIncrease).toBeLessThan(1.0); // But significantly less
      });
    });

    test('should apply geographic region multipliers', async ({ page }) => {
      await test.step('Test rewards for data-sparse regions', async () => {
        await page.click('[data-testid="nav-rewards"]');

        // Submit data from a remote/sparse region (should get higher multiplier)
        const remoteLocationData = {
          ...WeatherTestUtils.generateWeatherData(),
          location: {
            lat: 71.0489, // Northern Alaska (data-sparse region)
            lon: -156.7889,
            name: 'Utqiagvik, Alaska',
          },
        };

        await fetch(
          `${TEST_CONSTANTS.API_ENDPOINTS.IOT_SENSOR_NETWORKS}/data/submit`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              'X-API-Key': 'test-api-key',
            },
            body: JSON.stringify({
              deviceId: testData.iotDevice.id,
              timestamp: new Date().toISOString(),
              data: remoteLocationData,
              location: remoteLocationData.location,
              quality: { accuracy: 0.9, completeness: 1.0, timeliness: 1.0 },
            }),
          }
        );

        await page.waitForTimeout(5000);
        await page.reload();

        // Check geographic multiplier
        const geoMultiplier = await page.textContent(
          '[data-testid="geographic-multiplier"]'
        );
        expect(parseFloat(geoMultiplier!)).toBeGreaterThan(1.5); // High multiplier for sparse region

        const geoBonus = await page.textContent(
          '[data-testid="geographic-bonus-amount"]'
        );
        expect(parseFloat(geoBonus!)).toBeGreaterThan(0);
      });

      await test.step('Test rewards for data-dense regions', async () => {
        // Submit data from a dense region (should get standard multiplier)
        const denseLocationData = {
          ...WeatherTestUtils.generateWeatherData(),
          location: {
            lat: 40.7128, // New York City (data-dense region)
            lon: -74.006,
            name: 'New York, NY',
          },
        };

        await fetch(
          `${TEST_CONSTANTS.API_ENDPOINTS.IOT_SENSOR_NETWORKS}/data/submit`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              'X-API-Key': 'test-api-key',
            },
            body: JSON.stringify({
              deviceId: testData.iotDevice.id,
              timestamp: new Date().toISOString(),
              data: denseLocationData,
              location: denseLocationData.location,
              quality: { accuracy: 0.9, completeness: 1.0, timeliness: 1.0 },
            }),
          }
        );

        await page.waitForTimeout(5000);
        await page.reload();

        // Geographic multiplier should be lower for dense regions
        const geoMultiplier = await page.textContent(
          '[data-testid="geographic-multiplier"]'
        );
        expect(parseFloat(geoMultiplier!)).toBeLessThan(1.2); // Lower multiplier for dense region
      });
    });

    test('should calculate data frequency bonuses', async ({ page }) => {
      await test.step('Submit regular frequency data and track bonuses', async () => {
        await page.click('[data-testid="nav-rewards"]');
        const initialRewards = parseFloat(
          (await page.textContent(
            '[data-testid="total-rewards-earned"]'
          ))!.split(' ')[0]
        );

        // Submit data at regular 5-minute intervals (simulated)
        const regularSubmissions = 12; // 1 hour of 5-minute intervals

        for (let i = 0; i < regularSubmissions; i++) {
          const timestamp = new Date(Date.now() - i * 5 * 60 * 1000); // 5 minutes apart
          const weatherData = WeatherTestUtils.generateWeatherData();

          await fetch(
            `${TEST_CONSTANTS.API_ENDPOINTS.IOT_SENSOR_NETWORKS}/data/submit`,
            {
              method: 'POST',
              headers: {
                'Content-Type': 'application/json',
                'X-API-Key': 'test-api-key',
              },
              body: JSON.stringify({
                deviceId: testData.iotDevice.id,
                timestamp: timestamp.toISOString(),
                data: weatherData,
                quality: { accuracy: 0.9, completeness: 1.0, timeliness: 1.0 },
              }),
            }
          );

          await page.waitForTimeout(100); // Brief delay between submissions
        }

        await page.waitForTimeout(10000); // Allow processing time
        await page.reload();

        // Check frequency bonus
        const frequencyBonus = await page.textContent(
          '[data-testid="frequency-bonus-amount"]'
        );
        expect(parseFloat(frequencyBonus!)).toBeGreaterThan(0);

        const totalRewards = parseFloat(
          (await page.textContent(
            '[data-testid="total-rewards-earned"]'
          ))!.split(' ')[0]
        );
        expect(totalRewards).toBeGreaterThan(initialRewards + 5); // Significant increase
      });

      await test.step('Verify consistency bonus for long-term data providers', async () => {
        // Check consistency metrics
        await page.click('[data-testid="reward-details-tab"]');

        await expect(
          page.locator('[data-testid="consistency-score"]')
        ).toBeVisible();
        const consistencyScore = await page.textContent(
          '[data-testid="consistency-score"]'
        );
        expect(parseFloat(consistencyScore!.split('%')[0])).toBeGreaterThan(80);

        await expect(
          page.locator('[data-testid="uptime-percentage"]')
        ).toBeVisible();
        const uptimePercentage = await page.textContent(
          '[data-testid="uptime-percentage"]'
        );
        expect(parseFloat(uptimePercentage!.split('%')[0])).toBeGreaterThan(90);
      });
    });

    test('should calculate accuracy verification rewards', async ({ page }) => {
      await test.step('Submit data that gets verified by other sources', async () => {
        await page.click('[data-testid="nav-rewards"]');

        // Submit weather data
        const verifiableData = WeatherTestUtils.generateWeatherData();
        await fetch(
          `${TEST_CONSTANTS.API_ENDPOINTS.IOT_SENSOR_NETWORKS}/data/submit`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              'X-API-Key': 'test-api-key',
            },
            body: JSON.stringify({
              deviceId: testData.iotDevice.id,
              timestamp: new Date().toISOString(),
              data: verifiableData,
              quality: { accuracy: 0.95, completeness: 1.0, timeliness: 1.0 },
            }),
          }
        );

        // Simulate verification by submitting similar data from another "source"
        await fetch(
          `${TEST_CONSTANTS.API_ENDPOINTS.IOT_SENSOR_NETWORKS}/data/submit`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              'X-API-Key': 'verification-source-key',
            },
            body: JSON.stringify({
              deviceId: 'verification_device',
              timestamp: new Date().toISOString(),
              data: {
                temperature: verifiableData.temperature + 0.1, // Very close values
                humidity: verifiableData.humidity - 1,
                pressure: verifiableData.pressure + 0.5,
                windSpeed: verifiableData.windSpeed + 0.2,
              },
              location: verifiableData.location,
              quality: { accuracy: 0.93, completeness: 1.0, timeliness: 1.0 },
            }),
          }
        );

        await page.waitForTimeout(10000); // Allow cross-verification processing
        await page.reload();

        // Check verification bonus
        if (
          await page
            .locator('[data-testid="verification-bonus-amount"]')
            .isVisible()
        ) {
          const verificationBonus = await page.textContent(
            '[data-testid="verification-bonus-amount"]'
          );
          expect(parseFloat(verificationBonus!)).toBeGreaterThan(0);
        }
      });
    });
  });

  test.describe('Dashboard Integration', () => {
    test('should display real-time reward balance updates', async ({
      page,
    }) => {
      await test.step('Navigate to rewards dashboard', async () => {
        await page.click('[data-testid="nav-rewards"]');
        await expect(
          page.locator('[data-testid="rewards-dashboard"]')
        ).toBeVisible();
      });

      await test.step('Monitor real-time balance updates', async () => {
        const initialBalance = await page.textContent(
          '[data-testid="total-rewards-earned"]'
        );
        const initialAmount = parseFloat(initialBalance!.split(' ')[0]);

        // Submit new data
        const newData = WeatherTestUtils.generateWeatherData();
        await fetch(
          `${TEST_CONSTANTS.API_ENDPOINTS.IOT_SENSOR_NETWORKS}/data/submit`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              'X-API-Key': 'test-api-key',
            },
            body: JSON.stringify({
              deviceId: testData.iotDevice.id,
              timestamp: new Date().toISOString(),
              data: newData,
              quality: { accuracy: 0.9, completeness: 1.0, timeliness: 1.0 },
            }),
          }
        );

        // Wait for real-time update (should update without page refresh)
        await page.waitForFunction(
          () => {
            const balanceElement = document.querySelector(
              '[data-testid="total-rewards-earned"]'
            );
            const currentBalance = balanceElement
              ? parseFloat(balanceElement.textContent!.split(' ')[0])
              : 0;
            return currentBalance > initialAmount;
          },
          { timeout: 30000 }
        );

        const updatedBalance = await page.textContent(
          '[data-testid="total-rewards-earned"]'
        );
        const updatedAmount = parseFloat(updatedBalance!.split(' ')[0]);

        expect(updatedAmount).toBeGreaterThan(initialAmount);
      });

      await test.step('Verify reward animation and notifications', async () => {
        // Check for reward notification
        if (
          await page
            .locator('[data-testid="reward-earned-notification"]')
            .isVisible()
        ) {
          await expect(
            page.locator('[data-testid="reward-earned-notification"]')
          ).toContainText('earned');

          // Notification should auto-hide
          await page.waitForTimeout(5000);
          await expect(
            page.locator('[data-testid="reward-earned-notification"]')
          ).not.toBeVisible();
        }
      });
    });

    test('should display comprehensive reward history visualization', async ({
      page,
    }) => {
      await test.step('Navigate to reward history', async () => {
        await page.click('[data-testid="nav-rewards"]');
        await page.click('[data-testid="reward-history-tab"]');
        await expect(
          page.locator('[data-testid="reward-history-chart"]')
        ).toBeVisible();
      });

      await test.step('Verify chart data and interactions', async () => {
        // Test different time ranges
        await page.click('[data-testid="chart-range-7d"]');
        await page.waitForTimeout(1000);
        await expect(
          page.locator('[data-testid="chart-data-points"]')
        ).toBeVisible();

        await page.click('[data-testid="chart-range-30d"]');
        await page.waitForTimeout(1000);

        await page.click('[data-testid="chart-range-90d"]');
        await page.waitForTimeout(1000);

        // Test chart interaction
        const chartArea = page.locator('[data-testid="reward-history-chart"]');
        await chartArea.hover();

        if (await page.locator('[data-testid="chart-tooltip"]').isVisible()) {
          const tooltip = page.locator('[data-testid="chart-tooltip"]');
          await expect(
            tooltip.locator('[data-testid="tooltip-date"]')
          ).toBeVisible();
          await expect(
            tooltip.locator('[data-testid="tooltip-reward-amount"]')
          ).toBeVisible();
        }
      });

      await test.step('Verify reward breakdown visualization', async () => {
        await page.click('[data-testid="reward-breakdown-tab"]');

        // Check different reward components
        await expect(
          page.locator('[data-testid="base-rewards-chart"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="quality-bonus-chart"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="frequency-bonus-chart"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="geographic-bonus-chart"]')
        ).toBeVisible();

        // Test pie chart showing reward composition
        const pieChart = page.locator('[data-testid="reward-composition-pie"]');
        await expect(pieChart).toBeVisible();

        // Verify legend
        await expect(
          page.locator('[data-testid="chart-legend"]')
        ).toBeVisible();
        const legendItems = page.locator('[data-testid="legend-item"]');
        expect(await legendItems.count()).toBeGreaterThan(2);
      });

      await test.step('Test reward history filtering and search', async () => {
        await page.click('[data-testid="reward-history-table-tab"]');

        // Filter by reward type
        await page.selectOption(
          '[data-testid="reward-type-filter"]',
          'quality-bonus'
        );
        await page.waitForTimeout(1000);

        const filteredRows = page.locator('[data-testid="reward-history-row"]');
        if ((await filteredRows.count()) > 0) {
          const firstRow = filteredRows.first();
          await expect(
            firstRow.locator('[data-testid="reward-type"]')
          ).toContainText('Quality');
        }

        // Clear filter
        await page.selectOption('[data-testid="reward-type-filter"]', 'all');

        // Test date range filter
        await page.fill('[data-testid="start-date-filter"]', '2024-01-01');
        await page.fill('[data-testid="end-date-filter"]', '2024-12-31');
        await page.click('[data-testid="apply-date-filter"]');

        await page.waitForTimeout(2000);
        expect(
          await page.locator('[data-testid="reward-history-row"]').count()
        ).toBeGreaterThan(0);
      });
    });

    test('should show earnings analytics and trends', async ({ page }) => {
      await test.step('Navigate to analytics dashboard', async () => {
        await page.click('[data-testid="nav-rewards"]');
        await page.click('[data-testid="analytics-tab"]');
        await expect(
          page.locator('[data-testid="analytics-dashboard"]')
        ).toBeVisible();
      });

      await test.step('Verify key performance metrics', async () => {
        // Check KPI cards
        await expect(
          page.locator('[data-testid="total-lifetime-rewards"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="average-daily-rewards"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="data-submissions-count"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="average-data-quality"]')
        ).toBeVisible();

        // Verify values are realistic
        const lifetimeRewards = await page.textContent(
          '[data-testid="total-lifetime-rewards"]'
        );
        expect(
          parseFloat(lifetimeRewards!.split(' ')[0])
        ).toBeGreaterThanOrEqual(0);

        const avgQuality = await page.textContent(
          '[data-testid="average-data-quality"]'
        );
        expect(parseFloat(avgQuality!.split('%')[0])).toBeGreaterThan(0);
        expect(parseFloat(avgQuality!.split('%')[0])).toBeLessThanOrEqual(100);
      });

      await test.step('Verify trend analysis', async () => {
        // Check trend indicators
        await expect(
          page.locator('[data-testid="rewards-trend-indicator"]')
        ).toBeVisible();
        const trendIndicator = page.locator(
          '[data-testid="rewards-trend-indicator"]'
        );

        const trendText = await trendIndicator.textContent();
        expect(trendText).toMatch(/increase|decrease|stable/i);

        // Check trend percentage
        if (
          await page.locator('[data-testid="trend-percentage"]').isVisible()
        ) {
          const trendPercentage = await page.textContent(
            '[data-testid="trend-percentage"]'
          );
          expect(trendPercentage).toMatch(/^\d+(\.\d+)?%$/);
        }
      });

      await test.step('Test predictive analytics', async () => {
        if (
          await page.locator('[data-testid="reward-projection"]').isVisible()
        ) {
          // Check projected earnings
          await expect(
            page.locator('[data-testid="projected-monthly-rewards"]')
          ).toBeVisible();
          await expect(
            page.locator('[data-testid="projected-annual-rewards"]')
          ).toBeVisible();

          const monthlyProjection = await page.textContent(
            '[data-testid="projected-monthly-rewards"]'
          );
          expect(parseFloat(monthlyProjection!.split(' ')[0])).toBeGreaterThan(
            0
          );

          // Check projection methodology
          if (
            await page
              .locator('[data-testid="projection-methodology"]')
              .isVisible()
          ) {
            await page.click('[data-testid="projection-methodology"]');
            await expect(
              page.locator('[data-testid="methodology-explanation"]')
            ).toBeVisible();
          }
        }
      });
    });

    test('should provide reward withdrawal functionality', async ({
      page,
      walletPage,
    }) => {
      await test.step('Navigate to withdrawal section', async () => {
        await page.click('[data-testid="nav-rewards"]');
        await page.click('[data-testid="withdraw-rewards-button"]');
        await expect(
          page.locator('[data-testid="withdrawal-modal"]')
        ).toBeVisible();
      });

      await test.step('Check available balance for withdrawal', async () => {
        const availableBalance = await page.textContent(
          '[data-testid="available-withdrawal-balance"]'
        );
        expect(availableBalance).toBeTruthy();

        const balanceAmount = parseFloat(availableBalance!.split(' ')[0]);
        testData.availableWithdrawal = balanceAmount;

        // Check minimum withdrawal amount
        if (
          await page.locator('[data-testid="minimum-withdrawal"]').isVisible()
        ) {
          const minWithdrawal = await page.textContent(
            '[data-testid="minimum-withdrawal"]'
          );
          expect(parseFloat(minWithdrawal!.split(' ')[0])).toBeGreaterThan(0);
        }
      });

      await test.step('Configure withdrawal parameters', async () => {
        if (testData.availableWithdrawal > 0) {
          const withdrawalAmount = Math.min(
            testData.availableWithdrawal,
            10
          ).toString();

          await page.fill(
            '[data-testid="withdrawal-amount-input"]',
            withdrawalAmount
          );
          await page.selectOption(
            '[data-testid="withdrawal-token-select"]',
            'WRF'
          );

          // Check withdrawal fee calculation
          if (
            await page.locator('[data-testid="withdrawal-fee"]').isVisible()
          ) {
            const withdrawalFee = await page.textContent(
              '[data-testid="withdrawal-fee"]'
            );
            expect(
              parseFloat(withdrawalFee!.split(' ')[0])
            ).toBeGreaterThanOrEqual(0);
          }

          // Verify net amount calculation
          const netAmount = await page.textContent(
            '[data-testid="net-withdrawal-amount"]'
          );
          expect(parseFloat(netAmount!.split(' ')[0])).toBeGreaterThan(0);
        }
      });

      await test.step('Process withdrawal transaction', async () => {
        if (testData.availableWithdrawal > 0) {
          await page.click('[data-testid="initiate-withdrawal-button"]');

          // Confirm withdrawal in confirmation modal
          await expect(
            page.locator('[data-testid="withdrawal-confirmation"]')
          ).toBeVisible();
          await page.click('[data-testid="confirm-withdrawal-button"]');

          // Wait for transaction
          await WaitUtils.waitForTransaction(page);

          await expect(
            page.locator('[data-testid="withdrawal-success"]')
          ).toBeVisible();
          const txHash = await page.textContent(
            '[data-testid="withdrawal-tx-hash"]'
          );
          expect(txHash).toMatch(/^0x[a-fA-F0-9]{64}$/);

          testData.withdrawalTxHash = txHash;
        }
      });

      await test.step('Verify withdrawal in transaction history', async () => {
        if (testData.withdrawalTxHash) {
          await page.click('[data-testid="close-withdrawal-modal"]');

          // Navigate to wallet and check transaction history
          await page.click('[data-testid="nav-wallet"]');

          const transactions = await walletPage.getTransactionHistory();
          const withdrawalTx = transactions.find(
            tx => tx.hash === testData.withdrawalTxHash
          );

          expect(withdrawalTx).toBeTruthy();
          expect(withdrawalTx?.status).toMatch(/pending|confirmed/);
        }
      });
    });
  });

  test.describe('Cross-Service Integration', () => {
    test('should integrate with Compute Networks reward distribution (Port 6000)', async ({
      page,
    }) => {
      await test.step('Navigate to compute networks integration', async () => {
        await page.click('[data-testid="nav-compute"]');
        await expect(
          page.locator('[data-testid="compute-dashboard"]')
        ).toBeVisible();
      });

      await test.step('Submit data for compute processing rewards', async () => {
        // Submit weather data that requires computational processing
        const computeData = WeatherTestUtils.generateWeatherData();
        await fetch(
          `${TEST_CONSTANTS.API_ENDPOINTS.COMPUTE_NETWORKS}/jobs/weather-processing`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              'X-API-Key': 'test-compute-key',
            },
            body: JSON.stringify({
              dataId: testData.submissionId,
              processingType: 'predictive-analysis',
              weatherData: computeData,
              computeRequirements: {
                cpu: 2,
                memory: '4GB',
                estimatedTime: 300, // 5 minutes
              },
            }),
          }
        );

        await page.waitForTimeout(5000);
        await page.reload();

        // Check compute job status
        await expect(
          page.locator('[data-testid="active-compute-jobs"]')
        ).toBeVisible();

        const jobsList = page.locator('[data-testid="compute-job-item"]');
        if ((await jobsList.count()) > 0) {
          const firstJob = jobsList.first();
          await expect(
            firstJob.locator('[data-testid="job-status"]')
          ).toBeVisible();
          await expect(
            firstJob.locator('[data-testid="estimated-reward"]')
          ).toBeVisible();
        }
      });

      await test.step('Verify compute rewards in rewards dashboard', async () => {
        await page.click('[data-testid="nav-rewards"]');
        await page.reload();

        if (
          await page
            .locator('[data-testid="compute-rewards-section"]')
            .isVisible()
        ) {
          await expect(
            page.locator('[data-testid="compute-rewards-earned"]')
          ).toBeVisible();

          const computeRewards = await page.textContent(
            '[data-testid="compute-rewards-earned"]'
          );
          expect(
            parseFloat(computeRewards!.split(' ')[0])
          ).toBeGreaterThanOrEqual(0);
        }
      });
    });

    test('should generate carbon credits from weather data contributions', async ({
      page,
    }) => {
      await test.step('Navigate to carbon credits section', async () => {
        await page.click('[data-testid="nav-carbon-credits"]');
        await expect(
          page.locator('[data-testid="carbon-dashboard"]')
        ).toBeVisible();
      });

      await test.step('Verify carbon credit generation from weather data', async () => {
        // Weather data that helps verify carbon offset projects should generate credits
        await expect(
          page.locator('[data-testid="carbon-credits-earned"]')
        ).toBeVisible();

        const carbonCredits = await page.textContent(
          '[data-testid="carbon-credits-earned"]'
        );
        const creditAmount = parseFloat(carbonCredits!.split(' ')[0]);

        // Even small amounts of carbon credits from weather data verification
        expect(creditAmount).toBeGreaterThanOrEqual(0);

        if (creditAmount > 0) {
          // Check carbon credit details
          await page.click('[data-testid="carbon-credits-details"]');
          await expect(
            page.locator('[data-testid="credit-source-breakdown"]')
          ).toBeVisible();

          // Weather data verification should be listed as a source
          if (
            await page
              .locator('[data-testid="weather-verification-credits"]')
              .isVisible()
          ) {
            const weatherCredits = await page.textContent(
              '[data-testid="weather-verification-credits"]'
            );
            expect(parseFloat(weatherCredits!)).toBeGreaterThan(0);
          }
        }
      });

      await test.step('Test carbon credit trading integration', async () => {
        if (
          await page
            .locator('[data-testid="carbon-marketplace-link"]')
            .isVisible()
        ) {
          await page.click('[data-testid="carbon-marketplace-link"]');
          await expect(
            page.locator('[data-testid="carbon-marketplace"]')
          ).toBeVisible();

          // Should be able to list carbon credits for sale
          if (
            await page
              .locator('[data-testid="list-credits-button"]')
              .isVisible()
          ) {
            await page.click('[data-testid="list-credits-button"]');
            await expect(
              page.locator('[data-testid="listing-modal"]')
            ).toBeVisible();

            await page.click('[data-testid="cancel-listing"]');
          }
        }
      });
    });

    test('should provide staking rewards for weather data providers', async ({
      page,
      walletPage,
    }) => {
      await test.step('Navigate to staking section', async () => {
        await page.click('[data-testid="nav-defi"]');
        await page.click('[data-testid="staking-tab"]');
        await expect(
          page.locator('[data-testid="staking-dashboard"]')
        ).toBeVisible();
      });

      await test.step('Check weather provider staking pools', async () => {
        // Weather data providers should have access to special staking pools
        await expect(
          page.locator('[data-testid="staking-pools"]')
        ).toBeVisible();

        const weatherProviderPool = page.locator(
          '[data-testid="pool-weather-providers"]'
        );
        if (await weatherProviderPool.isVisible()) {
          // Check pool details
          const poolApy = await weatherProviderPool
            .locator('[data-testid="pool-apy"]')
            .textContent();
          expect(parseFloat(poolApy!.split('%')[0])).toBeGreaterThan(0);

          const poolRewards = await weatherProviderPool
            .locator('[data-testid="pool-rewards"]')
            .textContent();
          expect(poolRewards).toBeTruthy();

          // Weather providers should get bonus APY
          const bonusApy = await weatherProviderPool.locator(
            '[data-testid="provider-bonus-apy"]'
          );
          if (await bonusApy.isVisible()) {
            const bonusRate = await bonusApy.textContent();
            expect(
              parseFloat(bonusRate!.split('+')[1]!.split('%')[0])
            ).toBeGreaterThan(0);
          }
        }
      });

      await test.step('Test staking with weather provider benefits', async () => {
        const weatherProviderPool = page.locator(
          '[data-testid="pool-weather-providers"]'
        );
        if (await weatherProviderPool.isVisible()) {
          await weatherProviderPool
            .locator('[data-testid="stake-button"]')
            .click();

          await expect(
            page.locator('[data-testid="staking-modal"]')
          ).toBeVisible();

          // Check if weather provider bonus is displayed
          if (
            await page
              .locator('[data-testid="weather-provider-bonus"]')
              .isVisible()
          ) {
            const bonusText = await page.textContent(
              '[data-testid="weather-provider-bonus"]'
            );
            expect(bonusText).toContain('bonus');
            expect(bonusText).toContain('%');
          }

          // Cancel staking for now
          await page.click('[data-testid="cancel-staking"]');
        }
      });

      await test.step('Verify existing staking rewards for weather providers', async () => {
        if (await page.locator('[data-testid="my-stakes"]').isVisible()) {
          await page.click('[data-testid="my-stakes"]');

          const stakesList = page.locator('[data-testid="stake-item"]');
          if ((await stakesList.count()) > 0) {
            const firstStake = stakesList.first();

            // Check if weather provider bonus is being applied
            if (
              await firstStake
                .locator('[data-testid="weather-bonus-active"]')
                .isVisible()
            ) {
              const bonusAmount = await firstStake
                .locator('[data-testid="bonus-rewards-earned"]')
                .textContent();
              expect(parseFloat(bonusAmount!.split(' ')[0])).toBeGreaterThan(0);
            }
          }
        }
      });
    });
  });
});
