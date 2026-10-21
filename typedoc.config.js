/**
 * TypeDoc Configuration for WRFCoin SDK Documentation
 *
 * Generates comprehensive documentation for:
 * - JavaScript/TypeScript SDK
 * - Type definitions
 * - API interfaces
 * - Usage examples
 */

module.exports = {
  // Entry points
  entryPoints: ['sdk/wrf-sdk-js/src/index.ts'],

  // Output configuration
  out: 'docs/generated/typedoc',

  // Project settings
  name: 'WRFCoin Platform SDK Documentation',

  // Include/exclude patterns
  exclude: [
    '**/*.test.ts',
    '**/*.spec.ts',
    '**/*.test.js',
    '**/*.spec.js',
    '**/node_modules/**',
    '**/dist/**',
    '**/build/**',
  ],

  // Theme and rendering
  theme: 'default',
  plugin: ['typedoc-plugin-markdown'],

  // Documentation options
  readme: 'docs/sdk/README.md',
  includeVersion: true,
  excludePrivate: false,
  excludeProtected: false,
  excludeExternals: true,
  hideGenerator: false,
  disableSources: false,

  // Module organization
  categorizeByGroup: true,
  defaultCategory: 'Other',
  categoryOrder: [
    'Core',
    'Blockchain',
    'Wallet',
    'Transactions',
    'Smart Contracts',
    'Weather Data',
    'NFT',
    'Cross-Chain',
    'IoT',
    'Utilities',
    'Types',
    'Other',
  ],

  // Navigation and search
  navigation: {
    includeCategories: true,
    includeGroups: true,
  },

  // Output formats
  json: 'docs/generated/typedoc.json',

  // Validation
  validation: {
    notExported: false,
    invalidLink: false,
    notDocumented: false,
  },

  // Skip type checking errors
  skipErrorChecking: true,
  compilerOptions: {
    skipLibCheck: true,
    allowJs: true,
    noImplicitAny: false,
  },

  // Custom options
  customCss: 'docs/assets/typedoc-custom.css',

  // TypeScript compilation
  tsconfig: 'sdk/wrf-sdk-js/tsconfig.json',

  // Comment parsing
  commentStyle: 'jsdoc',
  blockTags: [
    '@alpha',
    '@beta',
    '@deprecated',
    '@example',
    '@experimental',
    '@internal',
    '@override',
    '@param',
    '@public',
    '@readonly',
    '@returns',
    '@see',
    '@since',
    '@throws',
    '@version',
  ],

  // External links
  githubPages: false,
  gitRevision: 'main',

  // Advanced options
  preserveWatchOutput: true,
  pretty: true,
  emit: 'both',

  // Custom footer
  customFooterHtml: `
    <div style="text-align: center; margin-top: 30px; padding: 20px; border-top: 1px solid #eee;">
      <p>
        <strong>WRFCoin Platform SDK Documentation</strong><br>
        Generated on ${new Date().toISOString().split('T')[0]} | 
        <a href="https://wrfcoin.com" target="_blank">Official Website</a> | 
        <a href="https://github.com/wrfcoin/core" target="_blank">GitHub Repository</a>
      </p>
    </div>
  `,
};
