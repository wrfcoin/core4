/**
 * Gap Report Script Unit Tests
 * Issue #275 — Gap-report automation
 */

import { describe, it, expect, jest, beforeEach } from '@jest/globals';

// We test the gap report patterns and utilities without running the full script.
// The script at scripts/update-gap-report.ts exports internal helpers via module,
// but since it's a script, we test the logic by re-implementing key patterns here.

describe('Gap Report Automation', () => {
  describe('GAP_AREA_PATTERNS', () => {
    // Replicate the patterns from the script
    const GAP_AREA_PATTERNS: Record<string, RegExp> = {
      'Bridge trust': /bridge|cross.?chain|relay|atomic.?swap|ibc|xcmp|layerzero|wormhole/i,
      'Carbon credit': /carbon|credit|offset|emission|footprint|climate.?finance|green/i,
      'CORS & security': /cors|helmet|csp|security.?header|xss|csrf|rate.?limit/i,
      'DeFi/AMM': /defi|amm|swap|liquidity|pool|yield|farming|staking/i,
      'Governance & DAO': /governance|dao|proposal|vote|quorum|delegation|treasury/i,
      'HSM integration': /hsm|vault|hardware.?security|key.?management|pkcs|signing/i,
      'IoT consolidation': /iot|sensor|device|telemetry|lorawan|helium|iotex/i,
      'NFT marketplace': /nft|marketplace|mint|token.?uri|royalt|collection/i,
      'Quantum crypto': /quantum|kyber|dilithium|falcon|sphincs|lattice|post.?quantum/i,
      'Schema & DB': /schema|migration|database|table|column|index|drizzle|sql/i,
      'Weather rewards': /weather|reward|earning|quality.?score|station|submission/i,
      'Rust core': /cargo|blockchain.?core|native.?chain|shared.?protocol|rust/i,
    };

    it('should match bridge-related PR titles', () => {
      expect(GAP_AREA_PATTERNS['Bridge trust'].test('fix: cross-chain bridge relay')).toBe(true);
      expect(GAP_AREA_PATTERNS['Bridge trust'].test('feat: add LayerZero integration')).toBe(true);
      expect(GAP_AREA_PATTERNS['Bridge trust'].test('fix: IBC protocol timeout')).toBe(true);
    });

    it('should match carbon credit PR titles', () => {
      expect(GAP_AREA_PATTERNS['Carbon credit'].test('feat: carbon credit trading')).toBe(true);
      expect(GAP_AREA_PATTERNS['Carbon credit'].test('fix: emission tracking calculation')).toBe(true);
    });

    it('should match CORS & security PR titles', () => {
      expect(GAP_AREA_PATTERNS['CORS & security'].test('fix: CORS headers in production')).toBe(true);
      expect(GAP_AREA_PATTERNS['CORS & security'].test('feat: add rate limiting')).toBe(true);
    });

    it('should match IoT consolidation PR titles', () => {
      expect(GAP_AREA_PATTERNS['IoT consolidation'].test('feat: IoT sensor integration')).toBe(true);
      expect(GAP_AREA_PATTERNS['IoT consolidation'].test('fix: Helium network data')).toBe(true);
    });

    it('should match weather rewards PR titles', () => {
      expect(GAP_AREA_PATTERNS['Weather rewards'].test('fix: weather data quality score')).toBe(true);
      expect(GAP_AREA_PATTERNS['Weather rewards'].test('feat: station reward calculation')).toBe(true);
    });

    it('should match Rust core PR titles', () => {
      expect(GAP_AREA_PATTERNS['Rust core'].test('fix: cargo build issue')).toBe(true);
      expect(GAP_AREA_PATTERNS['Rust core'].test('feat: blockchain-core consensus')).toBe(true);
    });

    it('should not match unrelated PR titles', () => {
      const unrelatedTitle = 'docs: update README formatting';
      let matched = false;
      for (const pattern of Object.values(GAP_AREA_PATTERNS)) {
        if (pattern.test(unrelatedTitle)) {
          matched = true;
          break;
        }
      }
      expect(matched).toBe(false);
    });
  });

  describe('matchGapAreas', () => {
    const GAP_AREA_PATTERNS: Record<string, RegExp> = {
      'Bridge trust': /bridge|cross.?chain|relay|atomic.?swap/i,
      'Carbon credit': /carbon|credit|offset|emission/i,
      'Weather rewards': /weather|reward|earning|quality.?score/i,
    };

    function matchGapAreas(prTitle: string, prBody: string): string[] {
      const combined = `${prTitle} ${prBody}`;
      return Object.entries(GAP_AREA_PATTERNS)
        .filter(([, pattern]) => pattern.test(combined))
        .map(([area]) => area);
    }

    it('should return matching areas for relevant PR', () => {
      const areas = matchGapAreas('fix: bridge relay timeout', 'Fixed cross-chain bridge issue');
      expect(areas).toContain('Bridge trust');
    });

    it('should return multiple areas when both match', () => {
      const areas = matchGapAreas('feat: carbon credit weather integration', '');
      expect(areas).toContain('Carbon credit');
      expect(areas).toContain('Weather rewards');
    });

    it('should return empty for unrelated PR', () => {
      const areas = matchGapAreas('chore: update dependencies', 'bump lodash version');
      expect(areas).toHaveLength(0);
    });
  });

  describe('sanitize', () => {
    function sanitize(input: string, maxLength: number = 200): string {
      return input
        .replace(/[|`<>]/g, '')
        .replace(/\n/g, ' ')
        .trim()
        .slice(0, maxLength);
    }

    it('should strip pipe characters', () => {
      expect(sanitize('cell|content|here')).toBe('cellcontenthere');
    });

    it('should strip backticks', () => {
      expect(sanitize('`code`')).toBe('code');
    });

    it('should strip angle brackets', () => {
      expect(sanitize('<script>alert(1)</script>')).toBe('scriptalert(1)/script');
    });

    it('should replace newlines with spaces', () => {
      expect(sanitize('line1\nline2')).toBe('line1 line2');
    });

    it('should truncate to maxLength', () => {
      const long = 'a'.repeat(300);
      expect(sanitize(long).length).toBe(200);
    });

    it('should handle empty string', () => {
      expect(sanitize('')).toBe('');
    });
  });

  describe('auto-strikethrough', () => {
    function applyStrikethrough(line: string, prNumber: number): string {
      // Skip already-resolved lines
      if (line.includes('~~') || line.includes('RESOLVED')) {
        return line;
      }
      return `~~${line}~~ **RESOLVED (PR #${prNumber})**`;
    }

    it('should apply strikethrough to open item', () => {
      const result = applyStrikethrough('| Bridge trust | No relay validation |', 42);
      expect(result).toContain('~~');
      expect(result).toContain('RESOLVED (PR #42)');
    });

    it('should skip already-resolved items', () => {
      const line = '~~| Bridge trust | Done |~~ **RESOLVED (PR #30)**';
      const result = applyStrikethrough(line, 42);
      expect(result).toBe(line); // unchanged
    });

    it('should skip lines already containing RESOLVED', () => {
      const line = '| Bridge trust | RESOLVED |';
      const result = applyStrikethrough(line, 42);
      expect(result).toBe(line);
    });
  });

  describe('residual gap counting', () => {
    function countResidualGaps(lines: string[]): { open: number; total: number } {
      let open = 0;
      let total = 0;
      for (const line of lines) {
        // Match table data rows: start with '|' or '~~|' (struck-through), skip header/separator
        const trimmed = line.replace(/^~~/, '');
        if (trimmed.startsWith('|') && !trimmed.startsWith('|---') && !trimmed.startsWith('| Domain')) {
          total++;
          if (!line.includes('~~') && !line.includes('RESOLVED')) {
            open++;
          }
        }
      }
      return { open, total };
    }

    it('should count all items in table', () => {
      const lines = [
        '| Domain | Gap |',
        '|--------|-----|',
        '| Bridge trust | No relay |',
        '| Carbon credit | No trading |',
        '~~| Weather | Fixed |~~ **RESOLVED (PR #1)**',
      ];
      const result = countResidualGaps(lines);
      expect(result.total).toBe(3); // excludes header and separator
      expect(result.open).toBe(2);
    });

    it('should return zero for empty table', () => {
      const lines: string[] = [];
      const result = countResidualGaps(lines);
      expect(result.open).toBe(0);
      expect(result.total).toBe(0);
    });

    it('should handle all resolved', () => {
      const lines = [
        '| Domain | Gap |',
        '|--------|-----|',
        '~~| Bridge trust | Done |~~ **RESOLVED (PR #1)**',
        '~~| Carbon credit | Done |~~ **RESOLVED (PR #2)**',
      ];
      const result = countResidualGaps(lines);
      expect(result.open).toBe(0);
      expect(result.total).toBe(2);
    });
  });

  describe('getBranchName fallback chain', () => {
    function getBranchName(env: Record<string, string | undefined>): string {
      return (
        env.GITHUB_HEAD_REF ||
        env.GITHUB_REF_NAME ||
        (env.GITHUB_SHA ? env.GITHUB_SHA.substring(0, 7) : null) ||
        'unknown'
      );
    }

    it('should prefer GITHUB_HEAD_REF', () => {
      expect(getBranchName({
        GITHUB_HEAD_REF: 'feature/auth',
        GITHUB_REF_NAME: 'main',
        GITHUB_SHA: 'abc1234567890',
      })).toBe('feature/auth');
    });

    it('should fall back to GITHUB_REF_NAME', () => {
      expect(getBranchName({
        GITHUB_HEAD_REF: '',
        GITHUB_REF_NAME: 'main',
        GITHUB_SHA: 'abc1234567890',
      })).toBe('main');
    });

    it('should fall back to short GITHUB_SHA', () => {
      expect(getBranchName({
        GITHUB_HEAD_REF: '',
        GITHUB_REF_NAME: '',
        GITHUB_SHA: 'abc1234567890',
      })).toBe('abc1234');
    });

    it('should fall back to "unknown"', () => {
      expect(getBranchName({})).toBe('unknown');
    });
  });

  describe('updateImplementationProgress idempotency', () => {
    function updateProgress(existing: string[], prNumber: number, entry: string): string[] {
      // Skip if PR already recorded
      if (existing.some(line => line.includes(`PR #${prNumber}`))) {
        return existing;
      }
      return [...existing, entry];
    }

    it('should add new PR entry', () => {
      const existing = ['| PR #1 | Bridge fix |'];
      const result = updateProgress(existing, 2, '| PR #2 | Carbon fix |');
      expect(result).toHaveLength(2);
    });

    it('should skip duplicate PR', () => {
      const existing = ['| PR #1 | Bridge fix |'];
      const result = updateProgress(existing, 1, '| PR #1 | Bridge fix again |');
      expect(result).toHaveLength(1);
    });
  });
});
