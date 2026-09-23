#!/bin/sh
# ==============================================================================
# Systemd sleep/resume hook for MECHREVO laptops
# Target: /usr/lib/systemd/system-sleep/mechrevo-ctl-resume.sh
# Purpose: Re-arms hardware power profiles and battery thresholds after S3 / s2idle wakeup
# ==============================================================================

case "$1" in
  post)
    # Give EC and PCIe power rails a brief moment to finish resume power-on
    sleep 0.5

    # Locate binary
    CTL_BIN=""
    if command -v mechrevo-ctl >/dev/null 2>&1; then
      CTL_BIN="mechrevo-ctl"
    elif [ -x /usr/local/bin/mechrevo-ctl ]; then
      CTL_BIN="/usr/local/bin/mechrevo-ctl"
    elif [ -x /usr/bin/mechrevo-ctl ]; then
      CTL_BIN="/usr/bin/mechrevo-ctl"
    fi

    if [ -n "$CTL_BIN" ]; then
      # Wait up to 1.5s for daemon socket to be ready after wakeup
      for _ in 1 2 3 4 5; do
        if [ -S /run/mechrevo-ctl.sock ]; then
          break
        fi
        sleep 0.3
      done
      $CTL_BIN reload-profile >/dev/null 2>&1 || true
    fi
    ;;
esac
