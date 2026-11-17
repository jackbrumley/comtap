#!/usr/bin/env bash
set -euo pipefail

# ComTap uninstaller for Linux.
#   curl --proto '=https' --tlsv1.2 -sSf https://brumley.info/comtap/uninstall.sh | bash
#
# Stops and removes the comtap systemd service (its /dev links go with it) and
# /usr/local/bin/comtap. Leaves /etc/comtap (settings), /var/lib/comtap
# (status, traffic) and /var/log/comtap unless --purge-data or
# COMTAP_PURGE_DATA=1. Asks for your password with sudo.

BIN_PATH="/usr/local/bin/comtap"
UNIT_PATH="/etc/systemd/system/comtap.service"

log() { printf "[comtap-uninstall] %s\n" "$*"; }
fail() { printf "[comtap-uninstall] ERROR: %s\n" "$*" >&2; exit 1; }

usage() {
  cat <<EOF
ComTap uninstaller

Usage:
  uninstall.sh [--purge-data]

Options:
  --purge-data   Also delete settings, logs and traffic logs
  -h, --help     Show this help message
EOF
}

purge=false
if [[ "${COMTAP_PURGE_DATA:-}" == "1" ]]; then
  purge=true
fi
while [[ $# -gt 0 ]]; do
  case "$1" in
    --purge-data) purge=true ;;
    -h|--help) usage; exit 0 ;;
    *) fail "unknown option: $1" ;;
  esac
  shift
done

[[ "$(uname -s)" == "Linux" ]] || fail "this uninstaller is for Linux."

run_as_root() {
  if [[ "$EUID" -eq 0 ]]; then "$@"; else sudo "$@"; fi
}

# shellcheck disable=SC2016 # $1..$3 expand in the root shell.
run_as_root bash -c '
  if [ -x "$1" ]; then
    "$1" uninstall || exit 1
  elif [ -f "$2" ]; then
    # No binary to do it: remove the service directly.
    systemctl disable --now comtap || true
    rm -f "$2"
    systemctl daemon-reload
  fi
  rm -f "$1"
  # The app menu entry and icon (`comtap launcher`).
  rm -f /usr/share/applications/info.brumley.comtap.desktop \
    /usr/share/icons/hicolor/256x256/apps/comtap.png
  if [ "$3" = true ]; then
    rm -rf /etc/comtap /var/lib/comtap /var/log/comtap
  fi
' bash "$BIN_PATH" "$UNIT_PATH" "$purge" || fail "the uninstall failed; see the output above"

log "ComTap is uninstalled."
if [[ "$purge" == true ]]; then
  log "Settings, logs and traffic logs were deleted."
else
  log "Settings stay in /etc/comtap, logs in /var/log/comtap. Use --purge-data to delete them."
fi
