import { Page, Locator, expect } from '@playwright/test';
import { BasePage } from '../common/base-page';

export class RewardsPage extends BasePage {
  // Main Dashboard locators
  readonly rewardsDashboard: Locator;
  readonly totalRewardsEarned: Locator;
  readonly dataQualityMultiplier: Locator;
  readonly frequencyBonus: Locator;
  readonly geographicBonus: Locator;
  readonly qualityBonusAmount: Locator;
  readonly frequencyBonusAmount: Locator;
  readonly geographicBonusAmount: Locator;
  readonly verificationBonusAmount: Locator;

  // Navigation tabs
  readonly rewardHistoryTab: Locator;
  readonly analyticsTab: Locator;
  readonly rewardDetailsTab: Locator;

  // History and Visualization
  readonly rewardHistoryChart: Locator;
  readonly chartRange7d: Locator;
  readonly chartRange30d: Locator;
  readonly chartRange90d: Locator;
  readonly chartDataPoints: Locator;
  readonly chartTooltip: Locator;
  readonly rewardBreakdownTab: Locator;
  readonly baseRewardsChart: Locator;
  readonly qualityBonusChart: Locator;
  readonly rewardCompositionPie: Locator;
  readonly chartLegend: Locator;

  // Analytics
  readonly analyticsDashboard: Locator;
  readonly totalLifetimeRewards: Locator;
  readonly averageDailyRewards: Locator;
  readonly dataSubmissionsCount: Locator;
  readonly averageDataQuality: Locator;
  readonly rewardsTrendIndicator: Locator;
  readonly trendPercentage: Locator;
  readonly rewardProjection: Locator;
  readonly projectedMonthlyRewards: Locator;
  readonly projectedAnnualRewards: Locator;

  // Withdrawal
  readonly withdrawRewardsButton: Locator;
  readonly withdrawalModal: Locator;
  readonly availableWithdrawalBalance: Locator;
  readonly withdrawalAmountInput: Locator;
  readonly withdrawalTokenSelect: Locator;
  readonly withdrawalFee: Locator;
  readonly netWithdrawalAmount: Locator;
  readonly initiateWithdrawalButton: Locator;
  readonly withdrawalConfirmation: Locator;
  readonly confirmWithdrawalButton: Locator;
  readonly withdrawalTxHash: Locator;

  // Reward Breakdown
  readonly consistencyScore: Locator;
  readonly uptimePercentage: Locator;
  readonly geographicMultiplier: Locator;

  // Cross-service Integration
  readonly computeRewardsSection: Locator;
  readonly computeRewardsEarned: Locator;
  readonly carbonCreditsEarned: Locator;
  readonly carbonCreditsDetails: Locator;
  readonly weatherVerificationCredits: Locator;

  // History Table and Filters
  readonly rewardHistoryTableTab: Locator;
  readonly rewardTypeFilter: Locator;
  readonly startDateFilter: Locator;
  readonly endDateFilter: Locator;
  readonly applyDateFilter: Locator;
  readonly rewardHistoryRows: Locator;

  constructor(page: Page) {
    super(page);

    // Main Dashboard elements
    this.rewardsDashboard = page.locator('[data-testid="rewards-dashboard"]');
    this.totalRewardsEarned = page.locator(
      '[data-testid="total-rewards-earned"]'
    );
    this.dataQualityMultiplier = page.locator(
      '[data-testid="data-quality-multiplier"]'
    );
    this.frequencyBonus = page.locator('[data-testid="frequency-bonus"]');
    this.geographicBonus = page.locator('[data-testid="geographic-bonus"]');
    this.qualityBonusAmount = page.locator(
      '[data-testid="quality-bonus-amount"]'
    );
    this.frequencyBonusAmount = page.locator(
      '[data-testid="frequency-bonus-amount"]'
    );
    this.geographicBonusAmount = page.locator(
      '[data-testid="geographic-bonus-amount"]'
    );
    this.verificationBonusAmount = page.locator(
      '[data-testid="verification-bonus-amount"]'
    );

    // Navigation tabs
    this.rewardHistoryTab = page.locator('[data-testid="reward-history-tab"]');
    this.analyticsTab = page.locator('[data-testid="analytics-tab"]');
    this.rewardDetailsTab = page.locator('[data-testid="reward-details-tab"]');

    // History and Visualization
    this.rewardHistoryChart = page.locator(
      '[data-testid="reward-history-chart"]'
    );
    this.chartRange7d = page.locator('[data-testid="chart-range-7d"]');
    this.chartRange30d = page.locator('[data-testid="chart-range-30d"]');
    this.chartRange90d = page.locator('[data-testid="chart-range-90d"]');
    this.chartDataPoints = page.locator('[data-testid="chart-data-points"]');
    this.chartTooltip = page.locator('[data-testid="chart-tooltip"]');
    this.rewardBreakdownTab = page.locator(
      '[data-testid="reward-breakdown-tab"]'
    );
    this.baseRewardsChart = page.locator('[data-testid="base-rewards-chart"]');
    this.qualityBonusChart = page.locator(
      '[data-testid="quality-bonus-chart"]'
    );
    this.rewardCompositionPie = page.locator(
      '[data-testid="reward-composition-pie"]'
    );
    this.chartLegend = page.locator('[data-testid="chart-legend"]');

    // Analytics
    this.analyticsDashboard = page.locator(
      '[data-testid="analytics-dashboard"]'
    );
    this.totalLifetimeRewards = page.locator(
      '[data-testid="total-lifetime-rewards"]'
    );
    this.averageDailyRewards = page.locator(
      '[data-testid="average-daily-rewards"]'
    );
    this.dataSubmissionsCount = page.locator(
      '[data-testid="data-submissions-count"]'
    );
    this.averageDataQuality = page.locator(
      '[data-testid="average-data-quality"]'
    );
    this.rewardsTrendIndicator = page.locator(
      '[data-testid="rewards-trend-indicator"]'
    );
    this.trendPercentage = page.locator('[data-testid="trend-percentage"]');
    this.rewardProjection = page.locator('[data-testid="reward-projection"]');
    this.projectedMonthlyRewards = page.locator(
      '[data-testid="projected-monthly-rewards"]'
    );
    this.projectedAnnualRewards = page.locator(
      '[data-testid="projected-annual-rewards"]'
    );

    // Withdrawal
    this.withdrawRewardsButton = page.locator(
      '[data-testid="withdraw-rewards-button"]'
    );
    this.withdrawalModal = page.locator('[data-testid="withdrawal-modal"]');
    this.availableWithdrawalBalance = page.locator(
      '[data-testid="available-withdrawal-balance"]'
    );
    this.withdrawalAmountInput = page.locator(
      '[data-testid="withdrawal-amount-input"]'
    );
    this.withdrawalTokenSelect = page.locator(
      '[data-testid="withdrawal-token-select"]'
    );
    this.withdrawalFee = page.locator('[data-testid="withdrawal-fee"]');
    this.netWithdrawalAmount = page.locator(
      '[data-testid="net-withdrawal-amount"]'
    );
    this.initiateWithdrawalButton = page.locator(
      '[data-testid="initiate-withdrawal-button"]'
    );
    this.withdrawalConfirmation = page.locator(
      '[data-testid="withdrawal-confirmation"]'
    );
    this.confirmWithdrawalButton = page.locator(
      '[data-testid="confirm-withdrawal-button"]'
    );
    this.withdrawalTxHash = page.locator('[data-testid="withdrawal-tx-hash"]');

    // Reward Breakdown
    this.consistencyScore = page.locator('[data-testid="consistency-score"]');
    this.uptimePercentage = page.locator('[data-testid="uptime-percentage"]');
    this.geographicMultiplier = page.locator(
      '[data-testid="geographic-multiplier"]'
    );

    // Cross-service Integration
    this.computeRewardsSection = page.locator(
      '[data-testid="compute-rewards-section"]'
    );
    this.computeRewardsEarned = page.locator(
      '[data-testid="compute-rewards-earned"]'
    );
    this.carbonCreditsEarned = page.locator(
      '[data-testid="carbon-credits-earned"]'
    );
    this.carbonCreditsDetails = page.locator(
      '[data-testid="carbon-credits-details"]'
    );
    this.weatherVerificationCredits = page.locator(
      '[data-testid="weather-verification-credits"]'
    );

    // History Table and Filters
    this.rewardHistoryTableTab = page.locator(
      '[data-testid="reward-history-table-tab"]'
    );
    this.rewardTypeFilter = page.locator('[data-testid="reward-type-filter"]');
    this.startDateFilter = page.locator('[data-testid="start-date-filter"]');
    this.endDateFilter = page.locator('[data-testid="end-date-filter"]');
    this.applyDateFilter = page.locator('[data-testid="apply-date-filter"]');
    this.rewardHistoryRows = page.locator('[data-testid="reward-history-row"]');
  }

  // Navigation methods
  async gotoRewards() {
    await this.goto('/dashboard/rewards');
    await expect(this.page).toHaveURL(/.*\/rewards/);
    await expect(this.rewardsDashboard).toBeVisible();
  }

  async switchToHistoryTab() {
    await this.rewardHistoryTab.click();
    await expect(this.rewardHistoryChart).toBeVisible();
  }

  async switchToAnalyticsTab() {
    await this.analyticsTab.click();
    await expect(this.analyticsDashboard).toBeVisible();
  }

  async switchToDetailsTab() {
    await this.rewardDetailsTab.click();
    await expect(this.consistencyScore).toBeVisible();
  }

  // Dashboard methods
  async getTotalRewardsEarned(): Promise<number> {
    const rewardsText = await this.totalRewardsEarned.textContent();
    return parseFloat(rewardsText?.split(' ')[0] || '0');
  }

  async getRewardBonuses(): Promise<{
    quality: number;
    frequency: number;
    geographic: number;
    verification?: number;
  }> {
    const quality = parseFloat(
      (await this.qualityBonusAmount.textContent()) || '0'
    );
    const frequency = parseFloat(
      (await this.frequencyBonusAmount.textContent()) || '0'
    );
    const geographic = parseFloat(
      (await this.geographicBonusAmount.textContent()) || '0'
    );

    let verification: number | undefined;
    if (await this.verificationBonusAmount.isVisible()) {
      verification = parseFloat(
        (await this.verificationBonusAmount.textContent()) || '0'
      );
    }

    return { quality, frequency, geographic, verification };
  }

  async getGeographicMultiplier(): Promise<number> {
    const multiplierText = await this.geographicMultiplier.textContent();
    return parseFloat(multiplierText || '1.0');
  }

  // Chart and visualization methods
  async selectChartTimeRange(range: '7d' | '30d' | '90d') {
    const ranges = {
      '7d': this.chartRange7d,
      '30d': this.chartRange30d,
      '90d': this.chartRange90d,
    };

    await ranges[range].click();
    await this.page.waitForTimeout(1000); // Allow chart to update
  }

  async getChartDataPoints(): Promise<number> {
    return await this.chartDataPoints.count();
  }

  async hoverOnChart(
    x: number,
    y: number
  ): Promise<{
    date?: string;
    amount?: string;
  }> {
    await this.rewardHistoryChart.hover({ position: { x, y } });

    if (await this.chartTooltip.isVisible()) {
      const date = await this.page
        .locator('[data-testid="tooltip-date"]')
        .textContent();
      const amount = await this.page
        .locator('[data-testid="tooltip-reward-amount"]')
        .textContent();
      return { date: date || undefined, amount: amount || undefined };
    }

    return {};
  }

  async switchToBreakdownView() {
    await this.rewardBreakdownTab.click();
    await expect(this.baseRewardsChart).toBeVisible();
    await expect(this.qualityBonusChart).toBeVisible();
  }

  // Analytics methods
  async getAnalyticsMetrics(): Promise<{
    lifetimeRewards: number;
    dailyAverage: number;
    submissionCount: number;
    averageQuality: number;
  }> {
    await this.switchToAnalyticsTab();

    const lifetime = parseFloat(
      (await this.totalLifetimeRewards.textContent()?.split(' ')[0]) || '0'
    );
    const daily = parseFloat(
      (await this.averageDailyRewards.textContent()?.split(' ')[0]) || '0'
    );
    const submissions = parseInt(
      (await this.dataSubmissionsCount.textContent()) || '0'
    );
    const quality = parseFloat(
      (await this.averageDataQuality.textContent()?.split('%')[0]) || '0'
    );

    return {
      lifetimeRewards: lifetime,
      dailyAverage: daily,
      submissionCount: submissions,
      averageQuality: quality,
    };
  }

  async getTrendIndicator(): Promise<{
    direction: 'increase' | 'decrease' | 'stable';
    percentage?: number;
  }> {
    const trendText = (await this.rewardsTrendIndicator.textContent()) || '';
    let direction: 'increase' | 'decrease' | 'stable' = 'stable';

    if (trendText.toLowerCase().includes('increase')) {
      direction = 'increase';
    } else if (trendText.toLowerCase().includes('decrease')) {
      direction = 'decrease';
    }

    let percentage: number | undefined;
    if (await this.trendPercentage.isVisible()) {
      percentage = parseFloat(
        (await this.trendPercentage.textContent()?.split('%')[0]) || '0'
      );
    }

    return { direction, percentage };
  }

  async getProjectedEarnings(): Promise<{
    monthly?: number;
    annual?: number;
  }> {
    if (!(await this.rewardProjection.isVisible())) {
      return {};
    }

    let monthly: number | undefined;
    let annual: number | undefined;

    if (await this.projectedMonthlyRewards.isVisible()) {
      monthly = parseFloat(
        (await this.projectedMonthlyRewards.textContent()?.split(' ')[0]) || '0'
      );
    }

    if (await this.projectedAnnualRewards.isVisible()) {
      annual = parseFloat(
        (await this.projectedAnnualRewards.textContent()?.split(' ')[0]) || '0'
      );
    }

    return { monthly, annual };
  }

  // Reward details methods
  async getConsistencyMetrics(): Promise<{
    consistencyScore: number;
    uptimePercentage: number;
  }> {
    await this.switchToDetailsTab();

    const consistency = parseFloat(
      (await this.consistencyScore.textContent()?.split('%')[0]) || '0'
    );
    const uptime = parseFloat(
      (await this.uptimePercentage.textContent()?.split('%')[0]) || '0'
    );

    return {
      consistencyScore: consistency,
      uptimePercentage: uptime,
    };
  }

  // Withdrawal methods
  async getAvailableWithdrawalBalance(): Promise<number> {
    await this.withdrawRewardsButton.click();
    await expect(this.withdrawalModal).toBeVisible();

    const balanceText = await this.availableWithdrawalBalance.textContent();
    return parseFloat(balanceText?.split(' ')[0] || '0');
  }

  async initiateWithdrawal(
    amount: number,
    token: string = 'WRF'
  ): Promise<string> {
    if (!(await this.withdrawalModal.isVisible())) {
      await this.withdrawRewardsButton.click();
      await expect(this.withdrawalModal).toBeVisible();
    }

    await this.withdrawalAmountInput.fill(amount.toString());
    await this.withdrawalTokenSelect.selectOption(token);

    // Check fees
    if (await this.withdrawalFee.isVisible()) {
      const fee = await this.withdrawalFee.textContent();
      console.log(`Withdrawal fee: ${fee}`);
    }

    await this.initiateWithdrawalButton.click();
    await expect(this.withdrawalConfirmation).toBeVisible();
    await this.confirmWithdrawalButton.click();

    // Wait for transaction
    await this.waitForAPIResponse('withdrawal');
    await expect(
      this.page.locator('[data-testid="withdrawal-success"]')
    ).toBeVisible();

    const txHash = await this.withdrawalTxHash.textContent();
    return txHash || '';
  }

  async getNetWithdrawalAmount(): Promise<number> {
    const netAmountText = await this.netWithdrawalAmount.textContent();
    return parseFloat(netAmountText?.split(' ')[0] || '0');
  }

  // History filtering methods
  async filterRewardHistory(options: {
    rewardType?: string;
    startDate?: string;
    endDate?: string;
  }) {
    await this.rewardHistoryTableTab.click();

    if (options.rewardType) {
      await this.rewardTypeFilter.selectOption(options.rewardType);
    }

    if (options.startDate) {
      await this.startDateFilter.fill(options.startDate);
    }

    if (options.endDate) {
      await this.endDateFilter.fill(options.endDate);
    }

    await this.applyDateFilter.click();
    await this.page.waitForTimeout(2000); // Allow filtering to complete
  }

  async getRewardHistoryEntries(): Promise<
    Array<{
      type: string;
      amount: string;
      date: string;
      status: string;
    }>
  > {
    const entries = [];
    const rows = this.rewardHistoryRows;
    const count = await rows.count();

    for (let i = 0; i < count; i++) {
      const row = rows.nth(i);
      const type =
        (await row.locator('[data-testid="reward-type"]').textContent()) || '';
      const amount =
        (await row.locator('[data-testid="reward-amount"]').textContent()) ||
        '';
      const date =
        (await row.locator('[data-testid="reward-date"]').textContent()) || '';
      const status =
        (await row.locator('[data-testid="reward-status"]').textContent()) ||
        '';

      entries.push({ type, amount, date, status });
    }

    return entries;
  }

  // Cross-service integration methods
  async getComputeRewards(): Promise<number> {
    if (!(await this.computeRewardsSection.isVisible())) {
      return 0;
    }

    const rewardsText = await this.computeRewardsEarned.textContent();
    return parseFloat(rewardsText?.split(' ')[0] || '0');
  }

  async getCarbonCredits(): Promise<{
    totalCredits: number;
    weatherVerificationCredits?: number;
  }> {
    const totalText = await this.carbonCreditsEarned.textContent();
    const totalCredits = parseFloat(totalText?.split(' ')[0] || '0');

    let weatherVerificationCredits: number | undefined;
    if (await this.carbonCreditsDetails.isVisible()) {
      await this.carbonCreditsDetails.click();
      if (await this.weatherVerificationCredits.isVisible()) {
        const weatherCreditsText =
          await this.weatherVerificationCredits.textContent();
        weatherVerificationCredits = parseFloat(weatherCreditsText || '0');
      }
    }

    return { totalCredits, weatherVerificationCredits };
  }

  // Real-time update monitoring
  async waitForRewardUpdate(
    initialAmount: number,
    timeout: number = 30000
  ): Promise<number> {
    return await this.page.waitForFunction(
      initial => {
        const balanceElement = document.querySelector(
          '[data-testid="total-rewards-earned"]'
        );
        const currentBalance = balanceElement
          ? parseFloat(balanceElement.textContent!.split(' ')[0])
          : 0;
        return currentBalance > initial;
      },
      initialAmount,
      { timeout }
    );
  }

  // Assertions
  async assertRewardsIncrease(initialAmount: number) {
    const currentAmount = await this.getTotalRewardsEarned();
    expect(currentAmount).toBeGreaterThan(initialAmount);
  }

  async assertQualityBonusApplied(minBonus: number = 0) {
    const bonuses = await this.getRewardBonuses();
    expect(bonuses.quality).toBeGreaterThan(minBonus);
  }

  async assertFrequencyBonusApplied(minBonus: number = 0) {
    const bonuses = await this.getRewardBonuses();
    expect(bonuses.frequency).toBeGreaterThan(minBonus);
  }

  async assertGeographicMultiplierApplied(expectedMultiplier: number) {
    const multiplier = await this.getGeographicMultiplier();
    expect(multiplier).toBeCloseTo(expectedMultiplier, 1);
  }

  async assertWithdrawalSuccess(txHash: string) {
    expect(txHash).toMatch(/^0x[a-fA-F0-9]{64}$/);
    await expect(
      this.page.locator('[data-testid="withdrawal-success"]')
    ).toBeVisible();
  }

  async assertAnalyticsDataAccuracy() {
    const metrics = await this.getAnalyticsMetrics();

    expect(metrics.lifetimeRewards).toBeGreaterThanOrEqual(0);
    expect(metrics.dailyAverage).toBeGreaterThanOrEqual(0);
    expect(metrics.submissionCount).toBeGreaterThanOrEqual(0);
    expect(metrics.averageQuality).toBeGreaterThan(0);
    expect(metrics.averageQuality).toBeLessThanOrEqual(100);
  }

  async assertConsistencyMetricsValid() {
    const metrics = await this.getConsistencyMetrics();

    expect(metrics.consistencyScore).toBeGreaterThanOrEqual(0);
    expect(metrics.consistencyScore).toBeLessThanOrEqual(100);
    expect(metrics.uptimePercentage).toBeGreaterThanOrEqual(0);
    expect(metrics.uptimePercentage).toBeLessThanOrEqual(100);
  }
}
