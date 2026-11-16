#!/usr/bin/env bash
# Release install test (Linux): with no software on the port, ComTap must stay
# connected to the device, report the port as not open, and drop the data
# quietly (no reopen or reconnect loop). On failure, prints the status and log.
set -uo pipefail
port="$1"
sleep "${2:-8}"

status=/var/lib/comtap/status.json
line="$(python3 - "$port" "$status" <<'EOF'
import json, sys
port, path = sys.argv[1], sys.argv[2]
for l in json.load(open(path))["links"]:
    if l["com"] == port:
        print(l["state"], l.get("software_open"), l["connects"], l["rx_bytes"])
EOF
)"
echo "$port: state software_open connects rx_bytes = $line"
read -r state open _ rx <<<"$line"
problems=()
[[ "$state" == "connected" ]] || problems+=("state is '$state', not connected")
[[ "$open" == "False" ]] || problems+=("software_open is '$open', not False")
[[ "${rx:-0}" -gt 0 ]] || problems+=("no data came from the device")
if grep -qE "Reopening|failed to write" /var/log/comtap/comtap-*.log; then
  problems+=("the log shows port reopen errors")
fi

if [[ ${#problems[@]} -eq 0 ]]; then
  echo "$port idle check passed"
  exit 0
fi
printf 'FAIL: %s\n' "${problems[@]}"
echo "--- log"; tail -40 /var/log/comtap/comtap-*.log
echo "--- journal"; sudo journalctl -u comtap --no-pager -n 40
exit 1
