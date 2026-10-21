/**
 * Lighthouse CI Configuration for WRFCoin Platform
 * Performance regression detection and monitoring
 */

module.exports = {
  ci: {
    // Collect configuration
    collect: {
      // URLs to test
      url: [
        'http://localhost:3000', // Main dashboard
        'http://localhost:3000/wallet', // Wallet page
        'http://localhost:3000/nft', // NFT marketplace
        'http://localhost:3000/governance', // Governance dashboard
        'http://localhost:3000/defi', // DeFi interface
        'http://localhost:3000/carbon', // Carbon credits
        'http://localhost:3000/weather', // Weather data
      ],

      // Collection settings
      numberOfRuns: 3, // Run 3 times for consistency
      puppeteerScript: './tests/utils/lighthouse-puppeteer-script.js',
      puppeteerLaunchOptions: {
        headless: true,
        args: [
          '--no-sandbox',
          '--disable-dev-shm-usage',
          '--disable-gpu',
          '--disable-background-timer-throttling',
          '--disable-backgrounding-occluded-windows',
          '--disable-renderer-backgrounding',
        ],
      },

      // Chrome flags for consistent testing
      chromeFlags: [
        '--headless',
        '--no-sandbox',
        '--disable-gpu',
        '--disable-dev-shm-usage',
        '--memory-pressure-off',
        '--max_old_space_size=4096',
      ],
    },

    // Upload configuration (for CI/CD artifact storage)
    upload: {
      target: 'filesystem',
      outputDir: './tests/results/lighthouse',
      reportFilenamePattern: '%%PATHNAME%%-%%DATETIME%%-report.%%EXTENSION%%',
    },

    // Server configuration for local testing
    server: {
      command: 'npm run start:frontend',
      port: 3000,
      // Wait for server to be ready
      waitForPort: {
        timeout: 60000,
        checkInterval: 1000,
      },
    },

    // Performance budgets and assertions
    assert: {
      assertions: {
        // Performance metrics
        'categories:performance': ['error', { minScore: 0.8 }],
        'categories:accessibility': ['error', { minScore: 0.9 }],
        'categories:best-practices': ['error', { minScore: 0.85 }],
        'categories:seo': ['warn', { minScore: 0.8 }],
        'categories:pwa': ['warn', { minScore: 0.7 }],

        // Core Web Vitals
        'first-contentful-paint': ['error', { maxNumericValue: 2000 }],
        'largest-contentful-paint': ['error', { maxNumericValue: 2500 }],
        'first-meaningful-paint': ['error', { maxNumericValue: 2000 }],
        'speed-index': ['error', { maxNumericValue: 3000 }],
        interactive: ['error', { maxNumericValue: 4000 }],
        'cumulative-layout-shift': ['error', { maxNumericValue: 0.1 }],

        // Resource optimization
        'total-byte-weight': ['warn', { maxNumericValue: 2000000 }], // 2MB
        'unused-css-rules': ['warn', { maxNumericValue: 50000 }],
        'unused-javascript': ['warn', { maxNumericValue: 100000 }],
        'render-blocking-resources': ['error', { maxNumericValue: 100 }],

        // Image optimization
        'modern-image-formats': ['warn', { minScore: 0.8 }],
        'uses-optimized-images': ['warn', { minScore: 0.8 }],
        'uses-webp-images': ['warn', { minScore: 0.8 }],
        'efficiently-encode-images': ['warn', { minScore: 0.8 }],

        // JavaScript optimization
        'uses-text-compression': ['error', { minScore: 0.8 }],
        'unminified-css': ['error', { minScore: 0.8 }],
        'unminified-javascript': ['error', { minScore: 0.8 }],

        // Network optimization
        'uses-http2': ['warn', { minScore: 0.8 }],
        'uses-rel-preconnect': ['warn', { minScore: 0.8 }],
        'uses-rel-preload': ['warn', { minScore: 0.8 }],

        // Security
        'is-on-https': ['error', { minScore: 1 }],
        'redirects-http': ['error', { minScore: 1 }],

        // Accessibility
        'color-contrast': ['error', { minScore: 1 }],
        'image-alt': ['error', { minScore: 1 }],
        label: ['error', { minScore: 1 }],
        'link-name': ['error', { minScore: 1 }],
      },

      // Assertion matrix for different page types
      matrix: [
        {
          matchingUrlPattern: '.*wallet.*',
          assertions: {
            // Wallet-specific performance requirements
            interactive: ['error', { maxNumericValue: 3000 }],
            'first-contentful-paint': ['error', { maxNumericValue: 1500 }],
            'categories:performance': ['error', { minScore: 0.85 }],
          },
        },
        {
          matchingUrlPattern: '.*nft.*',
          assertions: {
            // NFT marketplace specific requirements
            'largest-contentful-paint': ['error', { maxNumericValue: 2000 }],
            'modern-image-formats': ['error', { minScore: 0.9 }],
            'total-byte-weight': ['warn', { maxNumericValue: 3000000 }], // 3MB for images
          },
        },
        {
          matchingUrlPattern: '.*governance.*',
          assertions: {
            // Governance dashboard requirements
            'categories:accessibility': ['error', { minScore: 0.95 }],
            interactive: ['error', { maxNumericValue: 2500 }],
          },
        },
        {
          matchingUrlPattern: '.*defi.*',
          assertions: {
            // DeFi interface critical performance
            interactive: ['error', { maxNumericValue: 2000 }],
            'first-contentful-paint': ['error', { maxNumericValue: 1200 }],
            'categories:performance': ['error', { minScore: 0.9 }],
          },
        },
      ],
    },
  },
};
