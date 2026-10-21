#!/usr/bin/env bash
# Run inside a clean container after the netusb package has been installed from the file the
# release would ship. Proves that the dependencies are enough and the installed programs work.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT

for program in netusb-server netusb-clientd netusb-client; do
  "$program" --version
done
for file in /usr/lib/systemd/system/netusb-server.service /usr/lib/systemd/system/netusb-client.service \
            /usr/share/applications/netusb.desktop /usr/share/icons/hicolor/scalable/apps/netusb.svg \
            /usr/lib/modules-load.d/netusb.conf /etc/xdg/autostart/netusb.desktop; do
  [ -f "$file" ] || { echo "missing $file"; exit 1; }
done
getent group netusb > /dev/null || { echo "group netusb was not created"; exit 1; }

# The program starts and draws its window (no display needed with the offscreen platform).
for tab in use share activity; do
  QT_QPA_PLATFORM=offscreen netusb --screenshot "$W/$tab.png" --tab "$tab"
  [ -s "$W/$tab.png" ] || { echo "the $tab tab drew nothing"; exit 1; }
done
echo "the program drew all three tabs"

# The installed programs pass the security suite.
NETUSB_BIN_DIR=/usr/bin "$ROOT/scripts/e2e-security.sh"
