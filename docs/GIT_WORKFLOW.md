# 🔄 Git Workflow Guide

## Overview

WRFCoin uses **GitHub Flow** - a lightweight, branch-based workflow that supports teams and projects
where deployments are made regularly.

## 🌿 Branch Strategy

### Main Branches

- **`main`** - Production-ready code, always deployable
- **`develop`** - Integration branch for features (optional, used for major releases)

### Feature Branches

- **`feature/`** - New features and enhancements
- **`fix/`** - Bug fixes
- **`hotfix/`** - Critical production fixes
- **`docs/`** - Documentation improvements
- **`refactor/`** - Code refactoring
- **`perf/`** - Performance improvements
- **`security/`** - Security enhancements

## 🚀 Workflow Process

### 1. Start New Work

```bash
# Update main branch
git checkout main
git pull upstream main

# Create feature branch
git checkout -b feature/your-feature-name

# Or for different types:
git checkout -b fix/issue-description
git checkout -b docs/readme-improvements
git checkout -b perf/api-optimization
```

### 2. Development

```bash
# Make changes and commit frequently
git add .
git commit -m "feat: implement user authentication

- Add JWT token validation
- Implement middleware for protected routes
- Add user session management

Closes #123"

# Push to your fork
git push origin feature/your-feature-name
```

### 3. Keep Branch Updated

```bash
# Regularly sync with main
git fetch upstream
git rebase upstream/main

# Or merge if you prefer
git merge upstream/main

# Push updates
git push origin feature/your-feature-name --force-with-lease
```

### 4. Create Pull Request

1. **Push your branch** to your fork
2. **Create PR** on GitHub
3. **Fill out PR template** completely
4. **Request reviews** from appropriate teams
5. **Address feedback** and update

### 5. Merge and Cleanup

```bash
# After PR is merged, clean up
git checkout main
git pull upstream main
git branch -d feature/your-feature-name
git push origin --delete feature/your-feature-name
```

## 📝 Commit Message Format

### Conventional Commits

> **PR titles are CI-enforced; this section is not the source of truth for
> them.** The `preflight` job validates every PR title against the `types:`
> and `scopes:` lists in `.github/workflows/ci-readiness.yml` — which differ
> from the commit-message habits below (for PR titles there is no `security:`
> *type*; `security` is a *scope*, so use `fix(security): …`, and an issue
> number is never a valid scope — `feat(#123): …` fails). See
> [CONTRIBUTING.md § PR Title Format](../CONTRIBUTING.md#pr-title-format-enforced-by-ci)
> for the enforced grammar and the allowed lists.

Follow the [Conventional Commits](https://www.conventionalcommits.org/) specification:

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

### Types

- **`feat:`** - New features
- **`fix:`** - Bug fixes
- **`docs:`** - Documentation changes
- **`style:`** - Code style changes (formatting, missing semicolons, etc.)
- **`refactor:`** - Code refactoring (no functional changes)
- **`perf:`** - Performance improvements
- **`test:`** - Adding or updating tests
- **`chore:`** - Build process or auxiliary tool changes
- **`security:`** - Security improvements
- **`ci:`** - CI/CD changes

### Scopes (Optional)

- **`api`** - Backend API changes
- **`ui`** - Frontend UI changes
- **`blockchain`** - Blockchain core changes
- **`contracts`** - Smart contract changes
- **`docs`** - Documentation changes
- **`deps`** - Dependency updates

### Examples

```bash
# Simple feature
git commit -m "feat: add user profile endpoint"

# With scope and description
git commit -m "feat(api): implement JWT authentication middleware

- Add token validation middleware
- Create secure route protection
- Implement refresh token mechanism

Closes #456"

# Breaking change
git commit -m "feat!: restructure API response format

BREAKING CHANGE: API responses now include metadata object
Migration guide available in docs/migration/v2.0.md"

# Security fix
git commit -m "security: fix SQL injection vulnerability

- Sanitize user input in search queries
- Add prepared statements
- Update input validation

CVE-2024-12345"
```

## 🔀 Branch Naming

### Format

```
<type>/<short-description>
```

### Examples

```bash
# Features
feature/user-authentication
feature/weather-data-api
feature/carbon-credit-trading

# Bug fixes
fix/login-validation-error
fix/memory-leak-in-processor
fix/cross-chain-bridge-timeout

# Documentation
docs/api-reference-update
docs/contributing-guidelines
docs/deployment-guide

# Performance
perf/database-query-optimization
perf/frontend-bundle-size
perf/consensus-algorithm

# Security
security/hsm-key-rotation
security/input-validation
security/dependency-update

# Refactoring
refactor/payment-processor
refactor/user-service
refactor/blockchain-consensus
```

## 🔄 Release Process

### 1. Feature Release

```bash
# Create release branch
git checkout -b release/v1.2.0

# Update version numbers
npm version minor  # or major/patch
cd blockchain-core && cargo update

# Update CHANGELOG.md
# Test thoroughly
npm run test:all
npm run build

# Create release PR
# After approval, merge to main
# Tag release
git tag v1.2.0
git push origin v1.2.0
```

### 2. Hotfix Process

```bash
# Create hotfix from main
git checkout main
git checkout -b hotfix/critical-security-fix

# Make minimal changes
# Test thoroughly
npm run test:all
npm run security:audit

# Create urgent PR
# Fast-track review
# Merge and tag
git tag v1.1.1
git push origin v1.1.1
```

## 🔒 Security Considerations

### Sensitive Information

```bash
# Never commit secrets
echo "*.env" >> .gitignore
echo "*.key" >> .gitignore
echo "*.pem" >> .gitignore

# Use environment variables
export API_KEY="your-secret-key"

# For team secrets, use vault or secrets manager
```

### Signing Commits

```bash
# Set up GPG signing
git config --global user.signingkey YOUR_GPG_KEY
git config --global commit.gpgsign true

# Sign commits
git commit -S -m "feat: add secure feature"
```

## 🧪 Pre-Commit Checks

### Quality Gates

```bash
# Before committing, run:
npm run quality:check        # Linting and formatting
npm run test:all            # All tests
npm run security:audit      # Security scan
npm run build              # Build validation
```

### Git Hooks

```bash
# Install pre-commit hooks
npm install husky --save-dev
npx husky install

# Add pre-commit hook
npx husky add .husky/pre-commit "npm run quality:check"
npx husky add .husky/pre-push "npm run test:all"
```

## 🔄 Rebase vs Merge

### When to Rebase

```bash
# Clean up local commits before pushing
git rebase -i HEAD~3

# Keep feature branch up to date
git rebase upstream/main
```

### When to Merge

```bash
# Preserve merge history for important features
git merge upstream/main

# For collaboration on feature branches
git merge feature/shared-work
```

## 🚨 Emergency Procedures

### Critical Security Fix

```bash
# 1. Create hotfix immediately
git checkout main
git checkout -b hotfix/security-critical

# 2. Make minimal fix
# 3. Test thoroughly
npm run security:comprehensive

# 4. Create emergency PR
# 5. Request security team review
# 6. Fast-track merge
# 7. Deploy immediately
```

### Rollback Process

```bash
# 1. Identify problematic commit
git log --oneline

# 2. Create revert commit
git revert <commit-hash>

# 3. Or rollback to previous version
git checkout <previous-tag>
git checkout -b hotfix/rollback-v1.1.0

# 4. Emergency deploy
```

## 🎯 Best Practices

### Commit Frequency

- **Commit often** with logical, atomic changes
- **One feature per commit** when possible
- **Test before committing**
- **Write descriptive commit messages**

### Branch Management

- **Keep branches short-lived** (< 1 week)
- **Delete merged branches** promptly
- **Sync regularly** with main
- **Use descriptive branch names**

### Code Review

- **Self-review first** before requesting review
- **Request appropriate reviewers** based on CODEOWNERS
- **Respond to feedback** promptly
- **Keep PRs focused** and reasonably sized

### Documentation

- **Update docs** with code changes
- **Include examples** in commit messages
- **Link to issues** in commits and PRs
- **Explain the why**, not just the what

## 🛠️ Git Configuration

### Setup

```bash
# Configure user information
git config --global user.name "Your Name"
git config --global user.email "your.email@example.com"

# Set default branch name
git config --global init.defaultBranch main

# Set up aliases
git config --global alias.co checkout
git config --global alias.br branch
git config --global alias.ci commit
git config --global alias.st status
git config --global alias.unstage 'reset HEAD --'
git config --global alias.last 'log -1 HEAD'
git config --global alias.visual '!gitk'

# Set up better logging
git config --global alias.lg "log --color --graph --pretty=format:'%Cred%h%Creset -%C(yellow)%d%Creset %s %Cgreen(%cr) %C(bold blue)<%an>%Creset' --abbrev-commit"
```

### Useful Aliases

```bash
# Add to ~/.gitconfig
[alias]
    # Quick commands
    s = status
    co = checkout
    br = branch
    ci = commit

    # Logging
    lg = log --graph --pretty=format:'%Cred%h%Creset %C(yellow)%d%Creset %s %Cgreen(%cr)%Creset' --abbrev-commit --date=relative
    oneline = log --pretty=oneline --abbrev-commit --graph

    # Useful shortcuts
    unstage = reset HEAD
    uncommit = reset --soft HEAD^
    recommit = commit --amend --no-edit

    # Branch management
    branches = branch -a
    remotes = remote -v

    # Cleanup
    cleanup = "!git branch --merged | grep -v '\\*\\|main\\|develop' | xargs -n 1 git branch -d"
```

## 📊 Monitoring and Metrics

### Branch Analytics

```bash
# Check branch status
git branch -vv

# See what's ahead/behind
git status -sb

# Check remote tracking
git remote show origin
```

### Contribution Metrics

```bash
# See commit statistics
git shortlog -sn --since="1 month ago"

# File change frequency
git log --name-only --pretty=format: | sort | uniq -c | sort -nr

# Lines of code changes
git log --shortstat --since="1 month ago"
```

---

This workflow ensures **code quality**, **security**, and **collaboration** while maintaining the
**professional standards** expected for enterprise blockchain development.
