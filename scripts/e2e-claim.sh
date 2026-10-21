#!/usr/bin/env bash
# Regression test for the server's claim/release behaviour on a REAL local USB device.
#
#   ./scripts/e2e-claim.sh [BUSID]        (default 5-1; needs read/write access to the device node)
#
# It claims the device from the host driver for a few seconds at a time and checks that it always
# comes back: when the client leaves, on SIGTERM, and after a SIGKILL followed by a restart.
# No root needed if your user can open /dev/bus/usb/... (plugdev, or a udev rule).
# Choose a device you can live without for ~30 seconds. A safety restore runs at exit regardless.

set -u
BUSID=${1:-5-1}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
. "$ROOT/scripts/lib.sh"
W=$(mktemp -d /tmp/netusb-claim.XXXXXX)
PORT=7690
SP=""; HP=""; DP=""
BUS=${BUSID%%-*}
DEVNUM=$(cat /sys/bus/usb/devices/$BUSID/devnum 2>/dev/null) || { echo "no such device $BUSID" >&2; exit 1; }
NODE=$(printf '/dev/bus/usb/%03d/%03d' "$BUS" "$DEVNUM")
PASS=0; FAIL=0

drv() { readlink "/sys/bus/usb/devices/$BUSID:1.0/driver" 2>/dev/null | sed 's|.*/||'; }
ok()  { echo "  PASS  $1"; PASS=$((PASS+1)); }
bad() { echo "  FAIL  $1"; FAIL=$((FAIL+1)); }
expect() { [ "$2" = "$3" ] && ok "$1" || bad "$1 (got '$2', wanted '$3')"; }

restore() {
  # Give every interface back to its host driver, whatever state the test ended in.
  python3 - "$NODE" <<'EOF'
import os, sys, fcntl, struct
try:
    fd = os.open(sys.argv[1], os.O_RDWR)
    for ifno in range(8):
        try: fcntl.ioctl(fd, 0xC0105512, bytearray(struct.pack("iiQ", ifno, 0x5517, 0)))
        except OSError: pass
    os.close(fd)
except OSError: pass
EOF
}
cleanup() { [ -n "$DP" ] && kill "$DP" 2>/dev/null; [ -n "$HP" ] && kill "$HP" 2>/dev/null; [ -n "$SP" ] && kill "$SP" 2>/dev/null; sleep 0.5; restore; echo "final driver of $BUSID:1.0: $(drv)"; rm -rf "$W"; }
trap cleanup EXIT

# A scripted client that reuses the paired client certificate to attach and then sits there.
cat > "$W/hold.py" <<'EOF'
import socket, ssl, struct, sys, time
port, busid, hold, pem = int(sys.argv[1]), sys.argv[2], float(sys.argv[3]), sys.argv[4]
def frame(b): return struct.pack('<IB', len(b), 0) + b
def pstr(s): b = s.encode(); return bytes([len(b)]) + b
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT); ctx.minimum_version = ssl.TLSVersion.TLSv1_3
ctx.check_hostname = False; ctx.verify_mode = ssl.CERT_NONE; ctx.load_cert_chain(pem)
s = ctx.wrap_socket(socket.create_connection(("127.0.0.1", port)), server_hostname="netusb")
s.sendall(frame(bytes([0, 4]) + pstr("holdtest"))); time.sleep(0.3); s.recv(65536)
s.sendall(frame(bytes([3]) + pstr(busid))); time.sleep(0.8); s.recv(65536)
time.sleep(hold)
EOF

mkdir -p "$W/s" "$W/c"
# NOTE: launch the binaries directly (not through a shell function) so $! is the real PID.
start_server() { NETUSB_STATE_DIR=$W/s NETUSB_IPC_DIR=$W/s/ipc "$SRV" run --no-mdns --port $PORT >> "$W/server.log" 2>&1 & SP=$!; wait_service "$W/s" server; }

start_server
NETUSB_STATE_DIR=$W/c NETUSB_IPC_DIR=$W/c/ipc "$CLID" run > "$W/clientd.log" 2>&1 & DP=$!
wait_service "$W/c" client
PIN=$(sv "$W/s" pair | pin_from)
cl "$W/c" pair 127.0.0.1:$PORT --pin "$PIN" > /dev/null || { echo "pairing failed"; exit 1; }
BEFORE=$(drv); echo "device $BUSID ($NODE) starts on driver '$BEFORE'"
[ -n "$BEFORE" ] || { echo "device has no host driver to restore; pick another" >&2; exit 1; }

echo "== A: client leaves"
python3 "$W/hold.py" $PORT "$BUSID" 2 "$W/c/client.pem" & HP=$!; sleep 2
expect "claimed while attached" "$(drv)" "usbfs"
wait $HP; HP=""; sleep 1.5
expect "returned to host driver after client left" "$(drv)" "$BEFORE"

echo "== B: SIGTERM mid-session"
python3 "$W/hold.py" $PORT "$BUSID" 8 "$W/c/client.pem" & HP=$!; sleep 2
expect "claimed while attached" "$(drv)" "usbfs"
kill -TERM $SP; sleep 2.5
expect "returned to host driver after SIGTERM" "$(drv)" "$BEFORE"
kill -0 $SP 2>/dev/null && bad "server still running after SIGTERM" || ok "server exited after SIGTERM"
wait $HP 2>/dev/null; HP=""

echo "== C: SIGKILL mid-session, then restart"
start_server
python3 "$W/hold.py" $PORT "$BUSID" 8 "$W/c/client.pem" & HP=$!; sleep 2
expect "claimed while attached" "$(drv)" "usbfs"
kill -KILL $SP; sleep 0.7
[ -s "$W/s/claimed-devices" ] && ok "crash left a recovery journal" || bad "no journal after SIGKILL"
start_server
expect "restart restored the device" "$(drv)" "$BEFORE"
grep -q "restored $BUSID" "$W/server.log" && ok "restart logged the recovery" || bad "no recovery message"
[ ! -e "$W/s/claimed-devices" ] && ok "journal cleared after recovery" || bad "journal left behind"
wait $HP 2>/dev/null; HP=""

echo
echo "result: $PASS passed, $FAIL failed"
[ $FAIL -eq 0 ]
