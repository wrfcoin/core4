/**
 * Quick IoT/Weather Performance Test
 *
 * A shortened version of the performance test for rapid results
 */

const {
  IoTWeatherPerformanceSimulator,
} = require('./simulation-iot-performance-test');

class QuickIoTPerformanceTest extends IoTWeatherPerformanceSimulator {
  constructor() {
    super();

    // Override with shorter test scenarios
    this.config.testScenarios = [
      { name: 'Quick Baseline', devices: 1, duration: 10, interval: 2 },
      { name: 'Quick Multi-Device', devices: 5, duration: 15, interval: 2 },
      { name: 'Quick Load Test', devices: 10, duration: 20, interval: 1 },
      { name: 'Quick Stress Test', devices: 25, duration: 20, interval: 1 },
    ];
  }
}

// Run quick test
if (require.main === module) {
  const tester = new QuickIoTPerformanceTest();

  tester
    .startPerformanceTest()
    .then(() => {
      console.log('🎉 Quick IoT Performance Test completed!');
      process.exit(0);
    })
    .catch(error => {
      console.error('❌ Quick performance test failed:', error);
      process.exit(1);
    });
}

module.exports = { QuickIoTPerformanceTest };
