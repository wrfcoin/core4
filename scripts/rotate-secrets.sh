#!/usr/bin/env bash
#
# rotate-secrets.sh -- Interactive secret rotation helper for WRFCoin testnet
#
# Usage: ./scripts/rotate-secrets.sh
#
# Supports rotation of:
#   1. JWT_SECRET       (auto-generates new value, prompts to apply)
#   2. ENCRYPTION_KEY   (guided manual process -- blocks auto-rotation)
#   3. DB password      (prints ALTER USER command, prompts to apply)
#
# See docs/ops/secrets-rotation.md for the full procedure.

set -euo pipefail

COMPOSE_CMD="docker compose -f docker-compose.yml -f docker-compose.testnet.yml -f docker-compose.override.yml"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

bold()  { printf '\033[1m%s\033[0m' "$*"; }
warn()  { printf '\033[1;33m[WARN]\033[0m %s\n' "$*"; }
error() { printf '\033[1;31m[ERROR]\033[0m %s\n' "$*"; }
info()  { printf '\033[1;34m[INFO]\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m[OK]\033[0m %s\n' "$*"; }

confirm() {
    local prompt="${1:-Continue?}"
    printf '%s [y/N] ' "$prompt"
    read -r answer
    case "$answer" in
        [yY]|[yY][eE][sS]) return 0 ;;
        *) return 1 ;;
    esac
}

# ---------------------------------------------------------------------------
# JWT_SECRET rotation
# ---------------------------------------------------------------------------

rotate_jwt_secret() {
    info "Rotating JWT_SECRET"
    echo ""
    warn "This will invalidate ALL active user sessions."
    warn "Every logged-in client will need to re-authenticate."
    echo ""

    if ! confirm "Proceed with JWT_SECRET rotation?"; then
        info "Aborted."
        return
    fi

    local new_secret
    new_secret=$(openssl rand -base64 64 | tr -d '\n')

    echo ""
    info "New JWT_SECRET value (copy this):"
    echo ""
    echo "  $new_secret"
    echo ""
    info "Update your .env file (or docker-compose.override.yml):"
    echo ""
    echo "  JWT_SECRET=$new_secret"
    echo ""

    if ! confirm "Have you updated the .env file with the new JWT_SECRET?"; then
        warn "Rotation incomplete. Update .env before restarting services."
        return
    fi

    echo ""
    if confirm "Restart the backend container now?"; then
        info "Restarting backend..."
        if $COMPOSE_CMD restart backend; then
            ok "Backend restarted."
        else
            error "Failed to restart backend. Run manually:"
            echo "  $COMPOSE_CMD restart backend"
            return 1
        fi

        info "Checking health..."
        sleep 3
        if curl -sf http://localhost:4001/health > /dev/null 2>&1; then
            ok "Backend health check passed."
        else
            warn "Health check failed or timed out. Verify manually:"
            echo "  curl -sf http://localhost:4001/health | jq ."
        fi
    else
        info "Skipped restart. Remember to restart the backend before the new secret takes effect."
    fi

    echo ""
    ok "JWT_SECRET rotation complete."
}

# ---------------------------------------------------------------------------
# ENCRYPTION_KEY rotation (manual only -- too dangerous to automate)
# ---------------------------------------------------------------------------

rotate_encryption_key() {
    info "ENCRYPTION_KEY rotation (guided manual process)"
    echo ""
    error "Auto-rotation is BLOCKED for ENCRYPTION_KEY."
    error "Rotating this key without re-encrypting stored data first will"
    error "make all encrypted data (wallet keys, sensitive config) PERMANENTLY"
    error "UNREADABLE."
    echo ""
    info "Follow these steps manually:"
    echo ""
    echo "  1. Generate a new key:"
    echo ""
    echo "     NEW_KEY=\$(openssl rand -base64 32)"
    echo "     echo \"New ENCRYPTION_KEY: \$NEW_KEY\""
    echo ""
    echo "  2. Re-encrypt stored data BEFORE updating the env var:"
    echo ""
    echo "     export OLD_ENCRYPTION_KEY=\"<current value from .env>\""
    echo "     export NEW_ENCRYPTION_KEY=\"\$NEW_KEY\""
    echo "     $COMPOSE_CMD exec backend node scripts/re-encrypt-data.js"
    echo ""
    echo "  3. Back up the database before and after re-encryption."
    echo ""
    echo "  4. Update .env with the new ENCRYPTION_KEY."
    echo ""
    echo "  5. Restart the backend:"
    echo ""
    echo "     $COMPOSE_CMD restart backend"
    echo ""
    echo "  6. Verify the backend can read previously encrypted data:"
    echo ""
    echo "     curl -sf http://localhost:4001/health | jq ."
    echo ""
    info "See docs/ops/secrets-rotation.md for the full procedure."
}

# ---------------------------------------------------------------------------
# Database password rotation
# ---------------------------------------------------------------------------

rotate_db_password() {
    info "Rotating database password"
    echo ""
    warn "The database password must be changed in PostgreSQL AND in .env"
    warn "at the same time. If they fall out of sync, the backend cannot"
    warn "connect to the database."
    echo ""

    if ! confirm "Proceed with database password rotation?"; then
        info "Aborted."
        return
    fi

    local db_user
    printf 'Database user [wrfcoin]: '
    read -r db_user
    db_user="${db_user:-wrfcoin}"

    local new_pass
    new_pass=$(openssl rand -base64 32 | tr -d '=+/')

    echo ""
    info "New password generated (copy this):"
    echo ""
    echo "  $new_pass"
    echo ""
    info "Step 1: Run this SQL command in the PostgreSQL container:"
    echo ""
    echo "  $COMPOSE_CMD exec postgres psql -U $db_user -c \"ALTER USER $db_user WITH PASSWORD '$new_pass';\""
    echo ""

    if ! confirm "Have you run the ALTER USER command above?"; then
        warn "Rotation incomplete. Run the ALTER USER command before updating .env."
        return
    fi

    echo ""
    info "Step 2: Update .env with the new password:"
    echo ""
    echo "  DATABASE_URL=postgresql://$db_user:$new_pass@postgres:5432/wrfcoin"
    echo "  POSTGRES_PASSWORD=$new_pass"
    echo ""
    warn "Also update docker-compose.override.yml if passwords are set there."
    echo ""

    if ! confirm "Have you updated .env (and compose overrides) with the new password?"; then
        warn "Rotation incomplete. Update .env before restarting services."
        return
    fi

    echo ""
    if confirm "Restart the compose stack now?"; then
        info "Restarting backend first..."
        if $COMPOSE_CMD restart backend; then
            ok "Backend restarted."
        else
            error "Failed to restart backend."
            return 1
        fi

        info "Checking health..."
        sleep 3
        if curl -sf http://localhost:4001/health > /dev/null 2>&1; then
            ok "Backend health check passed."
        else
            warn "Health check failed. You may need a full stack restart:"
            echo "  $COMPOSE_CMD down && $COMPOSE_CMD up -d"
        fi
    else
        info "Skipped restart. Remember to restart services for the new password to take effect."
    fi

    echo ""
    ok "Database password rotation complete."
}

# ---------------------------------------------------------------------------
# Main menu
# ---------------------------------------------------------------------------

main() {
    echo ""
    bold "WRFCoin Testnet Secrets Rotation"
    echo ""
    echo "Which secret do you want to rotate?"
    echo ""
    echo "  1) JWT_SECRET        (invalidates all user sessions)"
    echo "  2) ENCRYPTION_KEY    (requires re-encryption -- guided manual)"
    echo "  3) Database password  (coordinated ALTER USER + env update)"
    echo "  q) Quit"
    echo ""
    printf 'Choice [1/2/3/q]: '
    read -r choice

    case "$choice" in
        1) rotate_jwt_secret ;;
        2) rotate_encryption_key ;;
        3) rotate_db_password ;;
        q|Q) info "Exited." ;;
        *)
            error "Invalid choice: $choice"
            exit 1
            ;;
    esac
}

main "$@"
