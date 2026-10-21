#!/usr/bin/env node

/**
 * WRFCoin Documentation Server
 *
 * Serves automated documentation including:
 * - Swagger UI for REST API documentation
 * - GraphQL Playground for GraphQL API
 * - TypeDoc generated SDK documentation
 * - Smart contract documentation
 */

const express = require('express');
const swaggerUi = require('swagger-ui-express');
const swaggerJsdoc = require('swagger-jsdoc');
const fs = require('fs');
const path = require('path');
const { createHandler } = require('graphql-http/lib/use/express');
const { buildSchema } = require('graphql');
const { corsMiddleware } = require('../backend/shared/middleware/cors-config.cjs');
const { createSecurityHeadersMiddleware } = require('../backend/shared/middleware/security-headers.cjs');

const app = express();
const PORT = process.env.DOCS_PORT || 3003;

// === Swagger/OpenAPI Configuration ===

const swaggerOptions = {
  definition: {
    openapi: '3.0.3',
    info: {
      title: 'WRFCoin Platform API Documentation',
      version: '1.0.0',
      description: `
        Comprehensive API documentation for the WRFCoin climate-focused blockchain platform.
        
        ## Features
        - **Weather Data Collection**: IoT sensor integration and environmental data tracking
        - **Smart Contracts**: Automated contract deployment and execution
        - **Cross-Chain Bridge**: Multi-blockchain asset transfers
        - **Governance**: DAO governance and community voting
        - **NFT Marketplace**: Environmental and computational NFTs
        - **Carbon Credits**: Environmental impact tracking and trading
        
        ## Authentication
        Most endpoints require authentication via API key or Bearer token.
        
        ## Rate Limiting
        API requests are rate-limited to 100 requests per 15-minute window per IP.
      `,
      contact: {
        name: 'WRFCoin API Support',
        email: 'api@wrfcoin.com',
        url: 'https://docs.wrfcoin.com',
      },
      license: {
        name: 'MIT',
        url: 'https://opensource.org/licenses/MIT',
      },
    },
    servers: [
      {
        url: 'https://api.wrfcoin.com/v1',
        description: 'Production server',
      },
      {
        url: 'https://staging-api.wrfcoin.com/v1',
        description: 'Staging server',
      },
      {
        url: 'http://localhost:3001',
        description: 'Local development server',
      },
    ],
    components: {
      securitySchemes: {
        ApiKeyAuth: {
          type: 'apiKey',
          in: 'header',
          name: 'X-API-Key',
        },
        BearerAuth: {
          type: 'http',
          scheme: 'bearer',
          bearerFormat: 'JWT',
        },
      },
    },
    security: [{ ApiKeyAuth: [] }, { BearerAuth: [] }],
  },
  apis: [
    './backend/**/*.js',
    './backend/**/*.ts',
    './backend/api/documentation/openapi.yaml',
  ],
};

// Load existing OpenAPI spec if available
let openApiSpec = null;
const openApiPath = path.join(
  __dirname,
  '../backend/api/documentation/openapi.yaml'
);
if (fs.existsSync(openApiPath)) {
  const yaml = require('js-yaml');
  const openApiContent = fs.readFileSync(openApiPath, 'utf8');
  openApiSpec = yaml.load(openApiContent);
  console.log('✅ Loaded existing OpenAPI specification');
}

// Generate Swagger specification
const specs = openApiSpec || swaggerJsdoc(swaggerOptions);

// === GraphQL Schema Setup ===

let graphqlSchema = null;
const graphqlSchemaPath = path.join(
  __dirname,
  '../backend/graphql/schema.graphql'
);
if (fs.existsSync(graphqlSchemaPath)) {
  const schemaContent = fs.readFileSync(graphqlSchemaPath, 'utf8');
  graphqlSchema = buildSchema(schemaContent);
  console.log('✅ Loaded GraphQL schema');
}

// === Middleware Setup ===

// Enable CORS with proper origin validation
app.use(corsMiddleware);
// Apply security headers (CSP, X-Frame-Options, etc.)
// Note: Using createSecurityHeadersMiddleware for HTML content (docs serve HTML pages)
app.use(createSecurityHeadersMiddleware());

// Serve static documentation files
app.use('/static', express.static(path.join(__dirname)));

// === Documentation Routes ===

// Main documentation homepage
app.get('/', (req, res) => {
  res.send(`
    <!DOCTYPE html>
    <html lang="en">
    <head>
        <meta charset="UTF-8">
        <meta name="viewport" content="width=device-width, initial-scale=1.0">
        <title>WRFCoin Platform Documentation</title>
        <style>
            body {
                font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
                max-width: 1200px;
                margin: 0 auto;
                padding: 40px 20px;
                line-height: 1.6;
                color: #333;
            }
            .header {
                text-align: center;
                margin-bottom: 50px;
                padding: 30px;
                background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
                color: white;
                border-radius: 10px;
            }
            .docs-grid {
                display: grid;
                grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
                gap: 30px;
                margin-bottom: 40px;
            }
            .doc-card {
                border: 1px solid #e1e5e9;
                border-radius: 8px;
                padding: 25px;
                text-decoration: none;
                color: inherit;
                transition: all 0.3s ease;
            }
            .doc-card:hover {
                box-shadow: 0 4px 12px rgba(0,0,0,0.1);
                transform: translateY(-2px);
                text-decoration: none;
                color: inherit;
            }
            .doc-card h3 {
                margin: 0 0 15px 0;
                color: #2c3e50;
            }
            .doc-card p {
                margin: 0;
                color: #7f8c8d;
            }
            .status {
                display: inline-block;
                padding: 4px 8px;
                border-radius: 4px;
                font-size: 12px;
                font-weight: bold;
                margin-left: 10px;
            }
            .status.available { background: #d4edda; color: #155724; }
            .status.unavailable { background: #f8d7da; color: #721c24; }
            .footer {
                text-align: center;
                margin-top: 50px;
                padding-top: 30px;
                border-top: 1px solid #e1e5e9;
                color: #7f8c8d;
            }
        </style>
    </head>
    <body>
        <div class="header">
            <h1>🌍 WRFCoin Platform Documentation</h1>
            <p>Comprehensive developer documentation for the climate-focused blockchain platform</p>
        </div>
        
        <div class="docs-grid">
            <a href="/api-docs" class="doc-card">
                <h3>🔌 REST API Documentation <span class="status ${specs ? 'available' : 'unavailable'}">${specs ? 'Available' : 'Unavailable'}</span></h3>
                <p>Interactive Swagger UI documentation for all REST API endpoints including blockchain operations, weather data, and wallet management.</p>
            </a>
            
            <a href="/graphql" class="doc-card">
                <h3>📊 GraphQL API Documentation <span class="status ${graphqlSchema ? 'available' : 'unavailable'}">${graphqlSchema ? 'Available' : 'Unavailable'}</span></h3>
                <p>GraphQL Playground for exploring the GraphQL API schema, running queries, and testing mutations.</p>
            </a>
            
            <a href="/sdk-docs/js" class="doc-card">
                <h3>📚 JavaScript SDK Documentation <span class="status available">Available</span></h3>
                <p>TypeDoc generated documentation for the JavaScript/TypeScript SDK including all classes, interfaces, and usage examples.</p>
            </a>
            
            <a href="/contract-docs" class="doc-card">
                <h3>📜 Smart Contract Documentation <span class="status available">Available</span></h3>
                <p>Generated documentation for all smart contracts including ABIs, function signatures, and deployment guides.</p>
            </a>
            
            <a href="/guides" class="doc-card">
                <h3>📖 Developer Guides <span class="status available">Available</span></h3>
                <p>Step-by-step guides, tutorials, and examples for integrating with the WRFCoin platform.</p>
            </a>
            
            <a href="/architecture" class="doc-card">
                <h3>🏗️ Architecture Documentation <span class="status available">Available</span></h3>
                <p>System architecture diagrams, component interactions, and technical specifications.</p>
            </a>
        </div>
        
        <div class="footer">
            <p>📧 Questions? Contact us at <strong>api@wrfcoin.com</strong></p>
            <p>🔗 <a href="https://github.com/wrfcoin/core">GitHub Repository</a> | <a href="https://wrfcoin.com">Official Website</a></p>
        </div>
    </body>
    </html>
  `);
});

// Swagger UI for REST API documentation
if (specs) {
  app.use(
    '/api-docs',
    swaggerUi.serve,
    swaggerUi.setup(specs, {
      customSiteTitle: 'WRFCoin API Documentation',
      customCss: `
      .swagger-ui .topbar { display: none; }
      .swagger-ui .info .title { color: #667eea; }
    `,
      customfavIcon: '/static/favicon.ico',
    })
  );
  console.log('✅ Swagger UI configured for REST API documentation');
} else {
  app.get('/api-docs', (req, res) => {
    res
      .status(404)
      .send(
        '<h1>API Documentation Not Available</h1><p>OpenAPI specification not found.</p>'
      );
  });
}

// GraphQL Playground (simplified version)
if (graphqlSchema) {
  app.get('/graphql', (req, res) => {
    res.send(`
      <!DOCTYPE html>
      <html>
      <head>
          <title>GraphQL Playground</title>
          <style>
              body {
                  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
                  margin: 0;
                  padding: 40px;
                  background: #f7f9fc;
              }
              .container {
                  max-width: 1200px;
                  margin: 0 auto;
                  background: white;
                  border-radius: 8px;
                  padding: 30px;
                  box-shadow: 0 2px 10px rgba(0,0,0,0.1);
              }
              .header {
                  text-align: center;
                  margin-bottom: 30px;
              }
              .schema-info {
                  background: #f8f9fa;
                  padding: 20px;
                  border-radius: 6px;
                  margin: 20px 0;
              }
              .endpoint {
                  background: #e3f2fd;
                  padding: 10px 15px;
                  border-radius: 4px;
                  font-family: monospace;
                  margin: 10px 0;
              }
          </style>
      </head>
      <body>
          <div class="container">
              <div class="header">
                  <h1>📊 GraphQL API Documentation</h1>
                  <p>Interactive GraphQL schema exploration and query testing</p>
              </div>
              
              <div class="schema-info">
                  <h3>🔗 GraphQL Endpoint</h3>
                  <div class="endpoint">POST http://localhost:4000/graphql</div>
                  
                  <h3>📋 Available Operations</h3>
                  <p>The GraphQL schema includes operations for:</p>
                  <ul>
                      <li><strong>Weather Data</strong> - Query and submit environmental sensor data</li>
                      <li><strong>Blockchain Operations</strong> - Transaction and block queries</li>
                      <li><strong>Wallet Management</strong> - Balance queries and transaction history</li>
                      <li><strong>NFT Operations</strong> - Mint, transfer, and query NFTs</li>
                      <li><strong>Governance</strong> - DAO proposals and voting</li>
                      <li><strong>Carbon Credits</strong> - Environmental impact tracking</li>
                  </ul>
                  
                  <h3>🛠️ Development Tools</h3>
                  <p>For interactive GraphQL exploration, use:</p>
                  <ul>
                      <li><a href="https://github.com/graphql/graphql-playground" target="_blank">GraphQL Playground Desktop App</a></li>
                      <li><a href="https://studio.apollographql.com/sandbox" target="_blank">Apollo Studio Sandbox</a></li>
                      <li>Browser GraphQL clients like GraphiQL or Altair</li>
                  </ul>
              </div>
              
              <div style="text-align: center; margin-top: 30px;">
                  <a href="/" style="color: #667eea; text-decoration: none;">← Back to Documentation Home</a>
              </div>
          </div>
      </body>
      </html>
    `);
  });
  console.log('✅ GraphQL documentation configured');
} else {
  app.get('/graphql', (req, res) => {
    res
      .status(404)
      .send(
        '<h1>GraphQL Documentation Not Available</h1><p>GraphQL schema not found.</p>'
      );
  });
}

// SDK Documentation routes (will be populated by TypeDoc)
app.get('/sdk-docs/js', (req, res) => {
  const typedocPath = path.join(__dirname, 'generated/typedoc/index.html');
  if (fs.existsSync(typedocPath)) {
    res.sendFile(typedocPath);
  } else {
    res.send(`
      <h1>📚 JavaScript SDK Documentation</h1>
      <p>TypeDoc documentation is being generated. Please run: <code>npm run docs:sdk</code></p>
      <p><a href="/">← Back to Documentation Home</a></p>
    `);
  }
});

// Smart Contract Documentation
app.get('/contract-docs', (req, res) => {
  const contractDocsPath = path.join(
    __dirname,
    'generated/contracts/index.html'
  );
  if (fs.existsSync(contractDocsPath)) {
    res.sendFile(contractDocsPath);
  } else {
    res.send(`
      <h1>📜 Smart Contract Documentation</h1>
      <p>Contract documentation is being generated. Please run: <code>npm run docs:contracts</code></p>
      <p><a href="/">← Back to Documentation Home</a></p>
    `);
  }
});

// Developer Guides
app.get('/guides', (req, res) => {
  res.redirect('/static/guides/index.html');
});

// Architecture Documentation
app.get('/architecture', (req, res) => {
  res.redirect('/static/architecture/index.html');
});

// Health check
app.get('/health', (req, res) => {
  res.json({
    status: 'healthy',
    timestamp: new Date().toISOString(),
    documentation: {
      'REST API': specs ? 'available' : 'unavailable',
      'GraphQL API': graphqlSchema ? 'available' : 'unavailable',
      'SDK Docs': 'pending-generation',
      'Contract Docs': 'pending-generation',
    },
  });
});

// 404 handler
app.use((req, res) => {
  res.status(404).send(`
    <h1>📄 Page Not Found</h1>
    <p>The requested documentation page could not be found.</p>
    <p><a href="/">← Back to Documentation Home</a></p>
  `);
});

// Error handler
app.use((err, req, res, next) => {
  console.error('Documentation server error:', err);
  res.status(500).send(`
    <h1>❌ Server Error</h1>
    <p>An error occurred while serving the documentation.</p>
    <p><a href="/">← Back to Documentation Home</a></p>
  `);
});

// Start server
app.listen(PORT, () => {
  console.log(`
🚀 WRFCoin Documentation Server running on http://localhost:${PORT}

📖 Available Documentation:
${specs ? '✅' : '❌'} REST API Documentation: http://localhost:${PORT}/api-docs
${graphqlSchema ? '✅' : '❌'} GraphQL API: http://localhost:${PORT}/graphql  
📚 SDK Documentation: http://localhost:${PORT}/sdk-docs/js
📜 Contract Documentation: http://localhost:${PORT}/contract-docs
🏠 Documentation Home: http://localhost:${PORT}/

🔧 To generate missing documentation:
   npm run docs:generate
   npm run docs:sdk  
   npm run docs:contracts
  `);
});

// Graceful shutdown
process.on('SIGTERM', () => {
  console.log('🛑 Documentation server shutting down gracefully...');
  process.exit(0);
});

module.exports = app;
