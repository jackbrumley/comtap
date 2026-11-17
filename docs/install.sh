#!/usr/bin/env bash
set -euo pipefail

# ComTap one-line installer for Linux.
#   curl --proto '=https' --tlsv1.2 -sSf https://brumley.info/comtap/install.sh | bash
# The in-app updater re-runs this same script, so keep its options stable.
# Follows the Release and Update Standard; see docs/RELEASE.md.
#
# ComTap runs as a systemd service, so it always installs system-wide:
#   /usr/local/bin/comtap, config in /etc/comtap, logs in /var/log/comtap.
# On a PC with ComTap set up, it updates the binary and restarts the service
# (devices and settings stay). Otherwise it opens ComTap's setup screen.

REPO="jackbrumley/comtap"
BIN_NAME="comtap"
BIN_PATH="/usr/local/bin/${BIN_NAME}"
UNIT_PATH="/etc/systemd/system/comtap.service"
INSTALL_BASE_URL="https://brumley.info/comtap"
LOG_DIR="/var/log/comtap"

# GitHub API request headers. COMTAP_GITHUB_TOKEN is optional: anonymous
# requests are limited to 60 an hour per IP address.
GITHUB_API_HEADERS=(-H "Accept: application/vnd.github+json")
if [[ -n "${COMTAP_GITHUB_TOKEN:-}" ]]; then
  GITHUB_API_HEADERS+=(-H "Authorization: Bearer ${COMTAP_GITHUB_TOKEN}")
fi

usage() {
  cat <<EOF
ComTap installer

Usage:
  install.sh [--version <tag>] [--no-relaunch] [--insecure-skip-verify]

Installs ComTap system-wide (asks for your password with sudo). On a PC where
ComTap is set up, it updates it and keeps your devices; otherwise it opens the
setup screen. Your user is added to the serial-port group (dialout, or uucp on
Arch) so your software can open ComTap's ports.

Options:
  --version <tag>         Install a specific release tag (e.g. v1.1.0)
  --no-relaunch           Do not open ComTap afterwards
  --insecure-skip-verify  Skip checksum verification (not recommended)
  -h, --help              Show this help message

Environment:
  COMTAP_GITHUB_TOKEN     GitHub token for API requests (avoids the anonymous rate limit)
  COMTAP_NO_RELAUNCH      Set to 1 to not open ComTap afterwards

Example:
  curl --proto '=https' --tlsv1.2 -sSf ${INSTALL_BASE_URL}/install.sh | bash
EOF
}

# Logged to a temp file first (root may not be available yet), then appended
# to ${LOG_DIR}/update.log at the end.
LOG_FILE="$(mktemp -t comtap-install.XXXXXX.log)"

log() {
  printf "[comtap] %s\n" "$*"
  printf "[%s] [comtap] %s\n" "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_FILE" 2>/dev/null || true
}

warn() {
  printf "[comtap] WARNING: %s\n" "$*" >&2
  printf "[%s] [comtap] WARNING: %s\n" "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_FILE" 2>/dev/null || true
}

fail() {
  printf "[comtap] ERROR: %b\n" "$*" >&2
  printf "[%s] [comtap] ERROR: %b\n" "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_FILE" 2>/dev/null || true
  keep_log
  exit 1
}

optional_cmd() {
  command -v "$1" >/dev/null 2>&1
}

need_cmd() {
  optional_cmd "$1" || fail "missing required command: $1"
}

run_as_root() {
  if [[ "$EUID" -eq 0 ]]; then
    "$@"
  else
    sudo "$@"
  fi
}

# Appends this run's log to the update log (best effort).
keep_log() {
  if [[ "$EUID" -eq 0 ]] || sudo -n true 2>/dev/null; then
    run_as_root mkdir -p "$LOG_DIR" 2>/dev/null || true
    # shellcheck disable=SC2016 # $1 and $2 expand in the root shell.
    run_as_root sh -c 'cat "$1" >> "$2"' sh "$LOG_FILE" "${LOG_DIR}/update.log" 2>/dev/null || true
  fi
}

resolve_arch() {
  case "$(uname -m)" in
    x86_64|amd64) printf "x64" ;;
    aarch64|arm64) fail "ARM64 builds of ComTap are not available yet; only x64 (x86_64) is supported." ;;
    *) fail "unsupported architecture: $(uname -m)" ;;
  esac
}

fetch_latest_release_tag() {
  local tag
  tag="$(curl -fsL --retry 3 --connect-timeout 10 "${GITHUB_API_HEADERS[@]}" \
    "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep -o '"tag_name": *"[^"]*"' | head -n1 | sed 's/.*"\([^"]*\)"$/\1/')" || true
  [[ -n "$tag" ]] || fail "could not determine the latest release from GitHub (you may be rate limited; try again in a few minutes)"
  printf "%s" "$tag"
}

# GitHub computes a SHA-256 digest for every uploaded release asset and
# exposes it through the releases API; that is the source of truth.
verify_checksum() {
  local file="$1" asset_name="$2"
  local release_json expected actual
  release_json="$(curl -fsL --retry 3 --connect-timeout 10 "${GITHUB_API_HEADERS[@]}" \
    "https://api.github.com/repos/${REPO}/releases/tags/${release_tag}")" \
    || fail "could not fetch the checksum for ${asset_name} from GitHub. Try again later, or re-run with --insecure-skip-verify."
  if optional_cmd python3; then
    expected="$(printf "%s" "$release_json" | python3 -c '
import json, sys
name = sys.argv[1]
for asset in json.load(sys.stdin).get("assets", []):
    if asset.get("name") == name:
        print(asset.get("digest") or "")
' "$asset_name" 2>/dev/null)" || true
  else
    # No JSON parser: each asset lists "digest" shortly before its
    # "browser_download_url", with only flat fields in between.
    expected="$(printf "%s" "$release_json" | tr -d '\n' \
      | grep -o "\"digest\": *\"sha256:[0-9a-f]*\"[^{}]*\"browser_download_url\": *\"[^\"]*/${asset_name//./\\.}\"" \
      | grep -o 'sha256:[0-9a-f]*' | head -n1)" || true
  fi
  expected="${expected#sha256:}"
  [[ -n "$expected" ]] || fail "GitHub did not provide a checksum for ${asset_name}. Re-run with --insecure-skip-verify to bypass verification."
  actual="$(sha256sum "$file" | awk '{print $1}')"
  [[ "$expected" == "$actual" ]] || fail "checksum mismatch (expected $expected, got $actual)"
  log "Checksum verified"
}

# ComTap's ports belong to the serial-port group (dialout, or uucp on Arch).
serial_group() {
  local group
  for group in dialout uucp; do
    if getent group "$group" >/dev/null 2>&1; then
      printf "%s" "$group"
      return 0
    fi
  done
  return 1
}

skip_verify=false
relaunch=true
if [[ "${COMTAP_NO_RELAUNCH:-}" == "1" || "${COMTAP_NO_RELAUNCH:-}" == "true" ]]; then
  relaunch=false
fi
version=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-relaunch) relaunch=false ;;
    --version)
      [[ $# -ge 2 ]] || fail "--version requires a value"
      version="$2"
      shift
      ;;
    --insecure-skip-verify) skip_verify=true ;;
    # Accepted for compatibility with the other apps' installers; ComTap never prompts.
    --yes|--system) ;;
    -h|--help) usage; exit 0 ;;
    *) fail "unknown option: $1" ;;
  esac
  shift
done

[[ "$(uname -s)" == "Linux" ]] || fail "this installer is for Linux. On Windows, see ${INSTALL_BASE_URL}/"
need_cmd curl
need_cmd sha256sum
need_cmd systemctl
if [[ "$EUID" -ne 0 ]]; then
  need_cmd sudo
fi

arch="$(resolve_arch)"
if [[ -n "$version" ]]; then
  release_tag="$version"
else
  release_tag="$(fetch_latest_release_tag)"
fi
asset_name="${BIN_NAME}-${release_tag#v}-linux-${arch}"
asset_url="https://github.com/${REPO}/releases/download/${release_tag}/${asset_name}"
log "Release: ${release_tag}"
log "Package: ${asset_name}"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"; rm -f "$LOG_FILE"' EXIT
download="${tmp_dir}/${asset_name}"

log "Downloading ${asset_url}"
curl -fL --retry 3 --connect-timeout 15 -o "$download" "$asset_url" \
  || fail "download failed: ${asset_name} was not found in release ${release_tag}."

if [[ "$skip_verify" == true ]]; then
  warn "checksum verification disabled"
else
  verify_checksum "$download" "$asset_name"
fi

if [[ "$EUID" -ne 0 ]]; then
  log "Installing system-wide; sudo asks for your password"
fi

user="${SUDO_USER:-$(id -un)}"
group=""
if group="$(serial_group)"; then
  if getent group "$group" | cut -d: -f4 | tr ',' '\n' | grep -qx "$user" || [[ "$user" == "root" ]]; then
    group=""
  fi
else
  warn "no dialout or uucp group found; software may need root to open ComTap's ports."
  group=""
fi

already_set_up=false
[[ -f "$UNIT_PATH" ]] && already_set_up=true

# One root call for the whole install, so sudo asks for the password once.
#  - install(1) replaces the file rather than writing into it, so a running
#    ComTap (the service or a screen) doesn't block it.
#  - On a PC that is set up, `comtap install` stops the service, rewrites the
#    unit, starts the service again and checks each device. Exit 3 means only
#    that step failed; the new binary is in place.
#  - Then the serial-port group, if needed, and the app menu entry and icon
#    (`comtap launcher`). Neither stops the other: exit 4 means only the group
#    failed, 5 only the menu entry, 6 both.
root_status=0
# shellcheck disable=SC2016 # $1..$5 expand in the root shell.
run_as_root bash -c '
  install -m 0755 "$1" "$2" || exit 1
  if [ "$3" = true ]; then
    "$2" install || exit 3
  fi
  status=0
  if [ -n "$4" ]; then
    usermod -aG "$4" "$5" || status=4
  fi
  if "$2" launcher; then
    if command -v update-desktop-database >/dev/null 2>&1; then
      update-desktop-database /usr/share/applications >/dev/null 2>&1 || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
      gtk-update-icon-cache -f -t -q /usr/share/icons/hicolor >/dev/null 2>&1 || true
    fi
  else
    status=$((status == 4 ? 6 : 5))
  fi
  exit "$status"
' bash "$download" "$BIN_PATH" "$already_set_up" "$group" "$user" || root_status=$?
case "$root_status" in
  0) ;;
  3) fail "ComTap ${release_tag} is installed at ${BIN_PATH}, but restarting the service failed; see the output above, or run: sudo comtap status" ;;
  4) warn "could not add ${user} to the ${group} group. Fix it with: sudo usermod -aG ${group} ${user}" ; group="" ;;
  5|6) warn "ComTap is installed, but adding it to the app menu failed; see the output above, or run: sudo comtap launcher" ;;&
  6) warn "could not add ${user} to the ${group} group. Fix it with: sudo usermod -aG ${group} ${user}" ; group="" ;;
  5) ;;
  *) fail "the install failed (exit ${root_status}); see the output above" ;;
esac
log "Installed ${BIN_PATH} ($("$BIN_PATH" --version))"
if [[ -n "$group" ]]; then
  log "Added ${user} to ${group}, so your software can open ComTap's ports. Log out and back in for it to take effect."
fi
if [[ "$already_set_up" == true ]]; then
  log "Update complete: ComTap ${release_tag#v} is running."
fi
keep_log

# ComTap is a terminal app, so it opens in this terminal. Piped into bash
# (curl | bash), stdin is the script, so its keys come from /dev/tty; without
# a terminal (CI, a detached updater) there is nothing to open it in.
if [[ "$relaunch" == true ]] && { : </dev/tty >/dev/tty; } 2>/dev/null; then
  rm -rf "$tmp_dir" "$LOG_FILE" # exec skips the EXIT trap
  if [[ "$already_set_up" == true ]]; then
    log "Opening ComTap"
    exec "$BIN_PATH" </dev/tty >/dev/tty 2>&1
  fi
  log "Opening ComTap's setup screen"
  exec sudo "$BIN_PATH" </dev/tty >/dev/tty 2>&1
fi
if [[ "$already_set_up" == false ]]; then
  log "Next: run 'sudo comtap' to add your devices and start the service."
fi
