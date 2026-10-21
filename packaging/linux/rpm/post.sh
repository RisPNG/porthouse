#!/bin/sh
getent group porthouse >/dev/null || groupadd --system porthouse
if [ -d /run/systemd/system ]; then
    systemctl daemon-reload || true
    systemctl enable porthouse-server.service porthouse-client.service || true
    systemctl restart porthouse-server.service porthouse-client.service || true
fi
exit 0
