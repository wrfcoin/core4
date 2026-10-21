const config = require('../config/test-config');

async function globalSetup() {
  console.log('🚀 Starting simple E2E test environment setup...');

  // Simple setup that just validates services are available
  await warmUpServices();

  console.log('✅ Simple E2E test environment setup complete');
}

async function warmUpServices() {
  console.log('🔥 Warming up services...');

  const services = [
    { name: 'Backend API', url: `${config.baseUrls.backend}/health` },
  ];

  for (const service of services) {
    try {
      await waitForService(service.url, 10000);
      console.log(`✅ ${service.name} is ready`);
    } catch (error) {
      console.warn(`⚠️ ${service.name} not ready:`, error.message);
    }
  }
}

async function waitForService(url, timeout = 10000) {
  const startTime = Date.now();

  while (Date.now() - startTime < timeout) {
    try {
      const response = await fetch(url);
      if (response.ok) {
        return;
      }
    } catch (error) {
      // Service not ready yet, continue waiting
    }

    await new Promise(resolve => setTimeout(resolve, 1000));
  }

  throw new Error(`Service at ${url} not ready within ${timeout}ms`);
}

module.exports = globalSetup;
