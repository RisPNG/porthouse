#!/usr/bin/env bash
# Real attach on Linux with real vhci_hcd: attaches netusb's virtual mouse, then proves that the
# kernel created a working input device and that movement reports arrive through the whole chain.
# Needs root and the vhci-hcd kernel module. Run from the repository root after `cargo build`.
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SRV=$ROOT/target/debug/netusb-server
CLI=$ROOT/target/debug/netusb-client
BIN_DIR=${BIN_DIR:-}
[ -n "$BIN_DIR" ] && { SRV=$BIN_DIR/netusb-server; CLI=$BIN_DIR/netusb-client; }
W=$(mktemp -d /tmp/netusb-linux-attach.XXXXXX)
PORT=7600
PASS=0; FAIL=0; SP=""; CP=""
ok()  { echo "  PASS  $1"; PASS=$((PASS+1)); }
bad() { echo "  FAIL  $1"; FAIL=$((FAIL+1)); }
cleanup() { [ -n "$CP" ] && kill "$CP" 2>/dev/null; [ -n "$SP" ] && kill "$SP" 2>/dev/null; sleep 0.5; echo "--- server log"; cat "$W/server.log" 2>/dev/null; echo "--- client output"; cat "$W/use.out" "$W/use.err" 2>/dev/null; }
trap cleanup EXIT
[ "$(id -u)" -eq 0 ] || { echo "run as root" >&2; exit 1; }

echo "== vhci_hcd"
modprobe vhci-hcd 2>&1 || true
if [ ! -d /sys/devices/platform/vhci_hcd.0 ]; then
  echo "vhci_hcd is not available on this kernel ($(uname -r)); cannot test attach here"; ls /lib/modules/$(uname -r)/kernel/drivers/usb/usbip 2>&1 | head; exit 3
fi
ok "vhci_hcd is loaded"

echo "== server, pairing"
mkdir -p "$W/s" "$W/c"
NETUSB_STATE_DIR=$W/s "$SRV" run --no-mdns --port $PORT --test-device mouse > "$W/server.log" 2>&1 & SP=$!
sleep 1.5
PIN=$(NETUSB_STATE_DIR=$W/s "$SRV" pair | sed -n 's/.*PIN:  \([0-9-]*\).*/\1/p')
NETUSB_STATE_DIR=$W/c "$CLI" pair 127.0.0.1:$PORT --pin "$PIN" > /dev/null && ok "paired" || bad "pairing"
NETUSB_STATE_DIR=$W/c "$CLI" list --host 127.0.0.1:$PORT | grep -q '99-1' && ok "virtual mouse is listed" || bad "virtual mouse not listed"

echo "== attach for 40 seconds"
NETUSB_STATE_DIR=$W/c "$CLI" use 127.0.0.1:$PORT 99-1 --seconds 40 > "$W/use.out" 2> "$W/use.err" & CP=$!
for _ in $(seq 1 30); do grep -q attached "$W/use.out" && break; sleep 1; done
grep -q attached "$W/use.out" && ok "client reports the device attached" || bad "client never attached"
sleep 4

echo "== diagnostics"
echo "--- lsusb -t"; lsusb -t 2>&1 | head -20
echo "--- interfaces and their drivers"
for d in /sys/bus/usb/devices/*; do
  [ -e "$d/idVendor" ] && [ "$(cat "$d/idVendor")" = 1209 ] && echo "$d: $(cat "$d/product" 2>/dev/null)"
  case "$d" in *:1.*) [ "$(cat "$(dirname "$d")/$(basename "$d" | cut -d: -f1)/idVendor" 2>/dev/null)" = 1209 ] && echo "  $(basename "$d") driver=$(basename "$(readlink "$d/driver" 2>/dev/null)" 2>/dev/null)";; esac
done
echo "--- modules"; lsmod | grep -E 'hid|vhci|usbip|evdev|input' || true
modprobe usbhid 2>&1 | head -2; modprobe hid-generic 2>&1 | head -2
echo "--- dmesg (usb/hid/vhci)"; dmesg 2>/dev/null | grep -i -E 'vhci|usb 1-|hid|input' | tail -25 || true
sleep 2

echo "== what the kernel made of it"
grep -B2 -A8 "netusb virtual mouse" /proc/bus/input/devices || true
EV=$(awk '/netusb virtual mouse/{f=1} f&&/Handlers=/{print; exit}' /proc/bus/input/devices | grep -o 'event[0-9]*' | head -1)
[ -n "$EV" ] && ok "an input device exists for it ($EV)" || bad "no input device was created"
lsusb 2>/dev/null | grep -i 1209:0001 && ok "device is on the virtual USB bus" || bad "device not visible to lsusb"

echo "== movement reports through the whole chain"
if [ -n "$EV" ]; then
  python3 - "/dev/input/$EV" <<'PY' && ok "pointer movement events arrived (REL_X/REL_Y, +3 steps, square pattern)" || bad "no movement events"
import os, struct, sys, time, select
fd = os.open(sys.argv[1], os.O_RDONLY | os.O_NONBLOCK)
deadline = time.time() + 3
xs, ys, n = [], [], 0
while time.time() < deadline:
    r, _, _ = select.select([fd], [], [], 0.2)
    if not r: continue
    try: data = os.read(fd, 24 * 64)
    except BlockingIOError: continue
    for off in range(0, len(data) - 23, 24):
        _s, _us, typ, code, val = struct.unpack("llHHi", data[off:off + 24])
        if typ == 2 and code == 0: xs.append(val); n += 1
        if typ == 2 and code == 1: ys.append(val)
print(f"  {len(xs)} X events, {len(ys)} Y events; distinct X values {sorted(set(xs))}, Y values {sorted(set(ys))}")
sys.exit(0 if (xs or ys) and set(xs + ys) <= {-3, 0, 3} else 1)
PY
fi

echo "== raw interrupt reports read from userspace (independent of any HID driver)"
if python3 -c 'import usb.core' 2>/dev/null; then
  python3 - <<'PY' && ok "60 interrupt reports arrived through the whole chain and form the expected square" || bad "raw interrupt read failed"
import sys, usb.core
dev = usb.core.find(idVendor=0x1209, idProduct=0x0001)
if dev is None:
    print("  device not found by libusb"); sys.exit(1)
try:
    if dev.is_kernel_driver_active(0): dev.detach_kernel_driver(0)
except Exception as e: print("  detach:", e)
try: dev.set_configuration()
except Exception as e: print("  set_configuration:", e)
reports = [tuple(dev.read(0x81, 4, timeout=3000)) for _ in range(60)]
sgn = lambda b: b - 256 if b > 127 else b
moves = {(sgn(r[1]), sgn(r[2])) for r in reports}
print(f"  {len(reports)} reports, distinct movements: {sorted(moves)}")
sys.exit(0 if len(reports) == 60 and moves <= {(3, 0), (0, 3), (-3, 0), (0, -3)} and len(moves) >= 2 else 1)
PY
else
  echo "  (pyusb not installed; skipped)"
fi

echo "== release"
wait $CP 2>/dev/null; CP=""
sleep 2
lsusb 2>/dev/null | grep -qi 1209:0001 && bad "device still present after release" || ok "virtual device is gone after release"
grep -q "released 99-1" "$W/server.log" && ok "server saw the release" || bad "server did not log the release"

echo; echo "result: $PASS passed, $FAIL failed"; [ $FAIL -eq 0 ]
