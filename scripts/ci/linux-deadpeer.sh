#!/usr/bin/env bash
# A client that vanishes without closing its connection (machine asleep, crashed, network gone) must
# not keep a shared device forever. Black-holes the network after a client attaches and checks that
# the server notices (TCP keepalive) and releases the device. Needs root and iptables.
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SRV=${NETUSB_BIN_DIR:-$ROOT/target/debug}/netusb-server
CLI=${NETUSB_BIN_DIR:-$ROOT/target/debug}/netusb-client
W=$(mktemp -d /tmp/netusb-deadpeer.XXXXXX)
PORT=7730
PASS=0; FAIL=0; PIDS=()
ok()  { echo "  PASS  $1"; PASS=$((PASS+1)); }
bad() { echo "  FAIL  $1"; FAIL=$((FAIL+1)); }
cleanup() {
  iptables -D INPUT -p tcp --dport $PORT -j DROP 2>/dev/null; iptables -D INPUT -p tcp --sport $PORT -j DROP 2>/dev/null
  for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
  echo "--- server log"; cat "$W/server.log" 2>/dev/null; rm -rf "$W"
}
trap cleanup EXIT
[ "$(id -u)" -eq 0 ] || { echo "run as root" >&2; exit 1; }
command -v iptables >/dev/null || { echo "iptables is needed" >&2; exit 3; }
mkdir -p "$W/s" "$W/c"

NETUSB_STATE_DIR=$W/s "$SRV" run --no-mdns --port $PORT --test-device mouse > "$W/server.log" 2>&1 & PIDS+=($!)
sleep 1.5
PIN=$(NETUSB_STATE_DIR=$W/s "$SRV" pair | sed -n 's/.*PIN:  \([0-9-]*\).*/\1/p')
NETUSB_STATE_DIR=$W/c "$CLI" pair 127.0.0.1:$PORT --pin "$PIN" > /dev/null && ok "paired" || bad "pairing"

# A scripted client: attach the virtual mouse, then go completely silent but keep the socket open.
cat > "$W/hold.py" <<'PY'
import socket, ssl, struct, sys, time
port, pem = int(sys.argv[1]), sys.argv[2]
def frame(b): return struct.pack('<IB', len(b), 0) + b
def pstr(s): b = s.encode(); return bytes([len(b)]) + b
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT); ctx.minimum_version = ssl.TLSVersion.TLSv1_3
ctx.check_hostname = False; ctx.verify_mode = ssl.CERT_NONE; ctx.load_cert_chain(pem)
s = ctx.wrap_socket(socket.create_connection(("127.0.0.1", port)), server_hostname="netusb")
s.sendall(frame(bytes([0, 4]) + pstr("deadpeer"))); time.sleep(0.3); s.recv(65536)
s.sendall(frame(bytes([3]) + pstr("99-1"))); time.sleep(0.5); s.recv(65536)
print("attached", flush=True)
time.sleep(300)
PY
python3 "$W/hold.py" $PORT "$W/c/client.pem" > "$W/hold.out" 2>&1 & PIDS+=($!)
for _ in $(seq 1 20); do grep -q attached "$W/hold.out" && break; sleep 0.5; done
grep -q 'attached 99-1' "$W/server.log" && ok "the client attached the virtual mouse" || bad "attach did not happen"

echo "== the client's network disappears (packets dropped, no FIN/RST)"
T0=$(date +%s)
iptables -I INPUT -p tcp --dport $PORT -j DROP
iptables -I INPUT -p tcp --sport $PORT -j DROP
for _ in $(seq 1 120); do grep -q 'released 99-1' "$W/server.log" && break; sleep 1; done
T1=$(date +%s); ELAPSED=$((T1 - T0))
if grep -q 'released 99-1' "$W/server.log"; then
  ok "the server noticed the dead client and released the device after ${ELAPSED}s"
  [ "$ELAPSED" -le 60 ] && ok "within a minute (keepalive: 10s idle + 3 probes 5s apart)" || bad "took ${ELAPSED}s, too slow"
else
  bad "the device was still held by the vanished client after ${ELAPSED}s"
fi

iptables -D INPUT -p tcp --dport $PORT -j DROP; iptables -D INPUT -p tcp --sport $PORT -j DROP
NETUSB_STATE_DIR=$W/c "$CLI" list --host 127.0.0.1:$PORT 2>&1 | grep '99-1' | grep -q 'in use by' && bad "the device still shows as in use" || ok "the device is free to be used again"
echo; echo "result: $PASS passed, $FAIL failed"; [ $FAIL -eq 0 ]
