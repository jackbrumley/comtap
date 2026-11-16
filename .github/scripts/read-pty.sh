#!/usr/bin/env bash
# Release install test (Linux): reads one line from a ComTap port and checks it,
# and that the port is open to the serial-port group. On failure, prints
# ComTap's status, log and journal, so the run says why.
set -uo pipefail
link="$1"
expect="$2"

target="$(readlink -f "$link")"
perms="$(stat -c '%G %a' "$target" 2>/dev/null)"
echo "$link -> $target ($perms)"

line="$(sudo timeout 20 python3 - "$link" <<'EOF'
import os, sys
fd = os.open(sys.argv[1], os.O_RDWR | os.O_NOCTTY)
buf = b""
while b"\n" not in buf:
    buf += os.read(fd, 64)
print(buf.decode(errors="replace").strip())
EOF
)"
echo "$link received: $line"

ok=true
[[ "$line" == *"$expect"* ]] || ok=false
[[ "$perms" == "dialout 660" ]] || { echo "FAIL: expected the port to be 'dialout 660', got '$perms'"; ok=false; }
if [[ "$ok" == true ]]; then
  exit 0
fi
echo "--- comtap status"; comtap status
echo "--- log"; tail -40 /var/log/comtap/comtap-*.log
echo "--- journal"; sudo journalctl -u comtap --no-pager -n 40
exit 1
