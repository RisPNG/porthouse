#!/usr/bin/env bash
# Run inside a clean container after the porthouse package has been installed from the file the
# release would ship. Proves that the dependencies are enough and the installed programs work.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT

for program in porthouse-server porthouse-clientd porthouse-client; do
  "$program" --version
done
for file in /usr/lib/systemd/system/porthouse-server.service /usr/lib/systemd/system/porthouse-client.service \
            /usr/share/applications/porthouse.desktop /usr/share/icons/hicolor/scalable/apps/porthouse.svg \
            /usr/lib/modules-load.d/porthouse.conf /etc/xdg/autostart/porthouse.desktop; do
  [ -f "$file" ] || { echo "missing $file"; exit 1; }
done
getent group porthouse > /dev/null || { echo "group porthouse was not created"; exit 1; }

# The program starts and draws its window (no display needed with the offscreen platform).
for tab in use share activity; do
  QT_QPA_PLATFORM=offscreen porthouse --screenshot "$W/$tab.png" --tab "$tab"
  [ -s "$W/$tab.png" ] || { echo "the $tab tab drew nothing"; exit 1; }
done
echo "the program drew all three tabs"

# The installed programs pass the security suite.
PORTHOUSE_BIN_DIR=/usr/bin "$ROOT/scripts/e2e-security.sh"
