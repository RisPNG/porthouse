#!/bin/sh
if [ "$1" = 0 ] && [ -d /run/systemd/system ]; then
    systemctl disable --now porthouse-server.service porthouse-client.service || true
fi
exit 0
