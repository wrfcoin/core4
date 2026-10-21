// Cross-Browser Testing Configuration for WRFCoin Platform
import { PlaywrightTestConfig } from '@playwright/test';

export interface BrowserTestConfig {
  name: string;
  projects: string[];
  description: string;
  tags: string[];
  timeout: number;
  parallelWorkers: number;
}

export interface DeviceCapability {
  viewport: { width: number; height: number };
  userAgent: string;
  deviceScaleFactor: number;
  isMobile: boolean;
  hasTouch: boolean;
  defaultBrowserType: 'chromium' | 'firefox' | 'webkit';
}

export interface NetworkCondition {
  name: string;
  downloadThroughput: number; // bytes per second
  uploadThroughput: number; // bytes per second
  latency: number; // milliseconds
}

// Browser Test Configurations
export const BROWSER_CONFIGS: Record<string, BrowserTestConfig> = {
  // Quick smoke tests across main browsers
  smoke: {
    name: 'Cross-Browser Smoke Tests',
    projects: [
      'chromium-desktop-hd',
      'firefox-desktop-hd',
      'webkit-desktop-hd',
      'mobile-safari-iphone-12',
      'mobile-chrome-pixel-5',
    ],
    description:
      'Quick validation across primary browser and device combinations',
    tags: ['smoke', 'cross-browser', 'fast'],
    timeout: 30000,
    parallelWorkers: 5,
  },

  // Comprehensive cross-browser testing
  full: {
    name: 'Full Cross-Browser Testing',
    projects: [
      'chromium-desktop-4k',
      'chromium-desktop-hd',
      'chromium-desktop-standard',
      'firefox-desktop-hd',
      'firefox-desktop-standard',
      'webkit-desktop-hd',
      'edge-desktop-hd',
    ],
    description: 'Complete desktop browser testing across all resolutions',
    tags: ['comprehensive', 'desktop', 'all-browsers'],
    timeout: 60000,
    parallelWorkers: 4,
  },

  // Mobile-focused testing
  mobile: {
    name: 'Mobile Device Testing',
    projects: [
      'mobile-safari-iphone-14',
      'mobile-safari-iphone-13',
      'mobile-safari-iphone-12',
      'mobile-safari-iphone-se',
      'mobile-chrome-pixel-7',
      'mobile-chrome-pixel-5',
      'mobile-chrome-galaxy-s9',
      'mobile-chrome-galaxy-s8',
    ],
    description: 'Comprehensive mobile device and browser testing',
    tags: ['mobile', 'responsive', 'touch'],
    timeout: 45000,
    parallelWorkers: 3,
  },

  // Tablet testing
  tablet: {
    name: 'Tablet Device Testing',
    projects: [
      'tablet-safari-ipad-pro',
      'tablet-safari-ipad-air',
      'tablet-safari-ipad-mini',
      'tablet-chrome-android',
    ],
    description: 'Tablet-specific interface and interaction testing',
    tags: ['tablet', 'medium-screen', 'touch'],
    timeout: 40000,
    parallelWorkers: 2,
  },

  // Performance testing across devices
  performance: {
    name: 'Cross-Device Performance Testing',
    projects: [
      'chromium-desktop-hd',
      'low-end-mobile-chrome',
      'slow-3g-mobile',
      'fast-3g-mobile',
      'slow-wifi-desktop',
    ],
    description:
      'Performance testing across different device capabilities and network conditions',
    tags: ['performance', 'network', 'low-end'],
    timeout: 90000,
    parallelWorkers: 2,
  },

  // Accessibility testing
  accessibility: {
    name: 'Accessibility Testing',
    projects: ['chromium-accessibility', 'rtl-language-test'],
    description: 'Accessibility and internationalization testing',
    tags: ['accessibility', 'a11y', 'i18n'],
    timeout: 60000,
    parallelWorkers: 2,
  },

  // Ultra-wide and special display testing
  displays: {
    name: 'Special Display Testing',
    projects: ['ultrawide-chrome', 'chromium-desktop-4k'],
    description: 'Testing on ultra-wide and high-DPI displays',
    tags: ['displays', 'ultrawide', '4k', 'high-dpi'],
    timeout: 45000,
    parallelWorkers: 2,
  },
};

// Network Conditions
export const NETWORK_CONDITIONS: Record<string, NetworkCondition> = {
  'slow-3g': {
    name: 'Slow 3G',
    downloadThroughput: (500 * 1024) / 8, // 500 Kbps
    uploadThroughput: (500 * 1024) / 8, // 500 Kbps
    latency: 2000, // 2000ms
  },
  'fast-3g': {
    name: 'Fast 3G',
    downloadThroughput: (1.5 * 1024 * 1024) / 8, // 1.5 Mbps
    uploadThroughput: (750 * 1024) / 8, // 750 Kbps
    latency: 562.5, // 562.5ms
  },
  '4g': {
    name: '4G',
    downloadThroughput: (4 * 1024 * 1024) / 8, // 4 Mbps
    uploadThroughput: (3 * 1024 * 1024) / 8, // 3 Mbps
    latency: 20, // 20ms
  },
  wifi: {
    name: 'WiFi',
    downloadThroughput: (30 * 1024 * 1024) / 8, // 30 Mbps
    uploadThroughput: (15 * 1024 * 1024) / 8, // 15 Mbps
    latency: 2, // 2ms
  },
  'slow-wifi': {
    name: 'Slow WiFi',
    downloadThroughput: (1.5 * 1024 * 1024) / 8, // 1.5 Mbps
    uploadThroughput: (0.75 * 1024 * 1024) / 8, // 750 Kbps
    latency: 40, // 40ms
  },
};

// Device Capabilities
export const DEVICE_CAPABILITIES: Record<string, DeviceCapability> = {
  'desktop-hd': {
    viewport: { width: 1920, height: 1080 },
    userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
    deviceScaleFactor: 1,
    isMobile: false,
    hasTouch: false,
    defaultBrowserType: 'chromium',
  },
  'desktop-4k': {
    viewport: { width: 2560, height: 1440 },
    userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
    deviceScaleFactor: 2,
    isMobile: false,
    hasTouch: false,
    defaultBrowserType: 'chromium',
  },
  ultrawide: {
    viewport: { width: 3440, height: 1440 },
    userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
    deviceScaleFactor: 1,
    isMobile: false,
    hasTouch: false,
    defaultBrowserType: 'chromium',
  },
  'iphone-14': {
    viewport: { width: 390, height: 844 },
    userAgent:
      'Mozilla/5.0 (iPhone; CPU iPhone OS 16_0 like Mac OS X) AppleWebKit/605.1.15',
    deviceScaleFactor: 3,
    isMobile: true,
    hasTouch: true,
    defaultBrowserType: 'webkit',
  },
  'pixel-7': {
    viewport: { width: 412, height: 915 },
    userAgent: 'Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36',
    deviceScaleFactor: 2.625,
    isMobile: true,
    hasTouch: true,
    defaultBrowserType: 'chromium',
  },
  'ipad-pro': {
    viewport: { width: 1024, height: 1366 },
    userAgent:
      'Mozilla/5.0 (iPad; CPU OS 16_0 like Mac OS X) AppleWebKit/605.1.15',
    deviceScaleFactor: 2,
    isMobile: false,
    hasTouch: true,
    defaultBrowserType: 'webkit',
  },
};

// Browser-Specific Workarounds and Configurations
export const BROWSER_WORKAROUNDS = {
  webkit: {
    // Safari-specific issues
    forceSlowMo: 100, // Slow down actions for Safari stability
    ignoreHTTPSErrors: true, // Safari certificate issues in testing
    extraWaitTime: 1000, // Additional wait time for Safari rendering
  },
  firefox: {
    // Firefox-specific issues
    disableBackgroundTimerThrottling: true,
    extraLaunchArgs: ['--disable-background-timer-throttling'],
  },
  chromium: {
    // Chrome-specific optimizations
    enableNetworkServiceLogging: true,
    extraLaunchArgs: ['--disable-dev-shm-usage', '--no-sandbox'],
  },
};

// Performance Benchmarks by Device Type
export const PERFORMANCE_BENCHMARKS = {
  'desktop-high-end': {
    pageLoadTime: 1500, // ms
    firstContentfulPaint: 800, // ms
    largestContentfulPaint: 2500, // ms
    timeToInteractive: 3000, // ms
    cumulativeLayoutShift: 0.1,
  },
  'desktop-standard': {
    pageLoadTime: 2500,
    firstContentfulPaint: 1200,
    largestContentfulPaint: 3500,
    timeToInteractive: 4000,
    cumulativeLayoutShift: 0.1,
  },
  'mobile-high-end': {
    pageLoadTime: 3000,
    firstContentfulPaint: 1500,
    largestContentfulPaint: 4000,
    timeToInteractive: 5000,
    cumulativeLayoutShift: 0.1,
  },
  'mobile-low-end': {
    pageLoadTime: 5000,
    firstContentfulPaint: 2500,
    largestContentfulPaint: 6000,
    timeToInteractive: 8000,
    cumulativeLayoutShift: 0.15,
  },
  tablet: {
    pageLoadTime: 2000,
    firstContentfulPaint: 1000,
    largestContentfulPaint: 3000,
    timeToInteractive: 4000,
    cumulativeLayoutShift: 0.1,
  },
};

// Test Suite Recommendations by Platform
export interface TestSuiteRecommendation {
  platform: string;
  recommendedSuites: string[];
  criticalTests: string[];
  skipTests: string[];
  specialConsiderations: string[];
}

export const TEST_SUITE_RECOMMENDATIONS: TestSuiteRecommendation[] = [
  {
    platform: 'iOS Safari',
    recommendedSuites: ['smoke', 'mobile', 'performance'],
    criticalTests: [
      'wallet-connection',
      'touch-interactions',
      'responsive-layout',
    ],
    skipTests: ['file-upload-drag-drop'],
    specialConsiderations: [
      'Test wallet integration with iOS Safari restrictions',
      'Verify touch gesture handling',
      'Check viewport meta tag behavior',
      'Test iOS-specific date/time pickers',
    ],
  },
  {
    platform: 'Android Chrome',
    recommendedSuites: ['smoke', 'mobile', 'performance'],
    criticalTests: ['pwa-installation', 'notifications', 'background-sync'],
    skipTests: [],
    specialConsiderations: [
      'Test PWA installation flow',
      'Verify notification permissions and handling',
      'Test background synchronization',
      'Check Android-specific UI elements',
    ],
  },
  {
    platform: 'Desktop Chrome',
    recommendedSuites: ['smoke', 'full', 'performance', 'accessibility'],
    criticalTests: [
      'advanced-defi-features',
      'file-upload',
      'keyboard-navigation',
    ],
    skipTests: [],
    specialConsiderations: [
      'Test all advanced DeFi features',
      'Verify complex form interactions',
      'Test drag-and-drop functionality',
      'Check Chrome DevTools integration',
    ],
  },
  {
    platform: 'Desktop Firefox',
    recommendedSuites: ['smoke', 'full'],
    criticalTests: ['wallet-integration', 'cookie-handling', 'local-storage'],
    skipTests: ['chrome-specific-apis'],
    specialConsiderations: [
      'Test Firefox-specific privacy features',
      'Verify Enhanced Tracking Protection compatibility',
      'Test add-on interactions',
      'Check Firefox-specific CSS features',
    ],
  },
  {
    platform: 'Desktop Safari',
    recommendedSuites: ['smoke', 'full'],
    criticalTests: ['webkit-specific-features', 'date-inputs', 'audio-video'],
    skipTests: ['chrome-extensions'],
    specialConsiderations: [
      'Test WebKit-specific rendering differences',
      'Verify Safari privacy features',
      'Test iCloud Keychain integration',
      'Check Safari-specific input handling',
    ],
  },
];

export default {
  BROWSER_CONFIGS,
  NETWORK_CONDITIONS,
  DEVICE_CAPABILITIES,
  BROWSER_WORKAROUNDS,
  PERFORMANCE_BENCHMARKS,
  TEST_SUITE_RECOMMENDATIONS,
};
