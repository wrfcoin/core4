# WRFCoin Code Commenting Standards & Documentation Templates

This guide establishes comprehensive code commenting standards and documentation templates for the
WRFCoin platform, ensuring consistency, clarity, and maintainability across all codebases.

## Overview

Code comments serve multiple purposes in the WRFCoin platform:

- **Explain Intent**: Why the code exists and what business problem it solves
- **Document Complexity**: Clarify non-obvious algorithms and logic
- **Provide Context**: Business rules, regulatory requirements, and domain knowledge
- **Guide Maintenance**: Help future developers (including yourself) understand and modify code
- **Enable Automation**: Support automated documentation generation

## Language-Specific Standards

### [TypeScript/JavaScript Standards](./typescript-standards.md)

- JSDoc comments for functions, classes, and modules
- Inline comments for complex business logic
- API endpoint documentation
- React component documentation

### [Rust Standards](./rust-standards.md)

- Documentation comments (`///` and `//!`)
- Inline comments for complex algorithms
- Blockchain-specific documentation patterns
- Performance-critical code annotations

### [Solidity Standards](./solidity-standards.md)

- NatSpec documentation for smart contracts
- Security-focused commenting
- Gas optimization explanations
- Compliance and audit annotations

### Python Standards (Coming Soon)

- Docstrings for functions, classes, and modules
- Type hints and documentation
- Data science and analytics comments
- Script and automation documentation

## Documentation Templates

### [Function Documentation Templates](./templates/function-templates.md)

Standardized templates for documenting functions across all languages.

### [Class Documentation Templates](./templates/class-templates.md)

Comprehensive templates for class and object documentation.

### [Module Documentation Templates](./templates/module-templates.md)

High-level module and package documentation standards.

### [API Documentation Templates](./templates/api-templates.md)

REST API, GraphQL, and WebSocket endpoint documentation.

### [Smart Contract Templates](./templates/contract-templates.md)

Blockchain smart contract documentation with security focus.

## Automated Validation

### [Comment Linting Rules](./validation/comment-linting.md)

Automated tools and rules for validating comment quality and completeness.

### [Documentation Generation](./validation/doc-generation.md)

Automated documentation generation from code comments.

### [Quality Metrics](./validation/quality-metrics.md)

Measuring and improving documentation coverage and quality.

## Best Practices

### Comment Quality Guidelines

**DO:**

- ✅ Explain _why_ the code exists, not _what_ it does
- ✅ Document business rules and domain knowledge
- ✅ Explain non-obvious algorithms and optimizations
- ✅ Include examples for complex APIs
- ✅ Document security considerations
- ✅ Reference relevant specifications or standards
- ✅ Keep comments up-to-date with code changes

**DON'T:**

- ❌ State the obvious ("increment counter")
- ❌ Leave outdated or incorrect comments
- ❌ Use comments to disable code (use version control)
- ❌ Include sensitive information (passwords, keys)
- ❌ Write comments that duplicate type information
- ❌ Use comments for temporary debugging

### Security and Compliance

**Required Comments:**

- Security-sensitive operations
- Compliance-related code (GDPR, SOC2, etc.)
- HSM interactions
- Cryptographic operations
- Data validation and sanitization
- Authorization and permission checks

**Example:**

```typescript
/**
 * Validates user input for weather station registration
 *
 * SECURITY: Input validation prevents SQL injection and XSS attacks
 * COMPLIANCE: GDPR Article 5(1)(d) - data accuracy requirement
 *
 * @param input Raw user input from registration form
 * @returns Validated and sanitized station data
 * @throws ValidationError if input fails security checks
 */
async function validateStationInput(input: unknown): Promise<WeatherStationData> {
  // Implementation with detailed security comments
}
```

### Domain-Specific Guidelines

**Weather Data Processing:**

```typescript
/**
 * Processes raw weather sensor data using WMO standards
 *
 * DOMAIN: Implements WMO-No. 8 meteorological codes
 * ACCURACY: Data validated against NOAA quality control procedures
 * PERFORMANCE: Optimized for real-time processing (< 100ms target)
 */
```

**Blockchain Operations:**

```rust
/// Validates blockchain transaction according to consensus rules
///
/// CONSENSUS: Implements modified Proof-of-Stake with weather data validation
/// SECURITY: Double-spending protection via UTXO verification
/// PERFORMANCE: O(log n) complexity using merkle tree validation
```

**Carbon Credit Trading:**

```solidity
/**
 * @notice Calculates carbon credit value based on weather impact
 * @dev Implements ISO 14064-2 greenhouse gas quantification standard
 *
 * COMPLIANCE: ISO 14064-2:2019 for GHG project accounting
 * AUDIT: Function verified by third-party carbon accounting firm
 *
 * @param weatherData Verified weather measurements
 * @param baseline Historical carbon baseline for comparison
 * @return carbonCredits Number of verified carbon credits earned
 */
```

## Comment Maintenance

### Automated Checks

```bash
# Run comment quality checks
npm run lint:comments

# Generate documentation coverage report
npm run docs:coverage

# Validate comment accuracy
npm run validate:comments

# Update auto-generated documentation
npm run docs:generate
```

### Review Process

1. **Code Review**: Comments reviewed alongside code changes
2. **Documentation Review**: Quarterly review of comment accuracy
3. **Quality Metrics**: Track documentation coverage and quality scores
4. **Community Feedback**: Developer feedback on comment helpfulness

## Integration with Development Workflow

### IDE Integration

**VS Code Extensions:**

- Better Comments: Enhanced comment highlighting
- Document This: Auto-generate JSDoc comments
- Rust Analyzer: Rust documentation support
- Solidity: Smart contract documentation

**Configuration:**

```json
{
  "editor.quickSuggestions": {
    "comments": true
  },
  "files.associations": {
    "*.md": "markdown"
  }
}
```

### Git Hooks

Pre-commit hooks validate comment quality:

```bash
#!/bin/bash
# .git/hooks/pre-commit
npm run lint:comments || {
    echo "❌ Comment quality check failed"
    echo "Please improve comments before committing"
    exit 1
}
```

### CI/CD Integration

```yaml
# .github/workflows/documentation.yml
name: Documentation Quality
on: [push, pull_request]
jobs:
  validate-comments:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Validate Comments
        run: |
          npm run lint:comments
          npm run docs:coverage
          npm run validate:docs
```

## Training and Resources

### Onboarding Materials

- [Comment Writing Tutorial](./training/comment-tutorial.md)
- [Language-Specific Examples](./training/examples/)
- [Common Mistakes Guide](./training/common-mistakes.md)

### Advanced Topics

- [Documentation Architecture](./advanced/doc-architecture.md)
- [API Documentation Best Practices](./advanced/api-docs.md)
- [Internationalization](./advanced/i18n.md)

### Community Resources

- **Discord**: #documentation-help
- **Office Hours**: Weekly documentation Q&A sessions
- **Mentorship**: Senior developer guidance on commenting standards

## Metrics and Improvement

### Documentation Quality Metrics

**Coverage Metrics:**

- Function documentation coverage: >90%
- Class documentation coverage: >95%
- Public API documentation: 100%
- Complex algorithm documentation: >80%

**Quality Metrics:**

- Comment accuracy (outdated comments): <5%
- Developer satisfaction with documentation: >4.0/5.0
- Time to understand new code (with good docs): <30 minutes

**Tools:**

```bash
# Generate metrics dashboard
npm run docs:metrics

# Coverage report
npm run docs:coverage:html

# Quality assessment
npm run docs:quality:report
```

### Continuous Improvement

**Monthly Reviews:**

- Documentation coverage analysis
- Developer feedback collection
- Comment quality assessment
- Process improvement recommendations

**Quarterly Assessments:**

- Documentation architecture review
- Tool effectiveness evaluation
- Training program updates
- Best practice refinements

---

## Quick Reference

### Comment Types by Purpose

| Purpose           | TypeScript     | Rust              | Solidity         | Python            |
| ----------------- | -------------- | ----------------- | ---------------- | ----------------- |
| Public API        | `/** JSDoc */` | `/// Doc comment` | `/** @notice */` | `"""Docstring"""` |
| Internal Logic    | `// Inline`    | `// Inline`       | `// Inline`      | `# Inline`        |
| Complex Algorithm | `/* Block */`  | `/* Block */`     | `/* Block */`    | `"""Block"""`     |
| Security Note     | `// SECURITY:` | `// SECURITY:`    | `// AUDIT:`      | `# SECURITY:`     |
| TODO/FIXME        | `// TODO:`     | `// TODO:`        | `// TODO:`       | `# TODO:`         |

### Required Documentation

- [ ] All public functions and methods
- [ ] All classes and interfaces
- [ ] All modules and packages
- [ ] All API endpoints
- [ ] All smart contracts
- [ ] Complex algorithms (>10 lines)
- [ ] Security-sensitive code
- [ ] Performance-critical sections
- [ ] Business logic and domain rules
- [ ] Configuration and setup code

---

**Need Help?**

- Read the [Style Guide](../STYLE_GUIDE.md) for general documentation standards
- Join [#documentation-help](https://discord.gg/wrfcoin-docs) on Discord
- Check the [FAQ](../troubleshooting/faq/development.md) for common questions

**Last Updated**: January 2025  
**Version**: 1.0.0  
**Maintained by**: WRFCoin Documentation Team
