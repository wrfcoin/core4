const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../utils/test-helpers');

test.describe('Computational Jobs Workflow', () => {
  let helpers;

  test.beforeEach(async ({ page }) => {
    helpers = new TestHelpers(page);
    await helpers.loginAs('compute-provider');
    await helpers.connectWallet('compute-provider');
  });

  test.afterEach(async () => {
    await helpers.cleanup();
  });

  test('Register as computational resource provider', async ({ page }) => {
    // Navigate to computational jobs section
    await page.click('[data-testid="nav-compute"]');
    await page.waitForURL('**/compute');

    // Verify compute dashboard
    await expect(
      page.locator('[data-testid="compute-dashboard"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="available-jobs"]')).toBeVisible();
    await expect(page.locator('[data-testid="provider-status"]')).toBeVisible();

    // Register as provider
    await page.click('[data-testid="become-provider-button"]');
    await page.waitForURL('**/compute/provider/register');

    // Fill provider registration form
    const providerData = {
      organizationName: 'Advanced Computing Solutions',
      contactEmail: 'provider@advancedcompute.com',
      location: 'San Francisco, CA',
      description:
        'High-performance computing cluster for weather modeling and AI/ML workloads',
    };

    await helpers.fillForm({
      'organization-name': providerData.organizationName,
      'contact-email': providerData.contactEmail,
      'provider-location': providerData.location,
      'provider-description': providerData.description,
    });

    // Specify computational resources
    await page.click('[data-testid="add-compute-resource"]');
    await helpers.fillForm({
      'resource-name-0': 'GPU Cluster Alpha',
      'cpu-cores-0': '128',
      'memory-gb-0': '512',
      'storage-gb-0': '10000',
      'gpu-count-0': '8',
      'gpu-model-0': 'NVIDIA A100',
    });

    await page.click('[data-testid="add-compute-resource"]');
    await helpers.fillForm({
      'resource-name-1': 'CPU Farm Beta',
      'cpu-cores-1': '256',
      'memory-gb-1': '1024',
      'storage-gb-1': '50000',
      'gpu-count-1': '0',
      'gpu-model-1': 'None',
    });

    // Set pricing and availability
    await helpers.fillForm({
      'cpu-hour-rate': '0.10',
      'memory-gb-hour-rate': '0.05',
      'gpu-hour-rate': '2.50',
      'storage-gb-rate': '0.01',
    });

    // Configure availability schedule
    await page.check('[data-testid="available-24-7"]');
    await page.fill('[data-testid="max-concurrent-jobs"]', '10');

    // Upload certifications and compliance documents
    await helpers.uploadFile(
      '[data-testid="security-certification"]',
      './fixtures/security-cert.pdf'
    );
    await helpers.uploadFile(
      '[data-testid="compliance-docs"]',
      './fixtures/compliance.pdf'
    );

    // Review and submit registration
    await page.click('[data-testid="review-registration-button"]');

    const modal = await helpers.waitForModal();

    // Verify registration summary
    await expect(
      page.locator('[data-testid="registration-summary"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="resource-summary"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="pricing-summary"]')).toBeVisible();

    // Submit registration
    await page.click('[data-testid="submit-registration-button"]');

    // Authenticate
    await page.fill(
      '[data-testid="wallet-password-input"]',
      'ProviderPass123!'
    );
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Verify provider status
    await page.waitForURL('**/compute/provider/dashboard');
    await expect(page.locator('[data-testid="provider-status"]')).toContainText(
      'Registered'
    );
    await expect(
      page.locator('[data-testid="pending-approval"]')
    ).toBeVisible();
  });

  test('Create computational job NFT', async ({ page }) => {
    // Switch to job creator
    await helpers.loginAs('researcher');
    await helpers.connectWallet('researcher');

    await page.click('[data-testid="nav-compute"]');
    await page.click('[data-testid="create-job-button"]');
    await page.waitForURL('**/compute/jobs/create');

    // Select job type
    await page.click('[data-testid="job-type-weather-modeling"]');

    // Fill job details
    const jobData = {
      title: 'Hurricane Path Prediction Model',
      description:
        'High-resolution atmospheric modeling to predict hurricane trajectory and intensity',
      category: 'weather-forecasting',
      priority: 'high',
    };

    await helpers.fillForm({
      'job-title': jobData.title,
      'job-description': jobData.description,
      'job-category': jobData.category,
    });

    await page.selectOption('[data-testid="job-priority"]', jobData.priority);

    // Specify computational requirements
    await helpers.fillForm({
      'required-cpu-cores': '64',
      'required-memory-gb': '256',
      'required-storage-gb': '5000',
      'required-gpu-count': '4',
      'estimated-runtime-hours': '12',
      'max-runtime-hours': '24',
    });

    // Set job parameters
    await page.click('[data-testid="add-job-parameter"]');
    await helpers.fillForm({
      'parameter-name-0': 'SIMULATION_RESOLUTION',
      'parameter-value-0': '0.25km',
      'parameter-type-0': 'string',
    });

    await page.click('[data-testid="add-job-parameter"]');
    await helpers.fillForm({
      'parameter-name-1': 'FORECAST_HOURS',
      'parameter-value-1': '120',
      'parameter-type-1': 'integer',
    });

    // Upload input data and scripts
    await helpers.uploadFile(
      '[data-testid="input-data-upload"]',
      './fixtures/weather-data.nc'
    );
    await helpers.uploadFile(
      '[data-testid="job-script-upload"]',
      './fixtures/hurricane-model.py'
    );
    await helpers.uploadFile(
      '[data-testid="requirements-upload"]',
      './fixtures/requirements.txt'
    );

    // Configure payment and rewards
    await helpers.fillForm({
      'job-payment-amount': '500',
      'job-payment-token': 'WRF',
      'completion-bonus': '100',
      'quality-bonus': '50',
    });

    // Set deadlines and SLA
    await page.fill('[data-testid="submission-deadline"]', '2024-12-31');
    await page.check('[data-testid="require-sla"]');
    await page.fill('[data-testid="sla-uptime"]', '99.9');

    // Create NFT for the job
    await page.click('[data-testid="create-job-nft-button"]');

    const modal = await helpers.waitForModal();

    // Review job NFT details
    await expect(page.locator('[data-testid="nft-preview"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="job-requirements-summary"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="payment-terms"]')).toBeVisible();

    // Confirm NFT creation
    await page.click('[data-testid="confirm-nft-creation"]');

    // Authenticate
    await page.fill(
      '[data-testid="wallet-password-input"]',
      'ResearcherPass123!'
    );
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Verify job NFT created
    await page.waitForURL('**/compute/jobs/**');
    await expect(page.locator('[data-testid="job-nft-id"]')).toBeVisible();
    await expect(page.locator('[data-testid="job-status"]')).toContainText(
      'Open'
    );
  });

  test('Accept and execute computational job', async ({ page }) => {
    // Switch back to provider
    await helpers.loginAs('compute-provider');
    await helpers.connectWallet('compute-provider');

    await page.click('[data-testid="nav-compute"]');
    await page.click('[data-testid="available-jobs-tab"]');

    // Browse available jobs
    await expect(
      page.locator('[data-testid="job-listing"]')
    ).toHaveCount.toBeGreaterThan(0);

    // Filter jobs by requirements
    await page.selectOption('[data-testid="filter-gpu-required"]', 'yes');
    await page.fill('[data-testid="filter-max-cpu-cores"]', '128');

    // Select compatible job
    await page.click('[data-testid="job-listing"]').first();

    // Review job details
    await expect(
      page.locator('[data-testid="job-requirements"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="payment-terms"]')).toBeVisible();
    await expect(page.locator('[data-testid="job-timeline"]')).toBeVisible();

    // Check resource compatibility
    await page.click('[data-testid="check-compatibility-button"]');
    await expect(
      page.locator('[data-testid="compatibility-result"]')
    ).toContainText('Compatible');

    // Accept job
    await page.click('[data-testid="accept-job-button"]');

    const modal = await helpers.waitForModal();

    // Select compute resource
    await page.selectOption(
      '[data-testid="assigned-resource"]',
      'GPU Cluster Alpha'
    );

    // Confirm resource allocation
    await helpers.fillForm({
      'allocated-cpu-cores': '64',
      'allocated-memory-gb': '256',
      'allocated-storage-gb': '5000',
      'allocated-gpu-count': '4',
    });

    // Set execution schedule
    await page.fill('[data-testid="start-time"]', '2024-07-15T10:00');
    await page.fill('[data-testid="estimated-completion"]', '2024-07-15T22:00');

    // Submit job acceptance
    await page.click('[data-testid="confirm-job-acceptance"]');

    // Authenticate
    await page.fill(
      '[data-testid="wallet-password-input"]',
      'ProviderPass123!'
    );
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Monitor job execution
    await page.waitForURL('**/compute/jobs/**/execution');
    await expect(page.locator('[data-testid="job-status"]')).toContainText(
      'In Progress'
    );
    await expect(
      page.locator('[data-testid="execution-progress"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="resource-utilization"]')
    ).toBeVisible();

    // Check real-time metrics
    await expect(page.locator('[data-testid="cpu-usage"]')).toBeVisible();
    await expect(page.locator('[data-testid="memory-usage"]')).toBeVisible();
    await expect(page.locator('[data-testid="gpu-usage"]')).toBeVisible();

    // Simulate job completion (in real scenario, this would be automatic)
    if (
      await page.locator('[data-testid="mark-complete-button"]').isVisible()
    ) {
      await page.click('[data-testid="mark-complete-button"]');

      // Upload results
      await helpers.uploadFile(
        '[data-testid="results-upload"]',
        './fixtures/hurricane-prediction-results.nc'
      );
      await page.fill(
        '[data-testid="completion-notes"]',
        'Job completed successfully. All output files generated as expected.'
      );

      await page.click('[data-testid="submit-completion"]');
      await helpers.waitForNotification('success');
    }
  });

  test('Verify and validate job results', async ({ page }) => {
    // Switch to job creator for verification
    await helpers.loginAs('researcher');
    await helpers.connectWallet('researcher');

    await page.click('[data-testid="nav-compute"]');
    await page.click('[data-testid="my-jobs-tab"]');

    // Find completed job
    await page
      .click('[data-testid="job-item"][data-status="completed"]')
      .first();

    // Review job results
    await expect(page.locator('[data-testid="job-results"]')).toBeVisible();
    await expect(page.locator('[data-testid="output-files"]')).toBeVisible();
    await expect(page.locator('[data-testid="execution-log"]')).toBeVisible();

    // Download and verify results
    await page.click('[data-testid="download-results-button"]');
    await helpers.waitForNotification('success');

    // Check result quality metrics
    await expect(
      page.locator('[data-testid="result-quality-score"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="execution-time"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="resource-efficiency"]')
    ).toBeVisible();

    // Validate results
    await page.click('[data-testid="validate-results-button"]');

    const modal = await helpers.waitForModal();

    // Provide validation feedback
    await page.selectOption('[data-testid="quality-rating"]', '5'); // Excellent
    await page.fill(
      '[data-testid="validation-comments"]',
      'Results exceed expectations. High-quality hurricane prediction with accurate trajectory modeling.'
    );

    // Approve quality bonus
    await page.check('[data-testid="approve-quality-bonus"]');

    // Submit validation
    await page.click('[data-testid="submit-validation"]');

    // Authenticate payment release
    await page.fill(
      '[data-testid="wallet-password-input"]',
      'ResearcherPass123!'
    );
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Verify payment processed
    await expect(page.locator('[data-testid="payment-status"]')).toContainText(
      'Completed'
    );
    await expect(
      page.locator('[data-testid="job-final-status"]')
    ).toContainText('Validated');
  });

  test('Manage computational resource pool', async ({ page }) => {
    await page.click('[data-testid="nav-compute"]');
    await page.click('[data-testid="resource-management-tab"]');

    // Verify resource pool interface
    await expect(page.locator('[data-testid="resource-pool"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="resource-utilization"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="performance-metrics"]')
    ).toBeVisible();

    // Monitor resource utilization
    await expect(page.locator('[data-testid="total-cpu-usage"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="total-memory-usage"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="total-gpu-usage"]')).toBeVisible();

    // Add new resource node
    await page.click('[data-testid="add-resource-node-button"]');

    const modal = await helpers.waitForModal();

    await helpers.fillForm({
      'node-name': 'Compute Node Gamma',
      'node-ip': '192.168.1.100',
      'ssh-key': 'ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQ...',
      'cpu-cores': '32',
      'memory-gb': '128',
      'storage-gb': '2000',
      'gpu-count': '2',
    });

    // Test node connectivity
    await page.click('[data-testid="test-node-connection"]');
    await page.waitForSelector('[data-testid="connection-result"]');

    const connectionResult = await page.textContent(
      '[data-testid="connection-result"]'
    );
    expect(connectionResult).toContain('success');

    // Add node to pool
    await page.click('[data-testid="add-node-to-pool"]');
    await helpers.waitForNotification('success');
    await modal.close();

    // Configure resource allocation policies
    await page.click('[data-testid="allocation-policies-tab"]');

    await page.selectOption(
      '[data-testid="allocation-strategy"]',
      'load-balanced'
    );
    await page.check('[data-testid="enable-auto-scaling"]');
    await page.fill('[data-testid="scale-up-threshold"]', '80');
    await page.fill('[data-testid="scale-down-threshold"]', '20');

    // Set resource limits
    await page.fill('[data-testid="max-cpu-per-job"]', '50');
    await page.fill('[data-testid="max-memory-per-job"]', '200');
    await page.fill('[data-testid="max-gpu-per-job"]', '2');

    // Save allocation policies
    await page.click('[data-testid="save-policies-button"]');
    await helpers.waitForNotification('success');

    // Monitor performance metrics
    await page.click('[data-testid="performance-metrics-tab"]');

    await expect(
      page.locator('[data-testid="job-completion-rate"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="average-execution-time"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="resource-efficiency"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="error-rate"]')).toBeVisible();
  });

  test('Browse computational jobs marketplace', async ({ page }) => {
    // Switch to general user browsing marketplace
    await helpers.loginAs('user1');
    await helpers.connectWallet('user1');

    await page.click('[data-testid="nav-compute"]');
    await page.click('[data-testid="marketplace-tab"]');

    // Verify marketplace interface
    await expect(
      page.locator('[data-testid="jobs-marketplace"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="featured-jobs"]')).toBeVisible();
    await expect(page.locator('[data-testid="job-categories"]')).toBeVisible();

    // Browse job categories
    await page.click('[data-testid="category-ai-ml"]');
    const aiJobs = page.locator(
      '[data-testid="job-listing"][data-category="ai-ml"]'
    );
    await expect(aiJobs).toHaveCount.toBeGreaterThan(0);

    await page.click('[data-testid="category-weather"]');
    const weatherJobs = page.locator(
      '[data-testid="job-listing"][data-category="weather"]'
    );
    await expect(weatherJobs).toHaveCount.toBeGreaterThan(0);

    // Filter jobs by payment range
    await page.fill('[data-testid="min-payment"]', '100');
    await page.fill('[data-testid="max-payment"]', '1000');
    await page.click('[data-testid="apply-payment-filter"]');

    // Sort jobs by payment
    await page.selectOption(
      '[data-testid="sort-dropdown"]',
      'payment-high-to-low'
    );
    await page.waitForTimeout(1000);

    // View job details
    await page.click('[data-testid="job-listing"]').first();

    // Verify job details page
    await expect(page.locator('[data-testid="job-title"]')).toBeVisible();
    await expect(page.locator('[data-testid="job-description"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="computational-requirements"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="payment-details"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="job-creator-info"]')
    ).toBeVisible();

    // Check if user can bid (if job allows bidding)
    if (await page.locator('[data-testid="place-bid-button"]').isVisible()) {
      await page.click('[data-testid="place-bid-button"]');

      const bidModal = await helpers.waitForModal();

      await page.fill('[data-testid="bid-amount"]', '450');
      await page.fill('[data-testid="estimated-completion"]', '10');
      await page.fill(
        '[data-testid="bid-notes"]',
        'Experienced in weather modeling with optimized GPU algorithms'
      );

      await page.click('[data-testid="submit-bid"]');
      await helpers.waitForNotification('success');
      await bidModal.close();
    }
  });

  test('Computational jobs error handling', async ({ page }) => {
    await page.click('[data-testid="nav-compute"]');

    // Test insufficient computational resources error
    await page.click('[data-testid="available-jobs-tab"]');

    if (await page.locator('[data-testid="job-listing"]').isVisible()) {
      await page.click('[data-testid="job-listing"]').first();

      // Try to accept job without sufficient resources
      if (
        await page
          .locator('[data-testid="insufficient-resources-warning"]')
          .isVisible()
      ) {
        await expect(
          page.locator('[data-testid="resource-mismatch"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="upgrade-resources-suggestion"]')
        ).toBeVisible();
      }
    }

    // Test job execution failure scenario
    if (await page.locator('[data-testid="active-job"]').isVisible()) {
      await page.click('[data-testid="active-job"]').first();

      // Simulate execution error
      if (
        await page.locator('[data-testid="report-error-button"]').isVisible()
      ) {
        await page.click('[data-testid="report-error-button"]');

        const errorModal = await helpers.waitForModal();

        await page.selectOption(
          '[data-testid="error-type"]',
          'resource-failure'
        );
        await page.fill(
          '[data-testid="error-description"]',
          'GPU memory allocation failed during model initialization'
        );

        await page.click('[data-testid="submit-error-report"]');
        await helpers.waitForNotification('success');
        await errorModal.close();
      }
    }

    // Test network connectivity issues
    await page.route('**/api/compute/**', route => route.abort());
    await page.reload();
    await expect(
      page.locator('[data-testid="compute-connection-error"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="retry-connection-button"]')
    ).toBeVisible();
  });

  test('Computational jobs accessibility and mobile responsiveness', async ({
    page,
  }) => {
    // Test keyboard navigation
    await page.keyboard.press('Tab');
    await page.keyboard.press('Tab');
    await page.keyboard.press('Enter');

    // Check accessibility violations
    const violations = await helpers.checkAccessibility();
    expect(violations).toHaveLength(0);

    // Test mobile viewport
    await page.setViewportSize({ width: 375, height: 667 });

    await page.click('[data-testid="nav-compute"]');

    // Mobile-specific elements should be visible
    await expect(
      page.locator('[data-testid="mobile-compute-nav"]')
    ).toBeVisible();

    // Test mobile job creation interface
    if (await page.locator('[data-testid="create-job-button"]').isVisible()) {
      await page.click('[data-testid="create-job-button"]');
      await expect(
        page.locator('[data-testid="mobile-job-form"]')
      ).toBeVisible();
    }
  });
});
