# 📋 Pull Request

## 🎯 Description

Brief description of changes and motivation for this pull request.

**Related Issue(s):**

- Closes #[issue_number]
- Related to #[issue_number]
- Fixes #[issue_number]

## 🔧 Type of Change

- [ ] 🐛 Bug fix (non-breaking change that fixes an issue)
- [ ] ✨ New feature (non-breaking change that adds functionality)
- [ ] 💥 Breaking change (fix or feature that would cause existing functionality to not work as
      expected)
- [ ] 📚 Documentation update
- [ ] ⚡ Performance improvement
- [ ] 🔒 Security enhancement
- [ ] 🔄 Refactoring (no functional changes)
- [ ] 🧪 Test improvements
- [ ] 🚀 CI/CD improvements

## 🧪 Testing Checklist

- [ ] **Unit tests** added/updated and passing
- [ ] **Integration tests** added/updated and passing
- [ ] **E2E tests** added/updated and passing (if applicable)
- [ ] **Manual testing** completed
- [ ] **Performance testing** completed (if applicable)
- [ ] **Security testing** completed (if applicable)

**Test Coverage:**

- Current coverage: [%]
- Coverage change: [+/-]%

**Testing Evidence:**

```bash
# Paste test results here
npm run test:all
# Test output...
```

## 📚 Documentation

- [ ] Code comments added/updated
- [ ] API documentation updated
- [ ] README.md updated (if applicable)
- [ ] CHANGELOG.md updated
- [ ] Migration guide created (if breaking changes)
- [ ] JSDoc/rustdoc documentation updated

## 🔒 Security Checklist

- [ ] No sensitive data exposed in code
- [ ] Input validation implemented where needed
- [ ] Authentication/authorization properly implemented
- [ ] Security best practices followed
- [ ] Dependencies are secure and up-to-date
- [ ] No hardcoded secrets or credentials

**Security Scan Results:**

```bash
# Paste security scan results if applicable
npm run security:audit
# Results...
```

## ⚡ Performance Impact

- [ ] No significant performance impact
- [ ] Performance improvement included
- [ ] Performance regression possible (explain below)
- [ ] Performance testing completed

**Performance Metrics:**

- Response time: [before/after]
- Memory usage: [before/after]
- Throughput: [before/after]
- Database query performance: [before/after]

**Performance Evidence:**

```bash
# Paste performance test results if applicable
npm run test:performance
# Results...
```

## 🌍 Climate/Environmental Impact

How does this change relate to WRFCoin's climate mission?

- [ ] Improves weather data accuracy/processing
- [ ] Enhances carbon credit tracking/trading
- [ ] Supports environmental monitoring
- [ ] Enables climate research capabilities
- [ ] Optimizes energy efficiency
- [ ] Not directly related to climate features

## 💥 Breaking Changes

**Are there breaking changes?**

- [ ] Yes, breaking changes included
- [ ] No breaking changes

**If yes, describe the breaking changes and migration path:**

```markdown
### Breaking Changes

1. [Describe breaking change 1]
   - **Migration:** [How to migrate]
2. [Describe breaking change 2]
   - **Migration:** [How to migrate]
```

## 🖼️ Screenshots/Recordings

**UI Changes (if applicable):**

<!-- Add screenshots or recordings of UI changes -->

**Before:** [Screenshot/recording of before state]

**After:** [Screenshot/recording of after state]

## 🔄 Deployment Considerations

- [ ] Database migrations included
- [ ] Environment variables updated
- [ ] Configuration changes documented
- [ ] Infrastructure changes required
- [ ] Feature flags considered
- [ ] Rollback plan documented

**Deployment Notes:**

```markdown
### Pre-deployment Steps

1. [Step 1]
2. [Step 2]

### Post-deployment Verification

1. [Verification step 1]
2. [Verification step 2]

### Rollback Procedure

1. [Rollback step 1]
2. [Rollback step 2]
```

## 🧪 Quality Assurance

**Code Quality Checks:**

```bash
# Paste quality check results
npm run quality:check
# Results...
```

**Linting Results:**

- [ ] TypeScript/JavaScript linting passed
- [ ] Rust linting (clippy) passed
- [ ] Solidity linting passed (if applicable)

**Formatting:**

- [ ] Code properly formatted (Prettier/rustfmt)
- [ ] No formatting violations

## 🔍 Review Checklist

**Self-Review Completed:**

- [ ] I have reviewed my own code
- [ ] I have checked for console.log/debug statements
- [ ] I have removed commented-out code
- [ ] I have optimized imports and dependencies
- [ ] I have verified error handling

**Code Quality:**

- [ ] Code follows project conventions
- [ ] Functions are reasonably sized and focused
- [ ] Variable and function names are descriptive
- [ ] Complex logic is well-commented
- [ ] Error handling is comprehensive

## 📊 Impact Analysis

**Components Affected:**

- [ ] Frontend (React/TypeScript)
- [ ] Backend API (Node.js/TypeScript)
- [ ] Blockchain Core (Rust)
- [ ] Smart Contracts (Solidity)
- [ ] Database Schema
- [ ] Infrastructure/DevOps
- [ ] Documentation
- [ ] CI/CD Pipeline

**Systems Integration:**

- [ ] Weather Data Processing
- [ ] Cross-Chain Bridge Operations
- [ ] DeFi/Carbon Credit Trading
- [ ] Governance System
- [ ] IoT Networks
- [ ] Storage Networks
- [ ] Authentication/Authorization

## 🎯 Success Criteria

How will we know this change is successful?

- [ ] All tests pass
- [ ] No performance regression
- [ ] Security scans clear
- [ ] User acceptance criteria met
- [ ] Documentation complete

## 🤝 Collaboration

**Additional Context:** [Any additional information that would help reviewers understand this
change]

**Questions for Reviewers:**

1. [Question 1]
2. [Question 2]

**Areas of Concern:** [Any areas you'd like reviewers to pay special attention to]

## ✅ Final Checklist

- [ ] I have read the [contributing guidelines](CONTRIBUTING.md)
- [ ] I have performed a self-review of my code
- [ ] I have added tests that prove my fix is effective or that my feature works
- [ ] I have added necessary documentation (if appropriate)
- [ ] My changes generate no new warnings
- [ ] I have checked my code and corrected any misspellings
- [ ] All CI checks are passing
- [ ] I have resolved any merge conflicts
- [ ] If I added a signed type or an integration test target, I updated its registry/inventory
      (`shared-protocol/src/crypto/domain_impls.rs`'s registry count / `scripts/integration_test_ratchet.json`) — see #3145

---

**Reviewer Guidelines:**

- Focus on code quality, security, and performance
- Verify all tests pass and coverage is maintained
- Check that documentation is updated appropriately
- Ensure changes align with project goals and standards
- Validate that breaking changes are properly documented
