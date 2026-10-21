#!/usr/bin/env bash
# Install netusb on this Linux machine.
#
#   sudo ./scripts/install-linux.sh server    share this machine's USB devices
#   sudo ./scripts/install-linux.sh client    use devices shared by other machines
#   sudo ./scripts/install-linux.sh both
#
# Builds in release mode first if needed, installs to /usr/local/bin and sets up systemd units.

set -eu
ROLE=${1:-}
case "$ROLE" in server|client|both) ;; *) echo "usage: sudo $0 server|client|both" >&2; exit 2;; esac
[ "$(id -u)" -eq 0 ] || { echo "run as root: sudo $0 $ROLE" >&2; exit 1; }

ROOT=$(cd "$(dirname "$0")/.." && pwd)
REL=$ROOT/target/release
if [ ! -x "$REL/netusb-server" ] || [ ! -x "$REL/netusb-client" ]; then
  [ -n "${SUDO_USER:-}" ] || { echo "build first: cargo build --release" >&2; exit 1; }
  echo "building (as $SUDO_USER)..."
  sudo -u "$SUDO_USER" -H bash -lc "cd '$ROOT' && cargo build --release"
fi

install -m 0755 "$REL/netusb-server" "$REL/netusb-client" /usr/local/bin/

if [ "$ROLE" = server ] || [ "$ROLE" = both ]; then
  install -m 0644 "$ROOT/packaging/netusb-server.service" /etc/systemd/system/
  systemctl daemon-reload
  systemctl enable --now netusb-server.service
  echo
  echo "Server running. To authorize a client, run here:  sudo netusb-server pair"
  echo "then on the client:  netusb-client pair <this-host>"
fi
if [ "$ROLE" = client ] || [ "$ROLE" = both ]; then
  echo vhci-hcd > /etc/modules-load.d/netusb.conf
  modprobe vhci-hcd || echo "warning: could not load vhci-hcd; install your kernel's USB/IP modules"
  install -m 0644 "$ROOT/packaging/netusb-client.service" /etc/systemd/system/
  systemctl daemon-reload
  systemctl enable --now netusb-client.service
  sleep 1
  echo
  echo "Client running. Open the web interface with:"
  echo "    sudo cat /var/lib/netusb/ui.url"
fi
