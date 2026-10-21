// Flat config for ESLint 10.
// Migrated from legacy (implicit) ESLint config under eslint 8. Tracked in #1153.
//
// Scope:
//   - Lint TypeScript sources under this SDK only (sibling .js / .d.ts
//     artifacts that shipped in earlier versions are ignored — see #1155
//     / #1156 which cover the TS 6 build restoration).
//   - Ignore build output, node_modules, jest harness, and examples.
//
// @typescript-eslint v8.58.0 declares `eslint: ^8.57.0 || ^9.0.0 || ^10.0.0`
// in its peer range and is the version used here.
import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    ignores: [
      'dist/**',
      'node_modules/**',
      '**/*.js',
      '**/*.d.ts',
      'jest.config.js',
      'iot-devices/examples/**',
    ],
  },
  ...tseslint.configs.recommended,
  {
    files: ['**/*.ts'],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: 'module',
    },
    rules: {
      // Keep migration surface minimal — enforce the previous behavior.
      // New rule enforcement should land in a follow-up PR per #1153 non-goals.
      '@typescript-eslint/no-explicit-any': 'off',
      '@typescript-eslint/no-unused-vars': [
        'warn',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
      '@typescript-eslint/no-empty-object-type': 'off',
      '@typescript-eslint/no-unsafe-function-type': 'off',
      '@typescript-eslint/no-wrapper-object-types': 'off',
    },
  }
);
