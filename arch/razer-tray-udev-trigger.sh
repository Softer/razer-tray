#!/bin/bash
# Helper invoked by 99-razer-tray.rules on Razer mouse hotplug.
# Starts razer-tray.service for every active non-root user session.
# A no-op if the service is already running for that user.

set -u

while read -r user; do
    [[ -z "$user" || "$user" == "root" ]] && continue
    uid=$(id -u "$user" 2>/dev/null) || continue
    runuser -u "$user" -- env XDG_RUNTIME_DIR="/run/user/$uid" \
        systemctl --user start razer-tray.service >/dev/null 2>&1 &
done < <(loginctl --no-legend list-users 2>/dev/null | awk '{print $2}')

wait
