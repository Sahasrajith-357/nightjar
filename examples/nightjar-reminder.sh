#!/usr/bin/env bash
# nightjar reminder — desktop notification reminding you to back up.
# Handles the cron environment, which lacks the desktop session vars.

export DISPLAY="${DISPLAY:-:0}"
if [ -z "$DBUS_SESSION_BUS_ADDRESS" ]; then
    uid="$(id -u)"
    export DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/${uid}/bus"
fi

notify-send -u normal -i drive-harddisk "nightjar" "Time to back up. Run nightjar-gui (or nightjar-cli backup)."
