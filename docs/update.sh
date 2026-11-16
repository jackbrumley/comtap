#!/usr/bin/env bash
# ComTap's in-app updater for Linux.
#
# F1 -> Update and `comtap update` (from 1.1.1 on) download the current copy
# of this script and run it in the same terminal, so this file owns the whole
# update. A fix made here reaches every installed version on its next update,
# with no new release. ComTap only starts bash.
#
# Contract with the app; keep it stable, every shipped version relies on it:
#   - Arguments: --base-url <where this script was downloaded from, without a
#     trailing slash> and --relaunch <the ComTap binary that started it>.
#     Unknown arguments are ignored, so new ones can be added.
#   - The first output line is exactly "[comtap-update] started".
#   - Output goes to the terminal; install.sh writes /var/log/comtap/update.log.
#   - The last line is "[comtap-update] finished: updated" (exit 0) or
#     "[comtap-update] finished: failed" (exit 1). On success the new version
#     opens in this terminal; on failure the installed version opens with
#     --update-failed, which shows the user a warning.
#   - COMTAP_NO_RELAUNCH=1 opens nothing afterwards (the release test).

echo "[comtap-update] started"

base_url=""
relaunch=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --base-url) base_url="${2:-}"; shift 2 || shift ;;
    --relaunch) relaunch="${2:-}"; shift 2 || shift ;;
    *) echo "[comtap-update] Ignoring unknown argument: $1"; shift ;;
  esac
done
echo "[comtap-update] $(date -u +%Y-%m-%dT%H:%M:%SZ) from ${base_url}"

BIN_PATH="/usr/local/bin/comtap"
UNIT_PATH="/etc/systemd/system/comtap.service"

# ComTap is a terminal app: it can only open again if there is a terminal.
can_open() {
  [[ "${COMTAP_NO_RELAUNCH:-}" != "1" && "${COMTAP_NO_RELAUNCH:-}" != "true" ]] \
    && { : </dev/tty >/dev/tty; } 2>/dev/null
}

finish_failed() {
  echo "[comtap-update] finished: failed"
  if [[ -n "$relaunch" && -x "$relaunch" ]] && can_open; then
    echo "[comtap-update] Opening the installed version"
    exec "$relaunch" --update-failed </dev/tty >/dev/tty 2>&1
  fi
  exit 1
}

if [[ -z "$base_url" ]]; then
  echo "[comtap-update] ERROR: --base-url is required"
  finish_failed
fi

installer="$(mktemp "${TMPDIR:-/tmp}/comtap-update-install.XXXXXX")"
if ! curl --proto '=https' --tlsv1.2 -fsSL --retry 3 --connect-timeout 10 "${base_url}/install.sh" -o "$installer"; then
  rm -f "$installer"
  echo "[comtap-update] ERROR: could not download ${base_url}/install.sh"
  finish_failed
fi

# --no-relaunch: install.sh would otherwise replace itself with ComTap's
# screen, and this script would see the screen's exit status as the update's.
bash "$installer" --yes --no-relaunch
status=$?
rm -f "$installer"
if [[ "$status" -ne 0 ]]; then
  echo "[comtap-update] Installer exited with status ${status}"
  finish_failed
fi

echo "[comtap-update] finished: updated"
if can_open; then
  if [[ -f "$UNIT_PATH" ]]; then
    exec "$BIN_PATH" </dev/tty >/dev/tty 2>&1
  fi
  # Not set up yet: the setup screen needs root.
  exec sudo "$BIN_PATH" </dev/tty >/dev/tty 2>&1
fi
exit 0
