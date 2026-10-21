/**
 * WebPageTest Configuration for WRFCoin Platform
 * Comprehensive performance monitoring and regression detection
 */

const WebPageTest = require('webpagetest');

class WRFWebPageTestRunner {
  constructor(options = {}) {
    this.apiKey = options.apiKey || process.env.WEBPAGETEST_API_KEY;
    this.baseUrl = options.baseUrl || 'http://localhost:3000';
    this.wpt = new WebPageTest('www.webpagetest.org', this.apiKey);

    // Performance budgets
    this.budgets = {
      firstView: {
        loadTime: 3000, // 3 seconds
        firstByte: 800, // 800ms TTFB
        startRender: 1500, // 1.5s start render
        speedIndex: 2500, // 2.5s speed index
        visualComplete: 4000, // 4s visual complete
        fullyLoaded: 5000, // 5s fully loaded
      },
      repeatView: {
        loadTime: 2000, // 2 seconds (cached)
        firstByte: 500, // 500ms TTFB
        startRender: 800, // 800ms start render
        speedIndex: 1500, // 1.5s speed index
        visualComplete: 2500, // 2.5s visual complete
        fullyLoaded: 3000, // 3s fully loaded
      },
      bytesIn: {
        total: 2000000, // 2MB total
        html: 50000, // 50KB HTML
        css: 200000, // 200KB CSS
        js: 1000000, // 1MB JavaScript
        images: 500000, // 500KB images
      },
    };

    // Test configurations for different pages
    this.testConfigs = {
      dashboard: {
        url: `${this.baseUrl}/`,
        label: 'Dashboard',
        location: 'Dulles:Chrome',
        runs: 3,
        firstViewOnly: false,
        pollResults: 5,
        video: true,
        breakdown: true,
        domains: true,
        pageSpeed: true,
        requests: true,
        timeline: true,
        netLog: true,
        chromeTrace: true,
        mobile: false,
        script: this.getAuthScript(),
      },

      wallet: {
        url: `${this.baseUrl}/wallet`,
        label: 'Wallet Interface',
        location: 'Dulles:Chrome',
        runs: 3,
        firstViewOnly: false,
        script: this.getWalletScript(),
        mobile: false,
      },

      nft: {
        url: `${this.baseUrl}/nft`,
        label: 'NFT Marketplace',
        location: 'Dulles:Chrome',
        runs: 3,
        firstViewOnly: false,
        script: this.getNFTScript(),
        mobile: false,
      },

      defi: {
        url: `${this.baseUrl}/defi`,
        label: 'DeFi Interface',
        location: 'Dulles:Chrome',
        runs: 3,
        firstViewOnly: false,
        script: this.getDeFiScript(),
        mobile: false,
      },

      governance: {
        url: `${this.baseUrl}/governance`,
        label: 'Governance Dashboard',
        location: 'Dulles:Chrome',
        runs: 3,
        firstViewOnly: false,
        script: this.getGovernanceScript(),
        mobile: false,
      },

      // Mobile tests
      dashboardMobile: {
        url: `${this.baseUrl}/`,
        label: 'Dashboard Mobile',
        location: 'Dulles:Chrome.4G',
        runs: 3,
        firstViewOnly: false,
        mobile: true,
        mobileDevice: 'Pixel2',
        script: this.getAuthScript(),
      },
    };
  }

  /**
   * Generate authentication script for protected pages
   */
  getAuthScript() {
    return `
      // Set authentication tokens
      localStorage.setItem('auth_token', 'test_token');
      localStorage.setItem('user_id', 'test_user');
      localStorage.setItem('wallet_connected', 'true');
      
      // Mock wallet connection
      window.ethereum = {
        isMetaMask: true,
        selectedAddress: '0x742d35Cc6634C0532925a3b8D4B9A4fc8E5d0D8a',
        request: async () => ['0x742d35Cc6634C0532925a3b8D4B9A4fc8E5d0D8a']
      };
    `;
  }

  /**
   * Generate wallet-specific test script
   */
  getWalletScript() {
    return `
      ${this.getAuthScript()}
      
      // Wait for wallet to load
      setTimeout(() => {
        // Click on balance refresh if available
        const refreshButton = document.querySelector('[data-testid="refresh-balance"]');
        if (refreshButton) refreshButton.click();
      }, 2000);
    `;
  }

  /**
   * Generate NFT marketplace test script
   */
  getNFTScript() {
    return `
      ${this.getAuthScript()}
      
      // Wait for NFT grid to load
      setTimeout(() => {
        // Simulate scrolling to load more NFTs
        window.scrollTo(0, 500);
      }, 2000);
      
      setTimeout(() => {
        // Click on first NFT if available
        const firstNFT = document.querySelector('[data-testid="nft-item"]:first-child');
        if (firstNFT) firstNFT.click();
      }, 3000);
    `;
  }

  /**
   * Generate DeFi interface test script
   */
  getDeFiScript() {
    return `
      ${this.getAuthScript()}
      
      // Wait for DeFi pools to load
      setTimeout(() => {
        // Click on first pool if available
        const firstPool = document.querySelector('[data-testid="liquidity-pool"]:first-child');
        if (firstPool) firstPool.click();
      }, 2000);
    `;
  }

  /**
   * Generate governance test script
   */
  getGovernanceScript() {
    return `
      ${this.getAuthScript()}
      
      // Wait for proposals to load
      setTimeout(() => {
        // Click on first proposal if available
        const firstProposal = document.querySelector('[data-testid="proposal-item"]:first-child');
        if (firstProposal) firstProposal.click();
      }, 2000);
    `;
  }

  /**
   * Run performance test for a specific page
   */
  async runTest(pageName) {
    const config = this.testConfigs[pageName];
    if (!config) {
      throw new Error(`Unknown page configuration: ${pageName}`);
    }

    console.log(`🚀 Starting WebPageTest for ${config.label}...`);

    return new Promise((resolve, reject) => {
      this.wpt.runTest(config.url, config, (err, data) => {
        if (err) {
          console.error(`❌ WebPageTest failed for ${config.label}:`, err);
          reject(err);
          return;
        }

        console.log(`✅ WebPageTest completed for ${config.label}`);
        console.log(`📊 Test ID: ${data.data.testId}`);
        console.log(`🔗 Results: ${data.data.summary}`);

        resolve(data);
      });
    });
  }

  /**
   * Analyze test results against budgets
   */
  analyzeResults(results, pageName) {
    const analysis = {
      testId: results.data.testId,
      page: pageName,
      url: results.data.url,
      timestamp: new Date().toISOString(),
      passed: true,
      violations: [],
      summary: {},
      recommendations: [],
    };

    if (!results.data.runs || !results.data.runs['1']) {
      analysis.passed = false;
      analysis.violations.push('No test data available');
      return analysis;
    }

    const run = results.data.runs['1'];
    const firstView = run.firstView;
    const repeatView = run.repeatView;

    // Analyze first view performance
    analysis.summary.firstView = this.analyzeView(
      firstView,
      this.budgets.firstView,
      'First View'
    );

    // Analyze repeat view performance if available
    if (repeatView) {
      analysis.summary.repeatView = this.analyzeView(
        repeatView,
        this.budgets.repeatView,
        'Repeat View'
      );
    }

    // Analyze byte weights
    analysis.summary.bytes = this.analyzeBytes(firstView, this.budgets.bytesIn);

    // Collect all violations
    const allViolations = [
      ...analysis.summary.firstView.violations,
      ...(analysis.summary.repeatView?.violations || []),
      ...analysis.summary.bytes.violations,
    ];

    analysis.violations = allViolations;
    analysis.passed = allViolations.length === 0;

    // Generate recommendations
    analysis.recommendations = this.generateRecommendations(
      firstView,
      repeatView
    );

    return analysis;
  }

  /**
   * Analyze individual view (first/repeat) against budgets
   */
  analyzeView(view, budgets, viewType) {
    const violations = [];
    const metrics = {};

    // Check each metric against budget
    Object.entries(budgets).forEach(([metric, budget]) => {
      const actual = view[metric];
      metrics[metric] = { actual, budget, passed: actual <= budget };

      if (actual > budget) {
        violations.push({
          metric,
          viewType,
          actual,
          budget,
          overrun: actual - budget,
          overrunPercent: Math.round(((actual - budget) / budget) * 100),
        });
      }
    });

    return { violations, metrics };
  }

  /**
   * Analyze byte weights against budgets
   */
  analyzeBytes(view, budgets) {
    const violations = [];
    const metrics = {};

    // Total bytes
    const totalBytes = view.bytesIn;
    metrics.total = {
      actual: totalBytes,
      budget: budgets.total,
      passed: totalBytes <= budgets.total,
    };

    if (totalBytes > budgets.total) {
      violations.push({
        metric: 'total',
        viewType: 'Bytes',
        actual: totalBytes,
        budget: budgets.total,
        overrun: totalBytes - budgets.total,
        overrunPercent: Math.round(
          ((totalBytes - budgets.total) / budgets.total) * 100
        ),
      });
    }

    // Breakdown by content type if available
    if (view.breakdown) {
      ['html', 'css', 'js', 'image'].forEach(type => {
        const typeData = view.breakdown[type];
        if (typeData && budgets[type]) {
          const actualBytes = typeData.bytes;
          metrics[type] = {
            actual: actualBytes,
            budget: budgets[type],
            passed: actualBytes <= budgets[type],
          };

          if (actualBytes > budgets[type]) {
            violations.push({
              metric: type,
              viewType: 'Bytes',
              actual: actualBytes,
              budget: budgets[type],
              overrun: actualBytes - budgets[type],
              overrunPercent: Math.round(
                ((actualBytes - budgets[type]) / budgets[type]) * 100
              ),
            });
          }
        }
      });
    }

    return { violations, metrics };
  }

  /**
   * Generate performance recommendations
   */
  generateRecommendations(firstView, repeatView) {
    const recommendations = [];

    // High TTFB
    if (firstView.TTFB > 1000) {
      recommendations.push({
        category: 'Server Performance',
        issue: 'High Time To First Byte',
        description: `TTFB is ${firstView.TTFB}ms. Consider server-side optimizations.`,
        priority: 'high',
      });
    }

    // Large JavaScript
    if (firstView.breakdown?.js?.bytes > 800000) {
      recommendations.push({
        category: 'JavaScript',
        issue: 'Large JavaScript Bundle',
        description: `JavaScript is ${Math.round(firstView.breakdown.js.bytes / 1024)}KB. Consider code splitting.`,
        priority: 'medium',
      });
    }

    // Poor caching
    if (repeatView && repeatView.loadTime / firstView.loadTime > 0.8) {
      recommendations.push({
        category: 'Caching',
        issue: 'Poor Caching Performance',
        description:
          'Repeat view is not significantly faster. Check cache headers.',
        priority: 'medium',
      });
    }

    // Large images
    if (firstView.breakdown?.image?.bytes > 1000000) {
      recommendations.push({
        category: 'Images',
        issue: 'Large Image Assets',
        description: `Images are ${Math.round(firstView.breakdown.image.bytes / 1024)}KB. Consider optimization.`,
        priority: 'low',
      });
    }

    return recommendations;
  }

  /**
   * Run comprehensive performance test suite
   */
  async runFullSuite() {
    console.log('🎯 Starting comprehensive WebPageTest suite...');

    const results = {};
    const pageNames = Object.keys(this.testConfigs);

    for (const pageName of pageNames) {
      try {
        const testResult = await this.runTest(pageName);
        const analysis = this.analyzeResults(testResult, pageName);
        results[pageName] = analysis;

        // Log summary
        console.log(`\n📊 ${pageName} Results:`);
        console.log(`  Status: ${analysis.passed ? '✅ PASSED' : '❌ FAILED'}`);
        if (analysis.violations.length > 0) {
          console.log(`  Violations: ${analysis.violations.length}`);
          analysis.violations.forEach(v => {
            console.log(
              `    • ${v.metric}: ${v.actual}ms (budget: ${v.budget}ms, +${v.overrunPercent}%)`
            );
          });
        }
      } catch (error) {
        console.error(`❌ Test failed for ${pageName}:`, error.message);
        results[pageName] = {
          passed: false,
          error: error.message,
          timestamp: new Date().toISOString(),
        };
      }

      // Wait between tests to avoid rate limiting
      await new Promise(resolve => setTimeout(resolve, 10000));
    }

    // Generate summary report
    const summary = this.generateSummaryReport(results);

    // Save results
    const fs = require('fs').promises;
    const resultsPath = './tests/results/webpagetest-results.json';
    await fs.writeFile(
      resultsPath,
      JSON.stringify({ summary, results }, null, 2)
    );

    console.log(`\n📝 Results saved to: ${resultsPath}`);
    console.log(
      `\n🎯 Overall Status: ${summary.passed ? '✅ PASSED' : '❌ FAILED'}`
    );

    return { summary, results };
  }

  /**
   * Generate summary report
   */
  generateSummaryReport(results) {
    const summary = {
      timestamp: new Date().toISOString(),
      totalTests: Object.keys(results).length,
      passed: 0,
      failed: 0,
      violations: [],
      recommendations: [],
    };

    Object.entries(results).forEach(([pageName, result]) => {
      if (result.passed) {
        summary.passed++;
      } else {
        summary.failed++;
        if (result.violations) {
          summary.violations.push(
            ...result.violations.map(v => ({ ...v, page: pageName }))
          );
        }
      }

      if (result.recommendations) {
        summary.recommendations.push(
          ...result.recommendations.map(r => ({ ...r, page: pageName }))
        );
      }
    });

    summary.passed = summary.failed === 0;

    return summary;
  }
}

module.exports = { WRFWebPageTestRunner };

// CLI execution
if (require.main === module) {
  const runner = new WRFWebPageTestRunner();

  runner
    .runFullSuite()
    .then(results => {
      if (!results.summary.passed) {
        console.error('❌ Performance tests failed!');
        process.exit(1);
      } else {
        console.log('✅ All performance tests passed!');
      }
    })
    .catch(error => {
      console.error('💥 Performance test suite failed:', error);
      process.exit(1);
    });
}
