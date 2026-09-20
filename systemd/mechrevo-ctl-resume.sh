#!/bin/sh
# ==============================================================================
# Systemd sleep/resume hook for MECHREVO laptops
# Target: /usr/lib/systemd/system-sleep/mechrevo-ctl-resume.sh
# Purpose: Re-arms hardware power profiles and battery thresholds after S3 / s2idle wakeup
# ==============================================================================

case "$1" in
  post)
    # Give EC and kernel drivers a brief moment to finish hardware power-on sequence
    sleep 0.5

    # Request daemon to re-apply the last active profile
    if [ -e /usr/local/bin/python3 ]; then
      PYTHON_BIN="/usr/local/bin/python3"
    else
      PYTHON_BIN="python3"
    fi

    $PYTHON_BIN -c '
import json, socket
try:
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.settimeout(2.0)
        s.connect("/run/mechrevo-ctl.sock")
        s.sendall(json.dumps({"action": "reload_profile"}).encode("utf-8"))
except Exception:
    pass
' 2>/dev/null || true
    ;;
esac

