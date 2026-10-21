/**
 * UI Performance Testing Suite
 * Tests frontend performance including page load times, rendering performance,
 * memory usage, and user interaction responsiveness using Playwright
 */

const { test, expect, chromium } = require('@playwright/test');
const fs = require('fs');
const path = require('path');
const config = require('../test.config');

class UIPerformanceTester {
  constructor() {
    this.baseUrl = config.baseUrls.frontend;
    this.results = [];
    this.reportDir = path.join(__dirname, '../reports/performance');

    // Ensure report directory exists
    if (!fs.existsSync(this.reportDir)) {
      fs.mkdirSync(this.reportDir, { recursive: true });
    }
  }

  /**
   * Run comprehensive UI performance test
   */
  async runPerformanceTest(testConfig) {
    const {
      title,
      url,
      actions = [],
      expectations = {},
      deviceType = 'desktop',
      networkCondition = 'fast3g',
    } = testConfig;

    console.log(`🎨 Starting ${title}...`);
    console.log(`   URL: ${this.baseUrl}${url}`);
    console.log(`   Device: ${deviceType}, Network: ${networkCondition}`);

    const browser = await chromium.launch({
      headless: true,
      args: ['--no-sandbox', '--disable-dev-shm-usage'],
    });

    try {
      const context = await browser.newContext({
        ...this.getDeviceConfig(deviceType),
        recordVideo: {
          dir: path.join(this.reportDir, 'videos'),
          size: { width: 1280, height: 720 },
        },
      });

      // Set network conditions
      await this.setNetworkConditions(context, networkCondition);

      const page = await context.newPage();

      // Enable performance monitoring
      await page.addInitScript(() => {
        window.performanceObserver = new PerformanceObserver(list => {
          window.performanceEntries = window.performanceEntries || [];
          window.performanceEntries.push(...list.getEntries());
        });
        window.performanceObserver.observe({
          entryTypes: [
            'navigation',
            'paint',
            'largest-contentful-paint',
            'layout-shift',
          ],
        });
      });

      // Start performance measurement
      const startTime = Date.now();

      // Navigate to page
      const navigationStart = Date.now();
      await page.goto(`${this.baseUrl}${url}`, { waitUntil: 'networkidle' });
      const navigationEnd = Date.now();

      // Wait for page to be fully loaded
      await page.waitForLoadState('domcontentloaded');
      await page.waitForLoadState('networkidle');

      // Get initial performance metrics
      const initialMetrics = await this.collectPerformanceMetrics(page);

      // Execute user actions and measure performance
      const actionResults = [];
      for (const action of actions) {
        const actionResult = await this.executeAction(page, action);
        actionResults.push(actionResult);
      }

      // Collect final performance metrics
      const finalMetrics = await this.collectPerformanceMetrics(page);

      // Memory usage analysis
      const memoryUsage = await this.analyzeMemoryUsage(page);

      // Accessibility performance
      const accessibilityMetrics =
        await this.measureAccessibilityPerformance(page);

      // Bundle analysis
      const bundleMetrics = await this.analyzeBundlePerformance(page);

      const endTime = Date.now();

      const result = {
        title,
        url,
        deviceType,
        networkCondition,
        timing: {
          total: endTime - startTime,
          navigation: navigationEnd - navigationStart,
          ...initialMetrics.timing,
        },
        metrics: {
          initial: initialMetrics,
          final: finalMetrics,
          delta: this.calculateMetricsDelta(initialMetrics, finalMetrics),
        },
        actions: actionResults,
        memory: memoryUsage,
        accessibility: accessibilityMetrics,
        bundle: bundleMetrics,
        lighthouse: await this.runLighthouse(page),
        timestamp: new Date().toISOString(),
        passed: this.evaluateUIPerformance(
          initialMetrics,
          actionResults,
          expectations
        ),
      };

      await context.close();

      this.results.push(result);
      this.printUIResults(result);

      return result;
    } finally {
      await browser.close();
    }
  }

  /**
   * Get device configuration for testing
   */
  getDeviceConfig(deviceType) {
    const configs = {
      desktop: {
        viewport: { width: 1920, height: 1080 },
        userAgent:
          'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
      },
      tablet: {
        viewport: { width: 768, height: 1024 },
        userAgent:
          'Mozilla/5.0 (iPad; CPU OS 14_7_1 like Mac OS X) AppleWebKit/605.1.15',
        isMobile: true,
        hasTouch: true,
      },
      mobile: {
        viewport: { width: 375, height: 667 },
        userAgent:
          'Mozilla/5.0 (iPhone; CPU iPhone OS 14_7_1 like Mac OS X) AppleWebKit/605.1.15',
        isMobile: true,
        hasTouch: true,
      },
    };

    return configs[deviceType] || configs.desktop;
  }

  /**
   * Set network conditions for testing
   */
  async setNetworkConditions(context, condition) {
    const conditions = {
      fast3g: {
        downloadThroughput: (1.5 * 1024 * 1024) / 8,
        uploadThroughput: (750 * 1024) / 8,
        latency: 150,
      },
      slow3g: {
        downloadThroughput: (500 * 1024) / 8,
        uploadThroughput: (500 * 1024) / 8,
        latency: 300,
      },
      offline: { downloadThroughput: 0, uploadThroughput: 0, latency: 0 },
    };

    if (conditions[condition]) {
      // Simulate network conditions using CDP
      const page = await context.newPage();
      const client = await page.context().newCDPSession(page);
      await client.send(
        'Network.emulateNetworkConditions',
        conditions[condition]
      );
      await page.close();
    }
  }

  /**
   * Collect comprehensive performance metrics
   */
  async collectPerformanceMetrics(page) {
    // Web Vitals and performance timing
    const metrics = await page.evaluate(() => {
      const navigation = performance.getEntriesByType('navigation')[0];
      const paint = performance.getEntriesByType('paint');
      const lcp = performance.getEntriesByType('largest-contentful-paint')[0];
      const cls = performance.getEntriesByType('layout-shift');

      // Calculate Cumulative Layout Shift
      const clsScore = cls.reduce((sum, entry) => {
        if (!entry.hadRecentInput) {
          return sum + entry.value;
        }
        return sum;
      }, 0);

      return {
        timing: {
          domContentLoaded: navigation
            ? navigation.domContentLoadedEventEnd -
              navigation.domContentLoadedEventStart
            : 0,
          loadComplete: navigation
            ? navigation.loadEventEnd - navigation.loadEventStart
            : 0,
          firstPaint: paint.find(p => p.name === 'first-paint')?.startTime || 0,
          firstContentfulPaint:
            paint.find(p => p.name === 'first-contentful-paint')?.startTime ||
            0,
          largestContentfulPaint: lcp ? lcp.startTime : 0,
          cumulativeLayoutShift: clsScore,
        },
        resources: {
          totalResources: performance.getEntriesByType('resource').length,
          imageResources: performance
            .getEntriesByType('resource')
            .filter(r => r.initiatorType === 'img').length,
          scriptResources: performance
            .getEntriesByType('resource')
            .filter(r => r.initiatorType === 'script').length,
          stylesheetResources: performance
            .getEntriesByType('resource')
            .filter(r => r.initiatorType === 'link').length,
        },
        performanceEntries: window.performanceEntries || [],
      };
    });

    // Runtime performance metrics
    const runtimeMetrics = await page.evaluate(() => {
      if ('memory' in performance) {
        return {
          memory: {
            usedJSHeapSize: performance.memory.usedJSHeapSize,
            totalJSHeapSize: performance.memory.totalJSHeapSize,
            jsHeapSizeLimit: performance.memory.jsHeapSizeLimit,
          },
        };
      }
      return { memory: null };
    });

    return { ...metrics, ...runtimeMetrics };
  }

  /**
   * Execute user action and measure performance impact
   */
  async executeAction(page, action) {
    const { type, selector, value, description } = action;

    console.log(`   📱 Executing: ${description || type}`);

    const startTime = Date.now();
    const startMetrics = await this.collectPerformanceMetrics(page);

    try {
      switch (type) {
        case 'click':
          await page.click(selector, { timeout: 10000 });
          break;
        case 'type':
          await page.fill(selector, value);
          break;
        case 'scroll':
          await page.evaluate(distance => {
            window.scrollBy(0, distance);
          }, value || 500);
          break;
        case 'hover':
          await page.hover(selector);
          break;
        case 'navigate':
          await page.goto(`${this.baseUrl}${value}`);
          break;
        default:
          throw new Error(`Unknown action type: ${type}`);
      }

      // Wait for potential re-renders
      await page.waitForTimeout(1000);
      await page.waitForLoadState('networkidle');

      const endTime = Date.now();
      const endMetrics = await this.collectPerformanceMetrics(page);

      return {
        action: description || type,
        duration: endTime - startTime,
        success: true,
        performanceImpact: {
          memoryIncrease:
            endMetrics.memory?.usedJSHeapSize -
              startMetrics.memory?.usedJSHeapSize || 0,
          layoutShiftIncrease:
            endMetrics.timing.cumulativeLayoutShift -
            startMetrics.timing.cumulativeLayoutShift,
          resourcesAdded:
            endMetrics.resources.totalResources -
            startMetrics.resources.totalResources,
        },
      };
    } catch (error) {
      const endTime = Date.now();

      return {
        action: description || type,
        duration: endTime - startTime,
        success: false,
        error: error.message,
        performanceImpact: null,
      };
    }
  }

  /**
   * Analyze memory usage patterns
   */
  async analyzeMemoryUsage(page) {
    const memoryInfo = await page.evaluate(() => {
      if ('memory' in performance) {
        return {
          current: {
            used: performance.memory.usedJSHeapSize,
            total: performance.memory.totalJSHeapSize,
            limit: performance.memory.jsHeapSizeLimit,
          },
          percentage:
            (performance.memory.usedJSHeapSize /
              performance.memory.jsHeapSizeLimit) *
            100,
        };
      }
      return null;
    });

    // Force garbage collection if possible and measure again
    try {
      await page.evaluate(() => {
        if (window.gc) {
          window.gc();
        }
      });

      const postGCMemory = await page.evaluate(() => {
        if ('memory' in performance) {
          return performance.memory.usedJSHeapSize;
        }
        return null;
      });

      if (memoryInfo && postGCMemory !== null) {
        memoryInfo.afterGC = postGCMemory;
        memoryInfo.garbageCollected = memoryInfo.current.used - postGCMemory;
      }
    } catch (error) {
      // GC not available, that's okay
    }

    return memoryInfo;
  }

  /**
   * Measure accessibility performance
   */
  async measureAccessibilityPerformance(page) {
    const accessibilityMetrics = await page.evaluate(() => {
      const elements = document.querySelectorAll('*');
      let violations = 0;
      let focusableElements = 0;
      let elementsWithAria = 0;
      let imagesWithoutAlt = 0;

      elements.forEach(el => {
        // Check for focusable elements
        if (
          el.tabIndex >= 0 ||
          el.matches('a, button, input, select, textarea')
        ) {
          focusableElements++;
        }

        // Check for ARIA attributes
        if (
          el.hasAttribute('aria-label') ||
          el.hasAttribute('aria-labelledby') ||
          el.hasAttribute('role')
        ) {
          elementsWithAria++;
        }

        // Check for images without alt text
        if (el.tagName === 'IMG' && !el.hasAttribute('alt')) {
          imagesWithoutAlt++;
          violations++;
        }

        // Check for missing form labels
        if (
          el.tagName === 'INPUT' &&
          el.type !== 'hidden' &&
          !el.hasAttribute('aria-label') &&
          !el.hasAttribute('aria-labelledby')
        ) {
          const label = document.querySelector(`label[for="${el.id}"]`);
          if (!label && !el.closest('label')) {
            violations++;
          }
        }
      });

      return {
        totalElements: elements.length,
        focusableElements,
        elementsWithAria,
        imagesWithoutAlt,
        violations,
        accessibilityScore: Math.max(
          0,
          100 - (violations / elements.length) * 100
        ),
      };
    });

    return accessibilityMetrics;
  }

  /**
   * Analyze bundle performance and resource loading
   */
  async analyzeBundlePerformance(page) {
    const bundleMetrics = await page.evaluate(() => {
      const resources = performance.getEntriesByType('resource');

      let totalSize = 0;
      let jsSize = 0;
      let cssSize = 0;
      let imageSize = 0;
      let slowResources = 0;

      const resourcesByType = {
        script: [],
        stylesheet: [],
        image: [],
        other: [],
      };

      resources.forEach(resource => {
        const size = resource.transferSize || resource.encodedBodySize || 0;
        totalSize += size;

        if (resource.duration > 1000) {
          slowResources++;
        }

        if (
          resource.name.includes('.js') ||
          resource.initiatorType === 'script'
        ) {
          jsSize += size;
          resourcesByType.script.push({
            name: resource.name,
            size,
            duration: resource.duration,
          });
        } else if (
          resource.name.includes('.css') ||
          resource.initiatorType === 'link'
        ) {
          cssSize += size;
          resourcesByType.stylesheet.push({
            name: resource.name,
            size,
            duration: resource.duration,
          });
        } else if (resource.initiatorType === 'img') {
          imageSize += size;
          resourcesByType.image.push({
            name: resource.name,
            size,
            duration: resource.duration,
          });
        } else {
          resourcesByType.other.push({
            name: resource.name,
            size,
            duration: resource.duration,
          });
        }
      });

      return {
        totalSize: Math.round(totalSize / 1024), // KB
        jsSize: Math.round(jsSize / 1024),
        cssSize: Math.round(cssSize / 1024),
        imageSize: Math.round(imageSize / 1024),
        resourceCount: resources.length,
        slowResources,
        resourcesByType,
        compressionRatio:
          totalSize > 0
            ? resources.reduce((sum, r) => sum + (r.decodedBodySize || 0), 0) /
              totalSize
            : 1,
      };
    });

    return bundleMetrics;
  }

  /**
   * Run basic Lighthouse-style performance audit
   */
  async runLighthouse(page) {
    // Simplified Lighthouse-style scoring
    const metrics = await this.collectPerformanceMetrics(page);

    const scores = {
      firstContentfulPaint: this.scoreMetric(
        metrics.timing.firstContentfulPaint,
        [1800, 3000]
      ),
      largestContentfulPaint: this.scoreMetric(
        metrics.timing.largestContentfulPaint,
        [2500, 4000]
      ),
      cumulativeLayoutShift: this.scoreMetric(
        metrics.timing.cumulativeLayoutShift,
        [0.1, 0.25],
        true
      ),
      domContentLoaded: this.scoreMetric(
        metrics.timing.domContentLoaded,
        [1500, 2500]
      ),
    };

    const overallScore =
      Object.values(scores).reduce((sum, score) => sum + score, 0) /
      Object.keys(scores).length;

    return {
      scores,
      overallScore: Math.round(overallScore),
      category:
        overallScore >= 90
          ? 'Good'
          : overallScore >= 50
            ? 'Needs Improvement'
            : 'Poor',
    };
  }

  /**
   * Score individual metrics like Lighthouse
   */
  scoreMetric(value, thresholds, inverse = false) {
    if (!value) return 0;

    const [good, poor] = thresholds;

    if (inverse) {
      if (value <= good) return 100;
      if (value >= poor) return 0;
      return Math.round(100 - ((value - good) / (poor - good)) * 100);
    } else {
      if (value <= good) return 100;
      if (value >= poor) return 0;
      return Math.round(100 - ((value - good) / (poor - good)) * 100);
    }
  }

  /**
   * Calculate performance metrics delta
   */
  calculateMetricsDelta(initial, final) {
    return {
      memoryIncrease:
        (final.memory?.usedJSHeapSize || 0) -
        (initial.memory?.usedJSHeapSize || 0),
      layoutShiftIncrease:
        final.timing.cumulativeLayoutShift -
        initial.timing.cumulativeLayoutShift,
      resourcesAdded:
        final.resources.totalResources - initial.resources.totalResources,
    };
  }

  /**
   * Evaluate UI performance against expectations
   */
  evaluateUIPerformance(metrics, actions, expectations) {
    const checks = [];

    if (expectations.maxFCP) {
      checks.push({
        check: 'First Contentful Paint',
        expected: `<${expectations.maxFCP}ms`,
        actual: `${metrics.timing.firstContentfulPaint}ms`,
        passed: metrics.timing.firstContentfulPaint <= expectations.maxFCP,
      });
    }

    if (expectations.maxLCP) {
      checks.push({
        check: 'Largest Contentful Paint',
        expected: `<${expectations.maxLCP}ms`,
        actual: `${metrics.timing.largestContentfulPaint}ms`,
        passed: metrics.timing.largestContentfulPaint <= expectations.maxLCP,
      });
    }

    if (expectations.maxCLS) {
      checks.push({
        check: 'Cumulative Layout Shift',
        expected: `<${expectations.maxCLS}`,
        actual: metrics.timing.cumulativeLayoutShift.toFixed(3),
        passed: metrics.timing.cumulativeLayoutShift <= expectations.maxCLS,
      });
    }

    if (expectations.maxMemoryUsage && metrics.memory) {
      const memoryUsageMB = metrics.memory.usedJSHeapSize / (1024 * 1024);
      checks.push({
        check: 'Memory Usage',
        expected: `<${expectations.maxMemoryUsage}MB`,
        actual: `${memoryUsageMB.toFixed(2)}MB`,
        passed: memoryUsageMB <= expectations.maxMemoryUsage,
      });
    }

    return checks;
  }

  /**
   * Print UI performance results
   */
  printUIResults(result) {
    console.log(`\n🎨 ${result.title} Results:`);
    console.log(
      `   FCP: ${result.metrics.initial.timing.firstContentfulPaint}ms`
    );
    console.log(
      `   LCP: ${result.metrics.initial.timing.largestContentfulPaint}ms`
    );
    console.log(
      `   CLS: ${result.metrics.initial.timing.cumulativeLayoutShift.toFixed(3)}`
    );
    console.log(
      `   Bundle Size: ${result.bundle.totalSize}KB (JS: ${result.bundle.jsSize}KB, CSS: ${result.bundle.cssSize}KB)`
    );
    console.log(
      `   Lighthouse Score: ${result.lighthouse.overallScore}/100 (${result.lighthouse.category})`
    );

    if (result.memory) {
      console.log(
        `   Memory Usage: ${(result.memory.current.used / (1024 * 1024)).toFixed(2)}MB`
      );
    }

    if (result.passed && result.passed.length > 0) {
      console.log(`   Performance Checks:`);
      result.passed.forEach(check => {
        const status = check.passed ? '✅' : '❌';
        console.log(
          `     ${status} ${check.check}: ${check.actual} (expected: ${check.expected})`
        );
      });
    }
  }

  /**
   * Generate UI performance report
   */
  generateUIReport() {
    const reportData = {
      summary: {
        totalTests: this.results.length,
        timestamp: new Date().toISOString(),
        averageLighthouseScore:
          this.results.reduce((sum, r) => sum + r.lighthouse.overallScore, 0) /
          this.results.length,
        performanceCategory: this.assessOverallPerformance(),
        recommendations: this.generateUIRecommendations(),
      },
      tests: this.results,
    };

    // Save JSON report
    const jsonReport = path.join(this.reportDir, 'ui-performance-results.json');
    fs.writeFileSync(jsonReport, JSON.stringify(reportData, null, 2));

    console.log(`\n📋 UI performance report generated: ${jsonReport}`);
    return reportData;
  }

  /**
   * Assess overall UI performance
   */
  assessOverallPerformance() {
    const avgScore =
      this.results.reduce((sum, r) => sum + r.lighthouse.overallScore, 0) /
      this.results.length;

    if (avgScore >= 90) return 'Excellent';
    if (avgScore >= 70) return 'Good';
    if (avgScore >= 50) return 'Needs Improvement';
    return 'Poor';
  }

  /**
   * Generate UI-specific recommendations
   */
  generateUIRecommendations() {
    const recommendations = [];

    this.results.forEach(result => {
      if (result.metrics.initial.timing.firstContentfulPaint > 2000) {
        recommendations.push({
          page: result.url,
          issue: 'Slow First Contentful Paint',
          recommendation:
            'Optimize critical rendering path, reduce blocking resources, enable server-side rendering',
        });
      }

      if (result.bundle.totalSize > 1000) {
        recommendations.push({
          page: result.url,
          issue: 'Large Bundle Size',
          recommendation:
            'Implement code splitting, tree shaking, and lazy loading. Consider bundle analysis.',
        });
      }

      if (result.metrics.initial.timing.cumulativeLayoutShift > 0.1) {
        recommendations.push({
          page: result.url,
          issue: 'Layout Shift Issues',
          recommendation:
            'Add size attributes to images, reserve space for dynamic content, avoid inserting content above existing content',
        });
      }

      if (result.memory && result.memory.percentage > 80) {
        recommendations.push({
          page: result.url,
          issue: 'High Memory Usage',
          recommendation:
            'Review memory leaks, optimize large objects, implement virtual scrolling for large lists',
        });
      }
    });

    return recommendations;
  }
}

// Export for use in other test files
module.exports = {
  UIPerformanceTester,
};

// Main execution for UI performance tests
async function runUIPerformanceTests() {
  const tester = new UIPerformanceTester();

  console.log('🎨 Starting WRFCoin UI Performance Testing Suite\n');

  try {
    // Test 1: Dashboard page performance
    await tester.runPerformanceTest({
      title: 'Dashboard Page Performance',
      url: '/dashboard',
      actions: [
        {
          type: 'click',
          selector: '[data-testid="weather-stations-tab"]',
          description: 'Switch to weather stations',
        },
        { type: 'scroll', value: 500, description: 'Scroll down page' },
        {
          type: 'click',
          selector: '[data-testid="refresh-data-button"]',
          description: 'Refresh dashboard data',
        },
      ],
      expectations: {
        maxFCP: 2000,
        maxLCP: 3000,
        maxCLS: 0.1,
        maxMemoryUsage: 50,
      },
    });

    // Test 2: NFT Marketplace performance
    await tester.runPerformanceTest({
      title: 'NFT Marketplace Performance',
      url: '/nft-marketplace',
      actions: [
        {
          type: 'click',
          selector: '[data-testid="filter-premium"]',
          description: 'Filter premium NFTs',
        },
        { type: 'scroll', value: 1000, description: 'Scroll through NFT grid' },
        {
          type: 'click',
          selector: '[data-testid="nft-card"]',
          description: 'Open NFT details',
        },
      ],
      expectations: {
        maxFCP: 2500,
        maxLCP: 4000,
        maxCLS: 0.15,
        maxMemoryUsage: 75,
      },
    });

    // Test 3: Wallet page performance
    await tester.runPerformanceTest({
      title: 'Wallet Page Performance',
      url: '/wallet',
      actions: [
        {
          type: 'click',
          selector: '[data-testid="transaction-history-tab"]',
          description: 'Switch to transaction history',
        },
        {
          type: 'click',
          selector: '[data-testid="multi-chain-view"]',
          description: 'Switch to multi-chain view',
        },
        {
          type: 'type',
          selector: '[data-testid="search-transactions"]',
          value: 'NFT',
          description: 'Search transactions',
        },
      ],
      expectations: {
        maxFCP: 1800,
        maxLCP: 2500,
        maxCLS: 0.05,
        maxMemoryUsage: 40,
      },
    });

    // Test 4: Governance page performance
    await tester.runPerformanceTest({
      title: 'Governance Page Performance',
      url: '/governance',
      actions: [
        { type: 'scroll', value: 800, description: 'Scroll through proposals' },
        {
          type: 'click',
          selector: '[data-testid="proposal-card"]',
          description: 'Open proposal details',
        },
        {
          type: 'navigate',
          value: '/governance/create',
          description: 'Navigate to create proposal',
        },
      ],
      expectations: {
        maxFCP: 2200,
        maxLCP: 3500,
        maxCLS: 0.1,
        maxMemoryUsage: 60,
      },
    });

    // Test 5: Mobile performance test
    await tester.runPerformanceTest({
      title: 'Mobile Dashboard Performance',
      url: '/dashboard',
      deviceType: 'mobile',
      networkCondition: 'slow3g',
      actions: [
        { type: 'scroll', value: 300, description: 'Mobile scroll' },
        {
          type: 'click',
          selector: '[data-testid="mobile-menu-button"]',
          description: 'Open mobile menu',
        },
      ],
      expectations: {
        maxFCP: 4000,
        maxLCP: 6000,
        maxCLS: 0.2,
        maxMemoryUsage: 30,
      },
    });

    // Generate comprehensive report
    const report = tester.generateUIReport();

    console.log('\n🎨 UI Performance Testing Complete!');
    console.log(
      `📊 Overall Performance: ${report.summary.performanceCategory}`
    );
    console.log(
      `🚀 Average Lighthouse Score: ${report.summary.averageLighthouseScore.toFixed(1)}/100`
    );

    if (report.summary.recommendations.length > 0) {
      console.log(
        `⚠️  Found ${report.summary.recommendations.length} UI optimization opportunities`
      );
    }

    return report;
  } catch (error) {
    console.error('❌ UI performance testing failed:', error);
    throw error;
  }
}

// Run tests if called directly
if (require.main === module) {
  runUIPerformanceTests()
    .then(report => {
      process.exit(report.summary.performanceCategory !== 'Poor' ? 0 : 1);
    })
    .catch(error => {
      console.error('Test execution failed:', error);
      process.exit(1);
    });
}
