# WRFCoin UI/UX Verification Suite

Comprehensive UI verification for the WRFCoin frontend following the Task 122 stability test.

## Overview

This test suite provides automated UI/UX verification covering:
- **425+ React Components**
- **45+ Pages/Routes**
- **Multiple Viewports** (Desktop, Laptop, Tablet, Mobile)
- **Accessibility Auditing** (WCAG compliance)
- **Visual Regression** (Screenshots)

## Directory Structure

```
ui-verification/
├── specs/                    # Test specifications (tracked)
│   ├── wallet-ux.spec.ts     # Wallet UX tests
│   ├── weather-ux.spec.ts    # Weather data UX tests
│   ├── defi-ux.spec.ts       # DeFi trading UX tests
│   ├── nft-ux.spec.ts        # NFT marketplace UX tests
│   ├── bridge-ux.spec.ts     # Cross-chain bridge UX tests
│   ├── governance-ux.spec.ts # Governance UX tests
│   └── page-audit.spec.ts    # Comprehensive page audits
├── utils/                    # Utilities (tracked)
│   ├── ux-helpers.ts         # UX verification helpers
│   ├── ui-global-setup.js    # Test setup
│   └── ui-global-teardown.js # Test teardown
├── playwright.config.ui.js  # Playwright configuration (tracked)
├── screenshots/             # Captured screenshots (gitignored)
├── traces/                  # Playwright traces (gitignored)
├── reports/                 # HTML reports (gitignored)
└── accessibility/           # A11y audit results (gitignored)
```

## Running Tests

### Prerequisites

1. Start the frontend dev server:
   ```bash
   cd frontend && npm run dev
   ```

2. The frontend should be running at `http://localhost:5000`

### Run All UI Verification Tests

```bash
npx playwright test --config=tests/e2e/ui-verification/playwright.config.ui.js
```

### Run Specific Test Files

```bash
# Wallet UX tests
npx playwright test tests/e2e/ui-verification/specs/wallet-ux.spec.ts --config=tests/e2e/ui-verification/playwright.config.ui.js

# Weather UX tests
npx playwright test tests/e2e/ui-verification/specs/weather-ux.spec.ts --config=tests/e2e/ui-verification/playwright.config.ui.js

# Page audit (all pages)
npx playwright test tests/e2e/ui-verification/specs/page-audit.spec.ts --config=tests/e2e/ui-verification/playwright.config.ui.js
```

### Run with Visible Browser

```bash
npx playwright test --headed --config=tests/e2e/ui-verification/playwright.config.ui.js
```

### Run Specific Viewport

```bash
# Desktop only
npx playwright test --project=desktop-chrome --config=tests/e2e/ui-verification/playwright.config.ui.js

# Mobile only
npx playwright test --project=mobile-iphone --config=tests/e2e/ui-verification/playwright.config.ui.js
```

### Generate HTML Report

```bash
npx playwright show-report tests/e2e/ui-verification/reports/html-report
```

## Test Categories

### 1. Critical User Flows

| Flow | Test File | Routes Covered |
|------|-----------|----------------|
| Wallet Operations | `wallet-ux.spec.ts` | `/wallet`, `/wallet-v2` |
| Weather Data | `weather-ux.spec.ts` | `/pop`, `/pop/dashboard`, `/IoT` |
| DeFi Trading | `defi-ux.spec.ts` | Swap, Liquidity interfaces |
| NFT Marketplace | `nft-ux.spec.ts` | `/nfts`, `/nft/create` |
| Cross-Chain Bridge | `bridge-ux.spec.ts` | `/bridge` |
| Governance | `governance-ux.spec.ts` | `/governance` |

### 2. UX Verification Points

Each test verifies:

- **Loading States**: Spinners, skeletons, progress indicators
- **Error States**: Clear error messages, recovery options
- **Empty States**: Helpful content when no data
- **Responsive Design**: Layout at all viewports
- **Button States**: Hover, active, disabled, loading
- **Form Validation**: Inline errors, required fields
- **Navigation**: Breadcrumbs, links, back navigation
- **Accessibility**: WCAG compliance, keyboard navigation

### 3. Viewports Tested

| Device | Dimensions | Test Project |
|--------|------------|--------------|
| Desktop | 1920x1080 | `desktop-chrome` |
| Firefox | 1440x900 | `desktop-firefox` |
| iPad Pro | 768x1024 | `tablet-ipad` |
| iPhone 13 | 390x844 | `mobile-iphone` |
| Pixel 5 | 393x851 | `mobile-android` |

## UX Helpers

The `ux-helpers.ts` module provides reusable verification utilities:

```typescript
import { UXVerifier, VIEWPORTS, testAllViewports } from '../utils/ux-helpers';

const verifier = new UXVerifier(page);

// Verify loading states
const loadingResult = await verifier.verifyLoadingStates();

// Verify error handling
const errorResult = await verifier.verifyErrorStates();

// Run accessibility audit
const a11yResult = await verifier.runAccessibilityAudit();

// Capture screenshot
await verifier.captureScreenshot('my-screenshot');

// Full page audit
const auditResult = await verifier.performFullAudit('page-name', 'desktop');
```

## Output Artifacts

All outputs are stored in gitignored directories:

### Screenshots
Location: `tests/e2e/ui-verification/screenshots/`

- Captured for every test
- Named with page and viewport: `wallet-desktop-loaded.png`
- Full-page screenshots by default

### Accessibility Reports
Location: `tests/e2e/ui-verification/accessibility/`

- JSON audit results per page
- Violations categorized by severity
- WCAG compliance details

### HTML Reports
Location: `tests/e2e/ui-verification/reports/`

- Interactive test reports
- Pass/fail statistics
- Screenshots attached to failures

### Traces
Location: `tests/e2e/ui-verification/traces/`

- Full Playwright traces
- Debug failed tests
- Network requests, console logs

## Accessibility Testing

The suite uses axe-core for automated accessibility auditing:

### Focus Areas
- Color contrast (WCAG AA)
- Keyboard navigation
- ARIA labels
- Focus management
- Form field labels
- Image alt text

### Severity Levels
- **Critical**: Must fix - blocks users
- **Serious**: Should fix - significantly impacts users
- **Moderate**: Consider fixing - may impact some users
- **Minor**: Nice to fix - minor impact

## Continuous Integration

For CI environments, set environment variables:

```bash
export CI=true
export UI_TEST_URL=http://localhost:5000
```

The config automatically adjusts for CI:
- Single worker
- Retry on failure
- JSON/JUnit reporters

## Adding New Tests

### 1. Create a new spec file

```typescript
// specs/my-feature-ux.spec.ts
import { test, expect } from '@playwright/test';
import { UXVerifier, VIEWPORTS } from '../utils/ux-helpers';

test.describe('My Feature UX', () => {
  let verifier: UXVerifier;

  test.beforeEach(async ({ page }) => {
    verifier = new UXVerifier(page);
  });

  test('should load correctly', async ({ page }) => {
    await page.goto('/my-feature');
    const loadingResult = await verifier.verifyLoadingStates();
    expect(loadingResult.hasLoadingIndicator).toBe(true);
    await verifier.captureScreenshot('my-feature-loaded');
  });
});
```

### 2. Add page to audit list

In `page-audit.spec.ts`, add to `PAGES_TO_AUDIT`:

```typescript
{ path: '/my-feature', name: 'my-feature', description: 'My Feature' },
```

## Troubleshooting

### Frontend not running
```
Error: net::ERR_CONNECTION_REFUSED
```
Start the frontend: `cd frontend && npm run dev`

### Tests timing out
Increase timeout in `playwright.config.ui.js`:
```javascript
timeout: 120000, // 2 minutes
```

### Screenshots not captured
Ensure output directories exist:
```bash
mkdir -p tests/e2e/ui-verification/{screenshots,traces,reports,accessibility}
```

## Related Documentation

- [Playwright Documentation](https://playwright.dev/docs/intro)
- [axe-core Rules](https://dequeuniversity.com/rules/axe/4.8)
- [WCAG Guidelines](https://www.w3.org/WAI/standards-guidelines/wcag/)
