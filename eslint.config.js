import tsPlugin from '@typescript-eslint/eslint-plugin';
import tsParser from '@typescript-eslint/parser';

// This gate deliberately enforces defect-oriented rules only. Formatting is
// owned by Prettier and tracked separately in #2443; duplicating those rules
// here made ESLint permanently red on the existing tree.
const defectRules = {
  'no-constant-binary-expression': 'error',
  'no-dupe-else-if': 'error',
  'no-duplicate-case': 'error',
  'no-import-assign': 'error',
  'no-loss-of-precision': 'error',
  'no-new-native-nonconstructor': 'error',
  'no-obj-calls': 'error',
  'no-self-assign': 'error',
  'no-self-compare': 'error',
  'no-setter-return': 'error',
  'no-unreachable-loop': 'error',
  'no-unsafe-finally': 'error',
  'no-unsafe-negation': 'error',
  'no-unsafe-optional-chaining': 'error',
  'no-useless-backreference': 'error',
  'require-yield': 'error',
  'use-isnan': 'error',
  'valid-typeof': 'error',
};

const parserOptions = {
  ecmaFeatures: {
    jsx: true,
  },
  ecmaVersion: 'latest',
  sourceType: 'module',
};

export default [
  {
    linterOptions: {
      // Generated SDKs carry blanket disable comments for their upstream
      // generator. Those comments are not debt in this narrow rule set.
      reportUnusedDisableDirectives: 'off',
    },
  },
  // A standalone ignores object is global under ESLint flat-config semantics.
  {
    ignores: [
      '.claude/**',
      'node_modules/**',
      'dist/**',
      'build/**',
      'coverage/**',
      'blockchain-core/target/**',
      '**/*.min.js',
    ],
  },
  {
    files: ['**/*.{js,jsx,mjs,cjs}'],
    languageOptions: {
      ecmaVersion: 'latest',
      parserOptions,
      sourceType: 'module',
    },
    rules: defectRules,
  },
  {
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      parser: tsParser,
      parserOptions,
    },
    plugins: {
      '@typescript-eslint': tsPlugin,
    },
    rules: defectRules,
  },
];
