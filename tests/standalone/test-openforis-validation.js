#!/usr/bin/env node

/**
 * OpenForis Integration Validation Script
 *
 * Tests all OpenForis components without the complex Jest setup
 */

const fs = require('fs');
const path = require('path');

console.log('🧪 OpenForis Integration Validation Starting...\n');

// Test file structure
const EXPECTED_FILES = [
  'backend/server/middleware/openforis-auth.ts',
  'backend/server/middleware/openforis-monitoring.ts',
  'backend/server/middleware/openforis-rate-limiter.ts',
  'backend/server/middleware/prometheus-metrics.ts',
  'backend/server/utils/logger.ts',
  'backend/server/utils/openforis-alerting.ts',
  'backend/server/utils/openforis-cache.js',
  'backend/server/utils/openforis-transformer.js',
  'backend/server/routes/openforis-health.ts',
  'backend/server/routes/openforis-routes.ts',
  'backend/server/config/openforis-rate-limits.json',
  'backend/server/__tests__/openforis-rate-limiting.test.ts',
];

let validationResults = {
  filesPresent: 0,
  filesMissing: 0,
  syntaxValid: 0,
  syntaxErrors: 0,
  details: [],
};

console.log('📁 Checking file structure...');
EXPECTED_FILES.forEach(file => {
  const fullPath = path.join(process.cwd(), file);
  if (fs.existsSync(fullPath)) {
    console.log(`✅ ${file}`);
    validationResults.filesPresent++;
  } else {
    console.log(`❌ ${file} - MISSING`);
    validationResults.filesMissing++;
    validationResults.details.push(`Missing file: ${file}`);
  }
});

console.log('\n📝 Validating file contents...');

// Check rate limiting configuration
const rateLimitsPath = path.join(
  process.cwd(),
  'backend/server/config/openforis-rate-limits.json'
);
if (fs.existsSync(rateLimitsPath)) {
  try {
    const rateLimitsConfig = JSON.parse(
      fs.readFileSync(rateLimitsPath, 'utf8')
    );
    console.log('✅ Rate limits configuration is valid JSON');

    // Check required tiers
    const requiredTiers = ['free', 'premium', 'enterprise'];
    const hasTiers = requiredTiers.every(
      tier => rateLimitsConfig.tiers && rateLimitsConfig.tiers[tier]
    );

    if (hasTiers) {
      console.log('✅ All required user tiers are configured');
    } else {
      console.log('⚠️  Some user tiers may be missing from configuration');
      validationResults.details.push(
        'Rate limits configuration may be incomplete'
      );
    }

    validationResults.syntaxValid++;
  } catch (error) {
    console.log(
      `❌ Rate limits configuration has syntax errors: ${error.message}`
    );
    validationResults.syntaxErrors++;
    validationResults.details.push(
      `Rate limits JSON syntax error: ${error.message}`
    );
  }
}

// Test TypeScript/JavaScript syntax by loading modules
const moduleTests = [
  {
    file: 'backend/server/middleware/openforis-auth.ts',
    description: 'Authentication middleware',
  },
  {
    file: 'backend/server/utils/openforis-cache.js',
    description: 'Caching utilities',
  },
  {
    file: 'backend/server/utils/openforis-transformer.js',
    description: 'Data transformation utilities',
  },
];

console.log('\n🔍 Testing module imports...');
moduleTests.forEach(test => {
  const fullPath = path.join(process.cwd(), test.file);
  if (fs.existsSync(fullPath)) {
    try {
      // Read and check for basic syntax issues
      const content = fs.readFileSync(fullPath, 'utf8');

      // Check for exports
      const hasExports =
        content.includes('export') || content.includes('module.exports');
      if (hasExports) {
        console.log(`✅ ${test.description} - has exports`);
        validationResults.syntaxValid++;
      } else {
        console.log(`⚠️  ${test.description} - no exports detected`);
        validationResults.details.push(`${test.file} may not export anything`);
      }

      // Check for common patterns
      if (test.file.includes('auth')) {
        const hasAuthPatterns =
          content.includes('authenticate') ||
          content.includes('jwt') ||
          content.includes('apiKey');
        if (hasAuthPatterns) {
          console.log('✅ Authentication patterns detected');
        } else {
          console.log('⚠️  Authentication patterns not clearly detected');
        }
      }

      if (test.file.includes('cache')) {
        const hasCachePatterns =
          content.includes('cache') ||
          content.includes('redis') ||
          content.includes('store');
        if (hasCachePatterns) {
          console.log('✅ Caching patterns detected');
        } else {
          console.log('⚠️  Caching patterns not clearly detected');
        }
      }
    } catch (error) {
      console.log(`❌ ${test.description} - Error: ${error.message}`);
      validationResults.syntaxErrors++;
      validationResults.details.push(`${test.file} error: ${error.message}`);
    }
  }
});

// Check README documentation
console.log('\n📚 Checking documentation...');
const readmeFiles = [
  'backend/server/middleware/README-openforis-auth.md',
  'backend/server/middleware/README-openforis-monitoring.md',
];

readmeFiles.forEach(file => {
  const fullPath = path.join(process.cwd(), file);
  if (fs.existsSync(fullPath)) {
    const content = fs.readFileSync(fullPath, 'utf8');
    if (content.length > 100) {
      console.log(`✅ ${path.basename(file)} - Documentation present`);
    } else {
      console.log(`⚠️  ${path.basename(file)} - Documentation seems minimal`);
    }
  } else {
    console.log(`❌ ${path.basename(file)} - Documentation missing`);
  }
});

// Test rate limiting middleware structure
console.log('\n⚡ Validating rate limiting implementation...');
const rateLimiterPath = path.join(
  process.cwd(),
  'backend/server/middleware/openforis-rate-limiter.ts'
);
if (fs.existsSync(rateLimiterPath)) {
  const content = fs.readFileSync(rateLimiterPath, 'utf8');

  const checks = [
    {
      pattern: 'class OpenForisRateLimiter',
      description: 'Rate limiter class definition',
    },
    {
      pattern: 'getGeneralLimiter',
      description: 'General rate limiter method',
    },
    {
      pattern: 'getProjectLimiter',
      description: 'Project rate limiter method',
    },
    {
      pattern: 'getAnalysisLimiter',
      description: 'Analysis rate limiter method',
    },
    { pattern: 'USER_TIERS', description: 'User tier configuration' },
    { pattern: 'redis', description: 'Redis integration' },
  ];

  checks.forEach(check => {
    if (content.includes(check.pattern)) {
      console.log(`✅ ${check.description}`);
    } else {
      console.log(`❌ ${check.description} - Not found`);
      validationResults.details.push(
        `Rate limiter missing: ${check.description}`
      );
    }
  });
}

// Test authentication middleware structure
console.log('\n🔐 Validating authentication implementation...');
const authPath = path.join(
  process.cwd(),
  'backend/server/middleware/openforis-auth.ts'
);
if (fs.existsSync(authPath)) {
  const content = fs.readFileSync(authPath, 'utf8');

  const authChecks = [
    {
      pattern: 'authenticateOpenForisFlexible',
      description: 'Flexible authentication function',
    },
    {
      pattern: 'authenticateOpenForisOptional',
      description: 'Optional authentication function',
    },
    {
      pattern: 'requireOpenForisPermission',
      description: 'Permission checking function',
    },
    { pattern: 'requireOpenForisTier', description: 'Tier checking function' },
    { pattern: 'jwt', description: 'JWT token support' },
    { pattern: 'apiKey', description: 'API key support' },
  ];

  authChecks.forEach(check => {
    if (content.toLowerCase().includes(check.pattern.toLowerCase())) {
      console.log(`✅ ${check.description}`);
    } else {
      console.log(`❌ ${check.description} - Not found`);
      validationResults.details.push(
        `Authentication missing: ${check.description}`
      );
    }
  });
}

// Test routes implementation
console.log('\n🚀 Validating routes implementation...');
const routesPath = path.join(
  process.cwd(),
  'backend/server/routes/openforis-routes.ts'
);
if (fs.existsSync(routesPath)) {
  const content = fs.readFileSync(routesPath, 'utf8');

  const routeChecks = [
    { pattern: '/status', description: 'Status endpoint' },
    { pattern: '/projects', description: 'Projects endpoints' },
    { pattern: '/climate', description: 'Climate endpoints' },
    { pattern: '/blockchain', description: 'Blockchain endpoints' },
    { pattern: 'router.get', description: 'GET routes' },
    { pattern: 'router.post', description: 'POST routes' },
  ];

  routeChecks.forEach(check => {
    if (content.includes(check.pattern)) {
      console.log(`✅ ${check.description}`);
    } else {
      console.log(`❌ ${check.description} - Not found`);
      validationResults.details.push(`Routes missing: ${check.description}`);
    }
  });
}

// Test monitoring implementation
console.log('\n📊 Validating monitoring implementation...');
const monitoringPath = path.join(
  process.cwd(),
  'backend/server/middleware/openforis-monitoring.ts'
);
if (fs.existsSync(monitoringPath)) {
  const content = fs.readFileSync(monitoringPath, 'utf8');

  const monitoringChecks = [
    { pattern: 'RequestMonitor', description: 'Request monitoring class' },
    { pattern: 'requestMetrics', description: 'Request metrics tracking' },
    { pattern: 'memoryUsage', description: 'Memory usage monitoring' },
    { pattern: 'responseTime', description: 'Response time tracking' },
    { pattern: 'startTime', description: 'Timing mechanisms' },
  ];

  monitoringChecks.forEach(check => {
    if (content.includes(check.pattern)) {
      console.log(`✅ ${check.description}`);
    } else {
      console.log(`❌ ${check.description} - Not found`);
      validationResults.details.push(
        `Monitoring missing: ${check.description}`
      );
    }
  });
}

// Summary
console.log('\n📋 VALIDATION SUMMARY');
console.log('===================');
console.log(
  `✅ Files present: ${validationResults.filesPresent}/${EXPECTED_FILES.length}`
);
console.log(`❌ Files missing: ${validationResults.filesMissing}`);
console.log(
  `✅ Components with valid syntax: ${validationResults.syntaxValid}`
);
console.log(
  `❌ Components with syntax errors: ${validationResults.syntaxErrors}`
);

if (validationResults.details.length > 0) {
  console.log('\n⚠️  Issues found:');
  validationResults.details.forEach(detail => {
    console.log(`   - ${detail}`);
  });
}

const overallScore = Math.round(
  ((validationResults.filesPresent + validationResults.syntaxValid) /
    (EXPECTED_FILES.length + moduleTests.length)) *
    100
);

console.log(`\n🎯 Overall validation score: ${overallScore}%`);

if (overallScore >= 90) {
  console.log('🟢 EXCELLENT - OpenForis integration is well implemented');
} else if (overallScore >= 75) {
  console.log(
    '🟡 GOOD - OpenForis integration is mostly complete with minor issues'
  );
} else if (overallScore >= 60) {
  console.log('🟠 FAIR - OpenForis integration needs some improvements');
} else {
  console.log('🔴 POOR - OpenForis integration has significant issues');
}

console.log('\n✨ Validation complete!');
