import { Page, Locator, expect } from '@playwright/test';
import { BasePage } from '../common/base-page';

export class WeatherDataPage extends BasePage {
  // Navigation locators
  readonly iotIntegrationTab: Locator;
  readonly manualEntryTab: Locator;
  readonly batchUploadTab: Locator;
  readonly mySubmissionsTab: Locator;
  readonly dataQualityTab: Locator;

  // IoT Integration locators
  readonly iotDashboard: Locator;
  readonly addIotDeviceButton: Locator;
  readonly deviceRegistrationModal: Locator;
  readonly deviceNameInput: Locator;
  readonly deviceIdInput: Locator;
  readonly deviceLocationInput: Locator;
  readonly deviceTypeSelect: Locator;
  readonly collectionIntervalSelect: Locator;
  readonly registerDeviceButton: Locator;
  readonly apiEndpoint: Locator;
  readonly generateApiKeyButton: Locator;
  readonly deviceApiKey: Locator;

  // Manual Entry locators
  readonly manualEntryForm: Locator;
  readonly temperatureInput: Locator;
  readonly humidityInput: Locator;
  readonly pressureInput: Locator;
  readonly windSpeedInput: Locator;
  readonly locationInput: Locator;
  readonly latitudeInput: Locator;
  readonly longitudeInput: Locator;
  readonly observationNotes: Locator;
  readonly validateDataButton: Locator;
  readonly submitManualDataButton: Locator;

  // Batch Upload locators
  readonly batchUploadInterface: Locator;
  readonly batchFileInput: Locator;
  readonly dataSourceSelect: Locator;
  readonly dataSourceDescription: Locator;
  readonly timeZoneSelect: Locator;
  readonly startBatchUploadButton: Locator;
  readonly uploadProgress: Locator;
  readonly processingResults: Locator;

  // Data Quality locators
  readonly qualityScore: Locator;
  readonly accuracyScore: Locator;
  readonly completenessScore: Locator;
  readonly timelinessScore: Locator;

  // Device Management locators
  readonly deviceCards: Locator;
  readonly deviceStatus: Locator;
  readonly lastReading: Locator;
  readonly latestTemperature: Locator;

  constructor(page: Page) {
    super(page);

    // Navigation tabs
    this.iotIntegrationTab = page.locator(
      '[data-testid="iot-integration-tab"]'
    );
    this.manualEntryTab = page.locator('[data-testid="manual-entry-tab"]');
    this.batchUploadTab = page.locator('[data-testid="batch-upload-tab"]');
    this.mySubmissionsTab = page.locator('[data-testid="my-submissions-tab"]');
    this.dataQualityTab = page.locator('[data-testid="data-quality-tab"]');

    // IoT Integration elements
    this.iotDashboard = page.locator('[data-testid="iot-dashboard"]');
    this.addIotDeviceButton = page.locator(
      '[data-testid="add-iot-device-button"]'
    );
    this.deviceRegistrationModal = page.locator(
      '[data-testid="device-registration-modal"]'
    );
    this.deviceNameInput = page.locator('[data-testid="device-name-input"]');
    this.deviceIdInput = page.locator('[data-testid="device-id-input"]');
    this.deviceLocationInput = page.locator(
      '[data-testid="device-location-input"]'
    );
    this.deviceTypeSelect = page.locator('[data-testid="device-type-select"]');
    this.collectionIntervalSelect = page.locator(
      '[data-testid="collection-interval"]'
    );
    this.registerDeviceButton = page.locator(
      '[data-testid="register-device-button"]'
    );
    this.apiEndpoint = page.locator('[data-testid="api-endpoint"]');
    this.generateApiKeyButton = page.locator(
      '[data-testid="generate-api-key-button"]'
    );
    this.deviceApiKey = page.locator('[data-testid="device-api-key"]');

    // Manual Entry elements
    this.manualEntryForm = page.locator('[data-testid="manual-entry-form"]');
    this.temperatureInput = page.locator('[data-testid="temperature-input"]');
    this.humidityInput = page.locator('[data-testid="humidity-input"]');
    this.pressureInput = page.locator('[data-testid="pressure-input"]');
    this.windSpeedInput = page.locator('[data-testid="wind-speed-input"]');
    this.locationInput = page.locator('[data-testid="location-input"]');
    this.latitudeInput = page.locator('[data-testid="latitude-input"]');
    this.longitudeInput = page.locator('[data-testid="longitude-input"]');
    this.observationNotes = page.locator('[data-testid="observation-notes"]');
    this.validateDataButton = page.locator(
      '[data-testid="validate-data-button"]'
    );
    this.submitManualDataButton = page.locator(
      '[data-testid="submit-manual-data-button"]'
    );

    // Batch Upload elements
    this.batchUploadInterface = page.locator(
      '[data-testid="batch-upload-interface"]'
    );
    this.batchFileInput = page.locator('[data-testid="batch-file-input"]');
    this.dataSourceSelect = page.locator('[data-testid="data-source-select"]');
    this.dataSourceDescription = page.locator(
      '[data-testid="data-source-description"]'
    );
    this.timeZoneSelect = page.locator('[data-testid="time-zone-select"]');
    this.startBatchUploadButton = page.locator(
      '[data-testid="start-batch-upload-button"]'
    );
    this.uploadProgress = page.locator('[data-testid="upload-progress"]');
    this.processingResults = page.locator('[data-testid="processing-summary"]');

    // Data Quality elements
    this.qualityScore = page.locator('[data-testid="quality-score"]');
    this.accuracyScore = page.locator('[data-testid="accuracy-score"]');
    this.completenessScore = page.locator('[data-testid="completeness-score"]');
    this.timelinessScore = page.locator('[data-testid="timeliness-score"]');

    // Device Management elements
    this.deviceCards = page.locator('[data-testid="device-card"]');
    this.deviceStatus = page.locator('[data-testid="device-status"]');
    this.lastReading = page.locator('[data-testid="last-reading"]');
    this.latestTemperature = page.locator('[data-testid="latest-temperature"]');
  }

  // Navigation methods
  async gotoWeatherData() {
    await this.goto('/dashboard/weather-data');
    await expect(this.page).toHaveURL(/.*\/weather-data/);
  }

  async switchToIotTab() {
    await this.iotIntegrationTab.click();
    await expect(this.iotDashboard).toBeVisible();
  }

  async switchToManualEntryTab() {
    await this.manualEntryTab.click();
    await expect(this.manualEntryForm).toBeVisible();
  }

  async switchToBatchUploadTab() {
    await this.batchUploadTab.click();
    await expect(this.batchUploadInterface).toBeVisible();
  }

  // IoT Device Management
  async registerIotDevice(deviceData: {
    name: string;
    id: string;
    location: string;
    type: string;
    collectionInterval?: string;
  }) {
    await this.addIotDeviceButton.click();
    await expect(this.deviceRegistrationModal).toBeVisible();

    await this.deviceNameInput.fill(deviceData.name);
    await this.deviceIdInput.fill(deviceData.id);
    await this.deviceLocationInput.fill(deviceData.location);
    await this.deviceTypeSelect.selectOption(deviceData.type);

    if (deviceData.collectionInterval) {
      await this.collectionIntervalSelect.selectOption(
        deviceData.collectionInterval
      );
    }

    // Enable data collection types
    await this.page.check('[data-testid="collect-temperature"]');
    await this.page.check('[data-testid="collect-humidity"]');
    await this.page.check('[data-testid="collect-pressure"]');
    await this.page.check('[data-testid="collect-wind"]');

    await this.registerDeviceButton.click();
    await expect(
      this.page.locator('[data-testid="device-registered-success"]')
    ).toBeVisible();
  }

  async generateApiKeyForDevice(deviceId: string): Promise<string> {
    const deviceCard = this.page.locator(
      `[data-testid="device-card-${deviceId}"]`
    );
    await deviceCard.click();

    await this.page.click('[data-testid="configure-endpoint-button"]');
    await this.generateApiKeyButton.click();

    const apiKey = await this.deviceApiKey.textContent();
    return apiKey || '';
  }

  async getDeviceEndpoint(deviceId: string): Promise<string> {
    const endpointText = await this.apiEndpoint.textContent();
    return endpointText || '';
  }

  // Manual Data Entry
  async submitManualWeatherData(weatherData: {
    temperature: number;
    humidity: number;
    pressure: number;
    windSpeed: number;
    location: {
      name: string;
      lat: number;
      lon: number;
    };
    notes?: string;
  }) {
    await this.switchToManualEntryTab();

    await this.temperatureInput.fill(weatherData.temperature.toString());
    await this.humidityInput.fill(weatherData.humidity.toString());
    await this.pressureInput.fill(weatherData.pressure.toString());
    await this.windSpeedInput.fill(weatherData.windSpeed.toString());

    await this.locationInput.fill(weatherData.location.name);
    await this.latitudeInput.fill(weatherData.location.lat.toString());
    await this.longitudeInput.fill(weatherData.location.lon.toString());

    if (weatherData.notes) {
      await this.observationNotes.fill(weatherData.notes);
    }

    await this.validateDataButton.click();
    await expect(
      this.page.locator('[data-testid="validation-success"]')
    ).toBeVisible();

    await this.submitManualDataButton.click();
    await expect(
      this.page.locator('[data-testid="manual-submission-success"]')
    ).toBeVisible();

    const submissionId = await this.page.textContent(
      '[data-testid="submission-id"]'
    );
    return submissionId || '';
  }

  // Batch Upload
  async uploadBatchWeatherData(
    csvContent: string,
    options: {
      dataSource?: string;
      description?: string;
      timeZone?: string;
    } = {}
  ) {
    await this.switchToBatchUploadTab();

    // Create file for upload
    await this.batchFileInput.setInputFiles([
      {
        name: 'weather_batch.csv',
        mimeType: 'text/csv',
        buffer: Buffer.from(csvContent),
      },
    ]);

    await expect(
      this.page.locator('[data-testid="file-validation-success"]')
    ).toBeVisible();

    // Configure options
    if (options.dataSource) {
      await this.dataSourceSelect.selectOption(options.dataSource);
    }

    if (options.description) {
      await this.dataSourceDescription.fill(options.description);
    }

    if (options.timeZone) {
      await this.timeZoneSelect.selectOption(options.timeZone);
    }

    // Enable validation options
    await this.page.check('[data-testid="validate-coordinates"]');
    await this.page.check('[data-testid="detect-duplicates"]');

    await this.startBatchUploadButton.click();
    await expect(this.uploadProgress).toBeVisible();

    // Wait for completion
    await this.page.waitForFunction(
      () => {
        const progress = document.querySelector(
          '[data-testid="upload-progress"]'
        );
        return progress?.textContent?.includes('100%');
      },
      { timeout: 60000 }
    );

    await expect(
      this.page.locator('[data-testid="batch-upload-complete"]')
    ).toBeVisible();

    const batchId = await this.page.textContent('[data-testid="batch-id"]');
    return batchId || '';
  }

  // Data Quality Assessment
  async getDataQualityScore(submissionId?: string): Promise<{
    overall: number;
    accuracy: number;
    completeness: number;
    timeliness: number;
  }> {
    if (submissionId) {
      const submissionRow = this.page.locator(
        `[data-testid="submission-${submissionId}"]`
      );
      await submissionRow.click();
    }

    await this.dataQualityTab.click();

    const overallScore = await this.qualityScore.textContent();
    const accuracyScore = await this.accuracyScore.textContent();
    const completenessScore = await this.completenessScore.textContent();
    const timelinessScore = await this.timelinessScore.textContent();

    return {
      overall: parseFloat(overallScore?.split('%')[0] || '0'),
      accuracy: parseFloat(accuracyScore?.split('%')[0] || '0'),
      completeness: parseFloat(completenessScore?.split('%')[0] || '0'),
      timeliness: parseFloat(timelinessScore?.split('%')[0] || '0'),
    };
  }

  // Device Status Monitoring
  async getDeviceStatus(deviceId: string): Promise<{
    status: string;
    lastReading: string;
    latestTemperature: string;
  }> {
    const deviceCard = this.page.locator(
      `[data-testid="device-card-${deviceId}"]`
    );
    await expect(deviceCard).toBeVisible();

    const status =
      (await deviceCard
        .locator('[data-testid="device-status"]')
        .textContent()) || '';
    const lastReading =
      (await deviceCard
        .locator('[data-testid="last-reading"]')
        .textContent()) || '';
    const latestTemp =
      (await deviceCard
        .locator('[data-testid="latest-temperature"]')
        .textContent()) || '';

    return {
      status,
      lastReading,
      latestTemperature: latestTemp,
    };
  }

  async getSubmissionHistory(): Promise<
    Array<{
      id: string;
      type: string;
      status: string;
      timestamp: string;
      qualityScore?: number;
    }>
  > {
    await this.mySubmissionsTab.click();

    const submissions = [];
    const submissionRows = this.page.locator('[data-testid^="submission-"]');
    const count = await submissionRows.count();

    for (let i = 0; i < count; i++) {
      const row = submissionRows.nth(i);
      const id = await row.getAttribute('data-testid');
      const type =
        (await row.locator('[data-testid="submission-type"]').textContent()) ||
        '';
      const status =
        (await row
          .locator('[data-testid="submission-status"]')
          .textContent()) || '';
      const timestamp =
        (await row
          .locator('[data-testid="submission-timestamp"]')
          .textContent()) || '';

      let qualityScore: number | undefined;
      if (await row.locator('[data-testid="quality-score"]').isVisible()) {
        const scoreText = await row
          .locator('[data-testid="quality-score"]')
          .textContent();
        qualityScore = parseFloat(scoreText?.split('%')[0] || '0');
      }

      submissions.push({
        id: id?.replace('submission-', '') || '',
        type,
        status,
        timestamp,
        qualityScore,
      });
    }

    return submissions;
  }

  // Validation Methods
  async validateTemperatureRange(temperature: number): Promise<boolean> {
    await this.temperatureInput.fill(temperature.toString());
    await this.validateDataButton.click();

    const hasError = await this.page
      .locator('[data-testid="temperature-range-error"]')
      .isVisible();
    return !hasError;
  }

  async validateCoordinates(lat: number, lon: number): Promise<boolean> {
    await this.latitudeInput.fill(lat.toString());
    await this.longitudeInput.fill(lon.toString());
    await this.validateDataButton.click();

    const hasError = await this.page
      .locator('[data-testid="coordinate-validation-error"]')
      .isVisible();
    return !hasError;
  }

  // Assertions
  async assertDeviceRegistered(deviceId: string) {
    const deviceCard = this.page.locator(
      `[data-testid="device-card-${deviceId}"]`
    );
    await expect(deviceCard).toBeVisible();

    const status = deviceCard.locator('[data-testid="device-status"]');
    await expect(status).toContainText(/Active|Connected|Online/);
  }

  async assertManualSubmissionSuccess(submissionId: string) {
    await this.mySubmissionsTab.click();

    const submissionRow = this.page.locator(
      `[data-testid="submission-${submissionId}"]`
    );
    await expect(submissionRow).toBeVisible();

    const status = submissionRow.locator('[data-testid="submission-status"]');
    await expect(status).toContainText(/Processing|Validated|Accepted/);
  }

  async assertBatchUploadSuccess(batchId: string, expectedRecordCount: number) {
    await expect(this.processingResults).toBeVisible();

    const processedCount = await this.page.textContent(
      '[data-testid="processed-records"]'
    );
    expect(parseInt(processedCount!)).toBeGreaterThan(
      expectedRecordCount * 0.8
    ); // At least 80% success

    const rejectedCount = await this.page.textContent(
      '[data-testid="rejected-records"]'
    );
    expect(parseInt(rejectedCount!)).toBeLessThan(expectedRecordCount * 0.2); // Less than 20% rejected
  }

  async assertDataQualityMeetsStandards(minQualityScore: number = 80) {
    const qualityScoreText = await this.qualityScore.textContent();
    const score = parseFloat(qualityScoreText?.split('%')[0] || '0');

    expect(score).toBeGreaterThanOrEqual(minQualityScore);
  }
}
