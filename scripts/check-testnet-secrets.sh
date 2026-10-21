#!/usr/bin/env bash
set -euo pipefail

# Scan tracked files only so local operator .env files are not inspected.
# Prevents committed plaintext values for sensitive environment variables
# while allowing placeholders, variable references, and generated-at-runtime examples.
#
# Usage:
#   bash scripts/check-testnet-secrets.sh           # normal scan
#   bash scripts/check-testnet-secrets.sh --self-test  # run built-in assertion suite

secret_names='WRFCOIN_OPERATOR_TOKEN|JWT_SECRET|OAUTH_STATE_SECRET|POSTGRES_PASSWORD|REDIS_PASSWORD|WRFCOIN_SNAPSHOT_SIGNER_PRIVATE_KEY|FAUCET_WALLET_PRIVKEY|API_KEY_ENCRYPTION_KEY|HCAPTCHA_SECRET|RECAPTCHA_SECRET'
assignment_regex="^[[:space:]]*(#[[:space:]]*)?(export[[:space:]]+)?(${secret_names})[[:space:]]*="
failure_count=0

trim() {
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "$value"
}

strip_wrapping_quotes() {
  local value="$1"
  if [[ "$value" =~ ^\".*\"$ || "$value" =~ ^\'.*\'$ ]]; then
    value="${value:1:${#value}-2}"
  fi
  printf '%s' "$value"
}

is_allowed_value() {
  local value="$1"

  value="$(trim "$value")"

  if [[ "$value" == *\\ ]]; then
    value="${value%\\}"
    value="$(trim "$value")"
  fi

  # Unquoted inline comments are documentation, not part of the value.
  if [[ "$value" != \"* && "$value" != \'* ]]; then
    value="${value%%#*}"
    value="$(trim "$value")"
  fi

  value="$(strip_wrapping_quotes "$value")"

  case "$value" in
    "")
      return 0  # Empty assignment commits no secret value
      ;;
    \$* | \${* | \$\(*)
      return 0
      ;;
    \<*\>)
      return 0
      ;;
    REPLACE_* | replace_* | replace-* | REPLACE-* | \
    GENERATE_* | generate_* | inject_* | INJECT_* | \
    placeholder* | PLACEHOLDER* | example* | EXAMPLE* | \
    your_* | your-* | YOUR_* | YOUR-* | \
    changeme* | CHANGEME* | change_me* | CHANGE_ME* | change-me* | CHANGE-ME* | \
    set_me* | SET_ME*)
      return 0
      ;;
  esac

  if [[ "$value" =~ ^[0-9a-f]{64}$ ]]; then
    return 1
  fi

  return 1
}

run_self_test() {
  local errors=0 passed=0

  assert_allowed() {
    local desc="$1" val="$2"
    if is_allowed_value "$val"; then
      passed=$((passed + 1))
    else
      echo "FAIL [allowed]: $desc -> value=$(printf '%q' "$val")"
      errors=$((errors + 1))
    fi
  }

  assert_flagged() {
    local desc="$1" val="$2"
    if ! is_allowed_value "$val"; then
      passed=$((passed + 1))
    else
      echo "FAIL [flagged]: $desc -> value=$(printf '%q' "$val")"
      errors=$((errors + 1))
    fi
  }

  # Values that must NOT trigger a failure.
  assert_allowed "empty (VAR=)"                        ""
  assert_allowed "simple variable ref"                 '$JWT_SECRET'
  assert_allowed "braced variable ref"                 '${POSTGRES_PASSWORD}'
  assert_allowed "command substitution"                '$(openssl rand -hex 32)'
  assert_allowed "angle-bracket placeholder"           '<generate-with-openssl-rand-hex-32>'
  assert_allowed "REPLACE_ prefix"                     'REPLACE_WITH_REAL_VALUE'
  assert_allowed "replace_ lowercase"                  'replace_me'
  assert_allowed "replace- hyphenated"                 'replace-with-local-jwt-secret'
  assert_allowed "GENERATE_ uppercase"                 'GENERATE_STRONG_RANDOM_SECRET_MIN_256_BITS'
  assert_allowed "generate_ lowercase"                 'generate_32_bytes'
  assert_allowed "inject_ prefix"                      'inject_at_deploy'
  assert_allowed "placeholder prefix"                  'placeholder'
  assert_allowed "example prefix"                      'example-secret'
  assert_allowed "your_ prefix"                        'your_postgres_password'
  assert_allowed "your- hyphenated"                    'your-secret'
  assert_allowed "your- longer hyphenated"             'your-secure-jwt-secret-here'
  assert_allowed "changeme prefix"                     'changeme'
  assert_allowed "change-me hyphenated"                'change-me-local-only'
  assert_allowed "change_me underscore"                'change_me_in_production'
  assert_allowed "set_me prefix"                       'set_me_before_deploy'
  assert_allowed "whitespace around empty"             '   '
  assert_allowed "quoted empty"                        '""'
  assert_allowed "comment-only after equals"           ' # fill this in'

  # Values that MUST trigger a failure.
  assert_flagged "64-char hex (real token shape)"      'a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2'
  assert_flagged "arbitrary password string"           'mysupersecretpassword123'
  assert_flagged "quoted password string"              '"my-postgres-pass!9x"'
  assert_flagged "JWT-shaped string"                   'eyJhbGciOiJIUzI1NiJ9.payload.signature'
  assert_flagged "private key looking value"           'K1aXqrs7DEADBEEF0099aabbccddeeff'

  echo ""
  if (( errors == 0 )); then
    echo "Self-test PASSED - $passed/$((passed + errors)) assertions"
    return 0
  else
    echo "Self-test FAILED - $errors failure(s), $passed passed"
    return 1
  fi
}

if [[ "${1:-}" == "--self-test" ]]; then
  echo "Running is_allowed_value assertion suite..."
  run_self_test
  exit $?
fi

while IFS=: read -r file line content; do
  [[ -n "${file:-}" ]] || continue

  value="${content#*=}"
  if is_allowed_value "$value"; then
    continue
  fi

  if (( failure_count == 0 )); then
    echo "Detected committed secret assignment(s) that are not explicit placeholders or env-derived values:"
  fi

  printf '  %s:%s: %s\n' "$file" "$line" "$content"
  failure_count=$((failure_count + 1))
done < <(git grep -nI -E "$assignment_regex" -- . || true)

if (( failure_count > 0 )); then
  cat <<'EOF'

Secrets covered by this scan:
  WRFCOIN_OPERATOR_TOKEN, JWT_SECRET, OAUTH_STATE_SECRET, POSTGRES_PASSWORD,
  REDIS_PASSWORD, WRFCOIN_SNAPSHOT_SIGNER_PRIVATE_KEY, FAUCET_WALLET_PRIVKEY,
  API_KEY_ENCRYPTION_KEY, HCAPTCHA_SECRET, RECAPTCHA_SECRET

Tracked assignments must use one of:
  - an explicit placeholder such as <generate_with_openssl_rand_hex_32>
  - a documented template such as generate_with_openssl_rand_hex_32
  - an environment or command substitution such as $JWT_SECRET or $(openssl rand -hex 32)
  - an empty value (VAR=)

Live secrets must stay in untracked local env files or approved secret storage.
EOF
  exit 1
fi

echo "No committed plaintext secrets detected in tracked files. (Scanned: ${secret_names//|/, })"
