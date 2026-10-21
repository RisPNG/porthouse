#!/usr/bin/env bash
# Full-chain test of the Windows attach route, runnable on Linux without root or a kernel driver.
#
#   ./scripts/e2e-windows-path.sh [BUSID]      (default 5-1; needs read/write access to the device node)
#
# On Windows the usbip-win2 driver connects to a local "USB/IP server" (our shim) and speaks URBs.
# Here a fake importer plays that driver and asks, through the real shim, the real TLS relay, the
# real server engine and a REAL USB device, for the device and configuration descriptors. They are
# compared byte for byte with what the Linux kernel itself reports for the same device.
#
# The device is taken from its host driver for a few seconds and always restored.

set -u
BUSID=${1:-5-1}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SRV=$ROOT/target/debug/netusb-server
CLI=$ROOT/target/debug/netusb-client
W=$(mktemp -d /tmp/netusb-winpath.XXXXXX)
PORT=7695
SP=""; CP=""
BUS=${BUSID%%-*}
DEVNUM=$(cat /sys/bus/usb/devices/$BUSID/devnum 2>/dev/null) || { echo "no such device $BUSID" >&2; exit 1; }
NODE=$(printf '/dev/bus/usb/%03d/%03d' "$BUS" "$DEVNUM")
PASS=0; FAIL=0
drv() { readlink "/sys/bus/usb/devices/$BUSID:1.0/driver" 2>/dev/null | sed 's|.*/||'; }
ok()  { echo "  PASS  $1"; PASS=$((PASS+1)); }
bad() { echo "  FAIL  $1"; FAIL=$((FAIL+1)); }

restore() {
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
cleanup() { [ -n "$CP" ] && kill "$CP" 2>/dev/null; [ -n "$SP" ] && kill "$SP" 2>/dev/null; sleep 0.5; restore; echo "final driver of $BUSID:1.0: $(drv)"; rm -rf "$W"; }
trap cleanup EXIT

mkdir -p "$W/s" "$W/c" "$W/out"
cat > "$W/fake-usbip" <<'EOF'
#!/usr/bin/env python3
# Stand-in for usbip-win2's usbip.exe (same command line), with the kernel driver's job done by a
# detached child after the importer itself has exited, just like the real thing.
import os, socket, struct, sys, time
OUT = os.environ["FAKE_OUT"]
a = sys.argv[1:]
if "detach" in a:
    open(os.path.join(OUT, "detach.txt"), "w").write(" ".join(a)); sys.exit(0)
assert a[0] == "--tcp-port" and a[2] == "attach" and "--once" in a and "--terse" in a, a
port = int(a[1]); token = a[a.index("-b") + 1]
s = socket.create_connection(("127.0.0.1", port))
s.sendall(b"\x01\x11\x80\x03\x00\x00\x00\x00" + token.encode().ljust(32, b"\0"))
def rd(n):
    b = b""
    while len(b) < n:
        c = s.recv(n - len(b))
        if not c: raise SystemExit("connection closed during import")
        b += c
    return b
rep = rd(320)
ver, code, status = struct.unpack(">HHI", rep[:8])
assert (ver, code, status) == (0x0111, 3, 0), (ver, code, status)
busnum, devnum = struct.unpack(">II", rep[8 + 256 + 32: 8 + 256 + 32 + 8])
devid = (busnum << 16) | devnum
open(os.path.join(OUT, "import.bin"), "wb").write(rep)
if os.fork():                      # importer exits; the "driver" keeps the socket
    print(9, flush=True); os._exit(0)
os.setsid()
dn = os.open(os.devnull, os.O_RDWR)
for fd in (0, 1, 2): os.dup2(dn, fd)
seq = 0
def urb(setup, length):
    global seq
    seq += 1
    h = struct.pack(">IIIII", 1, seq, devid, 1, 0) + struct.pack(">IIiII", 0, length, 0, 0, 0) + setup
    s.sendall(h)
    r = rd(48)
    cmd, rseq, _, _, _ = struct.unpack(">IIIII", r[:20])
    st, actual = struct.unpack(">iI", r[20:28])
    assert cmd == 3 and rseq == seq, (cmd, rseq, seq)
    return st, rd(actual) if actual else b""
st, dev = urb(bytes([0x80, 6, 0, 1, 0, 0, 18, 0]), 18)   # GET_DESCRIPTOR(device)
open(os.path.join(OUT, "dev.bin"), "wb").write(dev); open(os.path.join(OUT, "dev.status"), "w").write(str(st))
st, cfg = urb(bytes([0x80, 6, 0, 2, 0, 0, 9, 0]), 9)     # GET_DESCRIPTOR(configuration header)
open(os.path.join(OUT, "cfg.bin"), "wb").write(cfg)
st, stall = urb(bytes([0x80, 6, 0, 6, 0, 0, 10, 0]), 10)  # DEVICE_QUALIFIER: full-speed devices stall
open(os.path.join(OUT, "qual.status"), "w").write(str(st))
# HID report descriptor of interface 1 (an interface-directed request, GET_DESCRIPTOR type 0x22)
st, rep = urb(bytes([0x81, 6, 0, 0x22, 1, 0, 0x00, 0x10]), 4096)
open(os.path.join(OUT, "rep.bin"), "wb").write(rep); open(os.path.join(OUT, "rep.status"), "w").write(str(st))
open(os.path.join(OUT, "done"), "w").write("ok")
while s.recv(4096): pass          # like the driver: stay until the connection goes away
EOF
chmod +x "$W/fake-usbip"

NETUSB_STATE_DIR=$W/s "$SRV" run --no-mdns --port $PORT > "$W/server.log" 2>&1 & SP=$!
sleep 1.5
PIN=$(NETUSB_STATE_DIR=$W/s "$SRV" pair | sed -n 's/.*PIN:  \([0-9-]*\).*/\1/p')
NETUSB_STATE_DIR=$W/c "$CLI" pair 127.0.0.1:$PORT --pin "$PIN" > /dev/null || { echo "pairing failed"; exit 1; }
BEFORE=$(drv); echo "device $BUSID starts on driver '$BEFORE'"
[ -n "$BEFORE" ] || { echo "device has no host driver to restore; pick another" >&2; exit 1; }

FAKE_OUT=$W/out NETUSB_STATE_DIR=$W/c "$CLI" use 127.0.0.1:$PORT "$BUSID" --importer "$W/fake-usbip" > "$W/client.log" 2>&1 & CP=$!
for _ in $(seq 1 60); do [ -e "$W/out/done" ] && break; sleep 0.25; done

echo "== handshake and descriptors through the full chain =="
[ -e "$W/out/done" ] && ok "the fake driver completed its URB exchange" || { bad "no completion; client log: $(cat "$W/client.log")"; }
grep -q "attached $BUSID" "$W/client.log" && ok "client reports the device attached" || bad "client log: $(cat "$W/client.log")"
[ "$(drv)" = "usbfs" ] && ok "server claimed the device from its host driver" || bad "driver is '$(drv)'"
SYS=/sys/bus/usb/devices/$BUSID/descriptors
cmp -s <(head -c 18 "$SYS") "$W/out/dev.bin" && ok "device descriptor is byte-identical to the kernel's (18 bytes)" || bad "device descriptor differs: $(xxd -p "$W/out/dev.bin" 2>/dev/null)"
cmp -s <(dd if="$SYS" bs=1 skip=18 count=9 2>/dev/null) "$W/out/cfg.bin" && ok "configuration descriptor header is byte-identical (9 bytes)" || bad "config descriptor differs"
[ "$(cat "$W/out/dev.status" 2>/dev/null)" = "0" ] && ok "successful URBs report status 0" || bad "dev status $(cat "$W/out/dev.status" 2>/dev/null)"
[ "$(cat "$W/out/qual.status" 2>/dev/null)" = "-32" ] && ok "a STALLed request reports -EPIPE (-32)" || bad "stall status $(cat "$W/out/qual.status" 2>/dev/null)"
if [ "$(cat "$W/out/rep.status" 2>/dev/null)" = "0" ] && [ -s "$W/out/rep.bin" ]; then
  ok "an interface-directed request (HID report descriptor, interface 1) succeeded: $(wc -c < "$W/out/rep.bin") bytes"
  KRN=$(ls /sys/bus/usb/devices/$BUSID:1.1/*/report_descriptor 2>/dev/null | head -1)
  # The kernel's copy is only readable while the host driver is bound, so compare when available.
  [ -n "$KRN" ] && { cmp -s "$KRN" "$W/out/rep.bin" && ok "report descriptor matches the kernel's" || bad "report descriptor differs"; }
else
  bad "interface-directed request failed (status $(cat "$W/out/rep.status" 2>/dev/null))"
fi
python3 - "$W/out/import.bin" "$BUSID" <<'EOF' && ok "import reply described the right device" || bad "import reply wrong"
import struct, sys
b = open(sys.argv[1], "rb").read()
assert len(b) == 320
busid = b[8 + 256: 8 + 256 + 32].split(b"\0")[0].decode()
vid, pid = struct.unpack(">HH", b[8 + 256 + 32 + 12: 8 + 256 + 32 + 16])
want = sys.argv[2]
sysvid = int(open(f"/sys/bus/usb/devices/{want}/idVendor").read(), 16)
syspid = int(open(f"/sys/bus/usb/devices/{want}/idProduct").read(), 16)
assert busid == want and (vid, pid) == (sysvid, syspid), (busid, vid, pid)
EOF

echo "== release =="
kill -TERM $CP; for _ in $(seq 1 20); do kill -0 $CP 2>/dev/null || break; sleep 0.25; done
kill -0 $CP 2>/dev/null && bad "client did not exit on SIGTERM" || ok "client exited on SIGTERM"
CP=""
[ "$(cat "$W/out/detach.txt" 2>/dev/null)" = "detach -p 9" ] && ok "client asked the importer to detach its port" || bad "detach call: '$(cat "$W/out/detach.txt" 2>/dev/null)'"
sleep 1.5
[ "$(drv)" = "$BEFORE" ] && ok "device returned to its host driver" || bad "driver is '$(drv)'"

echo
echo "result: $PASS passed, $FAIL failed"
[ $FAIL -eq 0 ]
