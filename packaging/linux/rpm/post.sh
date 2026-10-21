getent group netusb >/dev/null || groupadd --system netusb
if [ -d /run/systemd/system ]; then
    systemctl daemon-reload || true
    systemctl enable netusb-server.service netusb-client.service || true
    systemctl restart netusb-server.service netusb-client.service || true
fi
exit 0
