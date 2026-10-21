#!/usr/bin/env bash
# Loopback attach test for porthouse.
#
# Starts the server, has the client claim one local USB device and re-attach it through the
# kernel's vhci_hcd, so the device keeps working while its traffic goes through porthouse.
#
#   sudo ./scripts/live-test.sh [BUSID] [SECONDS]
#
# BUSID defaults to 5-1 (list yours with: ./target/debug/porthouse-client list while the test runs).
# After SECONDS (default 30) everything is torn down and the device is handed back to the host.
# Use a device you can afford to lose for that long (a second mouse is ideal), and keep a
# keyboard within reach.

set -u
BUSID=${1:-5-1}
SECS=${2:-30}
PORT=7620
ROOT=$(cd "$(dirname "$0")/.." && pwd)
. "$ROOT/scripts/lib.sh"
OUT=${TMPDIR:-/tmp}/porthouse-live
SP=""; CP=""

if [ "$(id -u)" -ne 0 ]; then echo "run as root: sudo $0 $*" >&2; exit 1; fi
[ -x "$SRV" ] && [ -x "$CLI" ] && [ -x "$CLID" ] || { echo "build first: cargo build" >&2; exit 1; }
mkdir -p "$OUT"; : > "$OUT/server.log"; : > "$OUT/client.log"

restore() {
  # Whatever happened, make sure the real device gets its host driver back.
  [ -n "$CP" ] && kill -TERM "$CP" 2>/dev/null
  [ -n "$SP" ] && kill -TERM "$SP" 2>/dev/null
  sleep 2
  for i in /sys/bus/usb/devices/"$BUSID":*; do
    [ -e "$i" ] && echo "$(basename "$i")" > /sys/bus/usb/drivers_probe 2>/dev/null
  done
  echo
  echo "=== final state ==="
  for i in /sys/bus/usb/devices/"$BUSID":*; do
    [ -e "$i" ] && echo "$(basename "$i") driver: $(readlink "$i/driver" 2>/dev/null | sed 's|.*/||')"
  done
  rm -rf "$T"
  echo "logs: $OUT/server.log  $OUT/client.log"
}
trap restore EXIT

modprobe vhci-hcd || { echo "could not load vhci-hcd" >&2; exit 1; }

T=$(mktemp -d /tmp/porthouse-live.XXXXXX)
PORTHOUSE_TRACE=1 PORTHOUSE_STATE_DIR=$T/s PORTHOUSE_IPC_DIR=$T/s/ipc "$SRV" --no-mdns --port $PORT > "$OUT/server.log" 2>&1 & SP=$!
PORTHOUSE_STATE_DIR=$T/c PORTHOUSE_IPC_DIR=$T/c/ipc "$CLID" run > "$OUT/clientd.log" 2>&1 & CP=$!
wait_service "$T/s" server && wait_service "$T/c" client || exit 1
echo "=== pairing (encrypted; PIN-authenticated) ==="
PIN=$(sv "$T/s" pair | pin_from)
[ -n "$PIN" ] || { echo "could not open a pairing window" >&2; exit 1; }
cl "$T/c" pair 127.0.0.1:$PORT --pin "$PIN" || exit 1
cl "$T/c" add 127.0.0.1:$PORT
echo
echo "=== devices the server sees ==="
cl "$T/c" list || exit 1

echo
echo "=== attaching $BUSID for ${SECS}s ==="
cl "$T/c" use 127.0.0.1:$PORT "$BUSID" --seconds "$SECS" > "$OUT/client.log" 2>&1
sleep 4
cat "$OUT/client.log"
echo
echo "=== vhci ports in use ==="
"$CLI" ports | grep -v " 004 " || echo "(none: attach did not take)"
echo
echo "=== the device as the kernel now sees it (bus 8 = vhci_hcd) ==="
lsusb -d 1532:0098 2>/dev/null || true
for d in /sys/bus/usb/devices/8-*; do
  [ -e "$d/idVendor" ] && echo "$(basename "$d"): $(cat "$d/idVendor"):$(cat "$d/idProduct") $(cat "$d/product" 2>/dev/null) driver=$(readlink "$d:1.0/driver" 2>/dev/null | sed 's|.*/||')"
done
echo
echo "=== input devices created by vhci_hcd (this is the device arriving over porthouse) ==="
grep -B3 -A6 "vhci_hcd" /proc/bus/input/devices || echo "(none)"
echo
echo ">>> MOVE THE MOUSE NOW. It should still move the pointer. (${SECS}s window) <<<"
sleep "$SECS"
echo
echo "=== recent kernel messages ==="
dmesg | tail -40 | grep -i -E "vhci|usb 8|usb 9|usbhid|input" | tail -15
echo
echo "=== server log (first 40 lines, URB trace) ==="
head -40 "$OUT/server.log"
echo "... total trace lines: $(wc -l < "$OUT/server.log")"
