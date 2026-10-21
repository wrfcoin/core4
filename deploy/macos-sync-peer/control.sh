#!/usr/bin/env bash
set -euo pipefail
readonly LABEL='org.wrfcoin.sync-peer'
domain="gui/$(id -u)"
readonly DOMAIN="$domain"
readonly PLIST_PATH="$HOME/Library/LaunchAgents/$LABEL.plist"
case "${1:-}" in
  start) launchctl bootstrap "$DOMAIN" "$PLIST_PATH"; launchctl kickstart -k "$DOMAIN/$LABEL" ;;
  status) launchctl print "$DOMAIN/$LABEL" ;;
  stop) launchctl bootout "$DOMAIN/$LABEL" ;;
  uninstall) launchctl bootout "$DOMAIN/$LABEL" 2>/dev/null || true; rm -f "$PLIST_PATH"; printf 'uninstalled %s; peer data was retained\n' "$LABEL" ;;
  *) printf 'usage: %s {start|status|stop|uninstall}\n' "$0" >&2; exit 64 ;;
esac
