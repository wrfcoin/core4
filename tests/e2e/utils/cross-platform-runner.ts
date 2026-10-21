// Cross-Platform Test Runner for WRFCoin
import { test as base, expect, Page, Browser } from '@playwright/test';
import { TestDataLifecycleManager } from '../test-data-management/data-lifecycle';
import { MockServiceManager } from '../test-data-management/mock-services';
import {
  BROWSER_CONFIGS,
  DEVICE_CAPABILITIES,
  PERFORMANCE_BENCHMARKS,
} from '../config/cross-browser-config';

export interface CrossPlatformTestContext {
  sessionId: string;
  lifecycleManager: TestDataLifecycleManager;
  mockManager: MockServiceManager;
  platformInfo: PlatformInfo;
  performanceTracker: PerformanceTracker;
}

export interface PlatformInfo {
  browserName: string;
  deviceType: 'desktop' | 'mobile' | 'tablet';
  viewport: { width: number; height: number };
  userAgent: string;
  hasTouch: boolean;
  networkCondition?: string;
}

export interface PerformanceMetrics {
  pageLoadTime: number;
  firstContentfulPaint: number;
  largestContentfulPaint: number;
  timeToInteractive: number;
  cumulativeLayoutShift: number;
}

export class PerformanceTracker {
  private page: Page;
  private metrics: Map<string, PerformanceMetrics> = new Map();

  constructor(page: Page) {
    this.page = page;
  }

  async startTracking(testName: string): Promise<void> {
    await this.page.addInitScript(() => {
      window.performance.mark('test-start');
    });
  }

  async measurePageLoad(
    testName: string,
    url: string
  ): Promise<PerformanceMetrics> {
    const startTime = Date.now();

    await this.page.goto(url, { waitUntil: 'networkidle' });

    const metrics = await this.page.evaluate(() => {
      const navigation = performance.getEntriesByType(
        'navigation'
      )[0] as PerformanceNavigationTiming;
      const paint = performance.getEntriesByType('paint');

      return {
        pageLoadTime: Date.now() - performance.timeOrigin,
        firstContentfulPaint:
          paint.find(p => p.name === 'first-contentful-paint')?.startTime || 0,
        largestContentfulPaint: 0, // Would need LCP observer
        timeToInteractive:
          navigation.domContentLoadedEventEnd - navigation.navigationStart,
        cumulativeLayoutShift: 0, // Would need CLS observer
      };
    });

    // Get additional metrics using Page.metrics if in Chromium
    if (this.page.context().browser()?.browserType().name() === 'chromium') {
      try {
        const pageMetrics = await (this.page as any).metrics?.();
        if (pageMetrics) {
          // Add Chromium-specific metrics
          metrics.pageLoadTime =
            pageMetrics.TaskDuration || metrics.pageLoadTime;
        }
      } catch (e) {
        // Metrics API not available, continue with existing metrics
      }
    }

    this.metrics.set(testName, metrics);
    return metrics;
  }

  async validatePerformance(
    testName: string,
    platformInfo: PlatformInfo
  ): Promise<boolean> {
    const metrics = this.metrics.get(testName);
    if (!metrics) return false;

    // Get appropriate benchmarks based on platform
    let benchmarks = PERFORMANCE_BENCHMARKS['desktop-standard'];

    if (platformInfo.deviceType === 'mobile') {
      benchmarks =
        platformInfo.viewport.width > 400
          ? PERFORMANCE_BENCHMARKS['mobile-high-end']
          : PERFORMANCE_BENCHMARKS['mobile-low-end'];
    } else if (platformInfo.deviceType === 'tablet') {
      benchmarks = PERFORMANCE_BENCHMARKS['tablet'];
    } else if (platformInfo.viewport.width >= 2560) {
      benchmarks = PERFORMANCE_BENCHMARKS['desktop-high-end'];
    }

    const validations = {
      pageLoad: metrics.pageLoadTime <= benchmarks.pageLoadTime,
      fcp: metrics.firstContentfulPaint <= benchmarks.firstContentfulPaint,
      lcp: metrics.largestContentfulPaint <= benchmarks.largestContentfulPaint,
      tti: metrics.timeToInteractive <= benchmarks.timeToInteractive,
      cls: metrics.cumulativeLayoutShift <= benchmarks.cumulativeLayoutShift,
    };

    const allValid = Object.values(validations).every(valid => valid);

    if (!allValid) {
      console.warn(
        `Performance validation failed for ${testName} on ${platformInfo.browserName}:`,
        {
          metrics,
          benchmarks,
          validations,
        }
      );
    }

    return allValid;
  }

  getMetrics(testName: string): PerformanceMetrics | undefined {
    return this.metrics.get(testName);
  }

  getAllMetrics(): Map<string, PerformanceMetrics> {
    return this.metrics;
  }
}

// Enhanced test fixture with cross-platform capabilities
export const test = base.extend<CrossPlatformTestContext>({
  sessionId: async ({}, use) => {
    const sessionId = `cross-platform-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
    await use(sessionId);
  },

  lifecycleManager: async ({}, use) => {
    const manager = TestDataLifecycleManager.getInstance();
    await use(manager);
  },

  mockManager: async ({ page }, use) => {
    const manager = new MockServiceManager(page);
    await use(manager);
  },

  platformInfo: async ({ browserName, page }, use) => {
    const viewport = page.viewportSize() || { width: 1920, height: 1080 };
    const userAgent = await page.evaluate(() => navigator.userAgent);

    let deviceType: 'desktop' | 'mobile' | 'tablet';
    if (viewport.width <= 768) {
      deviceType = 'mobile';
    } else if (viewport.width <= 1024) {
      deviceType = 'tablet';
    } else {
      deviceType = 'desktop';
    }

    const hasTouch = await page.evaluate(() => 'ontouchstart' in window);

    await use({
      browserName: browserName || 'chromium',
      deviceType,
      viewport,
      userAgent,
      hasTouch,
    });
  },

  performanceTracker: async ({ page }, use) => {
    const tracker = new PerformanceTracker(page);
    await use(tracker);
  },
});

// Cross-platform test utilities
export class CrossPlatformUtils {
  // Detect platform-specific behaviors
  static async detectPlatformCapabilities(page: Page): Promise<{
    supportsWebGL: boolean;
    supportsLocalStorage: boolean;
    supportsServiceWorker: boolean;
    supportsWebAssembly: boolean;
    supportsCrypto: boolean;
    supportsGeolocation: boolean;
  }> {
    return await page.evaluate(() => {
      return {
        supportsWebGL: !!window.WebGLRenderingContext,
        supportsLocalStorage: typeof Storage !== 'undefined',
        supportsServiceWorker: 'serviceWorker' in navigator,
        supportsWebAssembly: typeof WebAssembly === 'object',
        supportsCrypto: !!window.crypto && !!window.crypto.subtle,
        supportsGeolocation: !!navigator.geolocation,
      };
    });
  }

  // Handle platform-specific interactions
  static async adaptInteraction(
    page: Page,
    selector: string,
    action: 'click' | 'hover' | 'focus',
    platformInfo: PlatformInfo
  ): Promise<void> {
    const element = page.locator(selector);

    if (platformInfo.hasTouch && action === 'hover') {
      // On touch devices, hover is often equivalent to tap
      await element.tap();
    } else if (platformInfo.hasTouch && action === 'click') {
      await element.tap();
    } else {
      switch (action) {
        case 'click':
          await element.click();
          break;
        case 'hover':
          await element.hover();
          break;
        case 'focus':
          await element.focus();
          break;
      }
    }
  }

  // Wait for platform-specific conditions
  static async waitForPlatformReady(
    page: Page,
    platformInfo: PlatformInfo
  ): Promise<void> {
    // Wait for basic page readiness
    await page.waitForLoadState('networkidle');

    // Platform-specific waits
    if (platformInfo.browserName === 'webkit') {
      // Safari needs extra time for some rendering
      await page.waitForTimeout(500);
    }

    if (platformInfo.deviceType === 'mobile') {
      // Mobile devices need extra time for touch event handlers
      await page.waitForTimeout(300);
    }

    // Wait for Web3 providers if needed
    if (page.url().includes('wallet') || page.url().includes('defi')) {
      await page
        .waitForFunction(
          () => {
            return window.ethereum !== undefined || window.web3 !== undefined;
          },
          { timeout: 5000 }
        )
        .catch(() => {
          // Web3 provider not available, continue
        });
    }
  }

  // Capture platform-specific screenshots
  static async captureScreenshot(
    page: Page,
    testName: string,
    platformInfo: PlatformInfo,
    options: { fullPage?: boolean; animations?: 'disabled' | 'allow' } = {}
  ): Promise<Buffer> {
    // Disable animations for consistent screenshots
    if (options.animations === 'disabled') {
      await page.addStyleTag({
        content: `
          *, *::before, *::after {
            animation-duration: 0s !important;
            animation-delay: 0s !important;
            transition-duration: 0s !important;
            transition-delay: 0s !important;
          }
        `,
      });
    }

    const filename = `${testName}-${platformInfo.browserName}-${platformInfo.deviceType}-${platformInfo.viewport.width}x${platformInfo.viewport.height}`;

    return await page.screenshot({
      fullPage: options.fullPage,
      path: `test-results/screenshots/${filename}.png`,
    });
  }

  // Validate responsive design
  static async validateResponsiveDesign(
    page: Page,
    selector: string,
    expectedBehavior: {
      mobile?: { visible: boolean; width?: string; height?: string };
      tablet?: { visible: boolean; width?: string; height?: string };
      desktop?: { visible: boolean; width?: string; height?: string };
    }
  ): Promise<boolean> {
    const element = page.locator(selector);
    const viewport = page.viewportSize() || { width: 1920, height: 1080 };

    let deviceType: keyof typeof expectedBehavior;
    if (viewport.width <= 768) {
      deviceType = 'mobile';
    } else if (viewport.width <= 1024) {
      deviceType = 'tablet';
    } else {
      deviceType = 'desktop';
    }

    const expected = expectedBehavior[deviceType];
    if (!expected) return true; // No expectations for this device type

    try {
      if (expected.visible) {
        await expect(element).toBeVisible();
      } else {
        await expect(element).toBeHidden();
      }

      if (expected.width) {
        const width = await element.evaluate(el => getComputedStyle(el).width);
        expect(width).toBe(expected.width);
      }

      if (expected.height) {
        const height = await element.evaluate(
          el => getComputedStyle(el).height
        );
        expect(height).toBe(expected.height);
      }

      return true;
    } catch (error) {
      console.error(`Responsive validation failed for ${deviceType}:`, error);
      return false;
    }
  }

  // Handle browser-specific workarounds
  static async applyBrowserWorkarounds(
    page: Page,
    browserName: string
  ): Promise<void> {
    switch (browserName) {
      case 'webkit':
        // Safari-specific workarounds
        await page.addInitScript(() => {
          // Mock Web3 provider for Safari testing
          if (!window.ethereum) {
            (window as any).ethereum = {
              isMetaMask: true,
              request: async ({ method }: { method: string }) => {
                switch (method) {
                  case 'eth_accounts':
                    return ['0x742d35Cc6634C0532925a3b8D1c9e37C2384C24D'];
                  case 'eth_chainId':
                    return '0x1';
                  default:
                    return null;
                }
              },
            };
          }
        });
        break;

      case 'firefox':
        // Firefox-specific workarounds
        await page.addInitScript(() => {
          // Polyfill for missing APIs
          if (!window.crypto.randomUUID) {
            (window.crypto as any).randomUUID = () => {
              return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(
                /[xy]/g,
                function (c) {
                  const r = (Math.random() * 16) | 0;
                  const v = c === 'x' ? r : (r & 0x3) | 0x8;
                  return v.toString(16);
                }
              );
            };
          }
        });
        break;

      case 'chromium':
        // Chrome-specific optimizations
        await page.addInitScript(() => {
          // Enable experimental features for testing
          if ('serviceWorker' in navigator) {
            // Register service worker for PWA testing
            navigator.serviceWorker.register('/sw.js').catch(() => {
              // Service worker registration failed, continue
            });
          }
        });
        break;
    }
  }

  // Network condition simulation helpers
  static async simulateNetworkConditions(
    page: Page,
    condition: 'slow-3g' | 'fast-3g' | '4g' | 'wifi' | 'offline'
  ): Promise<void> {
    const networkConditions = {
      'slow-3g': {
        offline: false,
        downloadThroughput: (500 * 1024) / 8,
        uploadThroughput: (500 * 1024) / 8,
        latency: 2000,
      },
      'fast-3g': {
        offline: false,
        downloadThroughput: (1.5 * 1024 * 1024) / 8,
        uploadThroughput: (750 * 1024) / 8,
        latency: 562.5,
      },
      '4g': {
        offline: false,
        downloadThroughput: (4 * 1024 * 1024) / 8,
        uploadThroughput: (3 * 1024 * 1024) / 8,
        latency: 20,
      },
      wifi: {
        offline: false,
        downloadThroughput: (30 * 1024 * 1024) / 8,
        uploadThroughput: (15 * 1024 * 1024) / 8,
        latency: 2,
      },
      offline: {
        offline: true,
        downloadThroughput: 0,
        uploadThroughput: 0,
        latency: 0,
      },
    };

    const condition_config = networkConditions[condition];

    // Use CDP if available (Chromium)
    const context = page.context();
    try {
      await (context as any)
        .newCDPSession(page)
        .send('Network.emulateNetworkConditions', {
          offline: condition_config.offline,
          latency: condition_config.latency,
          downloadThroughput: condition_config.downloadThroughput,
          uploadThroughput: condition_config.uploadThroughput,
        });
    } catch (error) {
      // CDP not available, simulate with delays
      if (condition === 'slow-3g' || condition === 'fast-3g') {
        await page.route('**/*', async route => {
          await new Promise(resolve =>
            setTimeout(resolve, condition_config.latency)
          );
          await route.continue();
        });
      }
    }
  }

  // Accessibility testing helpers
  static async validateAccessibility(
    page: Page,
    selector?: string
  ): Promise<{ violations: any[]; passes: any[] }> {
    // Inject axe-core for accessibility testing
    await page.addScriptTag({
      url: 'https://unpkg.com/axe-core@latest/axe.min.js',
    });

    const results = await page.evaluate(elementSelector => {
      return new Promise(resolve => {
        const config = {
          rules: {
            'color-contrast': { enabled: true },
            'keyboard-navigation': { enabled: true },
            'focus-management': { enabled: true },
          },
        };

        const context = elementSelector
          ? { include: [elementSelector] }
          : document;

        (window as any).axe.run(context, config, (err: any, results: any) => {
          if (err) resolve({ violations: [err], passes: [] });
          else
            resolve({ violations: results.violations, passes: results.passes });
        });
      });
    }, selector);

    return results as { violations: any[]; passes: any[] };
  }

  // Test data helpers for cross-platform consistency
  static async ensureDataConsistency(
    page: Page,
    testData: any
  ): Promise<boolean> {
    // Verify localStorage works
    await page.evaluate(data => {
      localStorage.setItem('test-data-check', JSON.stringify(data));
    }, testData);

    const retrievedData = await page.evaluate(() => {
      const data = localStorage.getItem('test-data-check');
      localStorage.removeItem('test-data-check');
      return data ? JSON.parse(data) : null;
    });

    return JSON.stringify(testData) === JSON.stringify(retrievedData);
  }
}

export { expect };
