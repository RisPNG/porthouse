#!/usr/bin/env bash
# End-to-end security tests: tries to break pairing and authorization. No root, no USB claims.
#
#   ./scripts/e2e-security.sh
#
# Everything runs on loopback with throw-away state directories.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
. "$ROOT/scripts/lib.sh"
W=$(mktemp -d /tmp/netusb-e2e.XXXXXX)
PORT=7650; RELAY=7651
PASS=0; FAIL=0
SPID=""; RPID=""; D1=""; D2=""
cleanup() { for p in "$SPID" "$RPID" "$D1" "$D2"; do [ -n "$p" ] && kill "$p" 2>/dev/null; done; rm -rf "$W"; }
trap cleanup EXIT

ok()   { echo "  PASS  $1"; PASS=$((PASS+1)); }
bad()  { echo "  FAIL  $1"; FAIL=$((FAIL+1)); }
# expect_fail NAME PATTERN CMD...   -> command must fail and its output must match PATTERN
expect_fail() { local n=$1 p=$2; shift 2; local o; o=$("$@" 2>&1); local rc=$?
  if [ $rc -ne 0 ] && echo "$o" | grep -q -i -E "$p"; then ok "$n"; else bad "$n (rc=$rc): $o"; fi; }
expect_ok()   { local n=$1 p=$2; shift 2; local o; o=$("$@" 2>&1); local rc=$?
  if [ $rc -eq 0 ] && echo "$o" | grep -q -i -E "$p"; then ok "$n"; else bad "$n (rc=$rc): $o"; fi; }

S=$W/server; C1=$W/client1; C2=$W/client2
mkdir -p "$S" "$C1" "$C2"
srv()  { sv "$S" "$@"; }
c1()   { cl "$C1" "$@"; }
c2()   { cl "$C2" "$@"; }
pin_of() { cat "$S/pairing.pin" 2>/dev/null | head -1; }

NETUSB_STATE_DIR=$S NETUSB_IPC_DIR=$S/ipc "$SRV" run --no-mdns --port $PORT > "$W/server.log" 2>&1 & SPID=$!
NETUSB_STATE_DIR=$C1 NETUSB_IPC_DIR=$C1/ipc "$CLID" run > "$W/c1d.log" 2>&1 & D1=$!
NETUSB_STATE_DIR=$C2 NETUSB_IPC_DIR=$C2/ipc "$CLID" run > "$W/c2d.log" 2>&1 & D2=$!
wait_service "$S" server && wait_service "$C1" client && wait_service "$C2" client || exit 1
c1 add 127.0.0.1:$PORT; c2 add 127.0.0.1:$PORT
echo "== server started; first run opens a pairing window =="
PIN=$(pin_of); [ -n "$PIN" ] && ok "pairing window opened automatically" || bad "no pairing window"

echo "== unauthenticated access =="
expect_ok "an unpaired client sees the server but no devices" "not paired" c1 list
printf 'GET / HTTP/1.0\r\n\r\n' | timeout 3 nc 127.0.0.1 $PORT > "$W/plain.out" 2>&1
# rustls answers non-TLS bytes with a 7-byte TLS alert record (type 0x15) and closes; anything
# else, or anything longer, would mean plaintext application data leaked.
SZ=$(wc -c < "$W/plain.out"); FIRST=$(head -c1 "$W/plain.out" | xxd -p)
if [ "$SZ" -eq 0 ] || { [ "$SZ" -le 16 ] && [ "$FIRST" = "15" ]; }; then ok "plaintext HTTP gets no data (at most a TLS alert)"; else bad "plaintext request got $SZ bytes, first byte $FIRST"; fi
head -c 2000 /dev/urandom | timeout 3 nc 127.0.0.1 $PORT > /dev/null 2>&1
kill -0 $SPID 2>/dev/null && ok "server survives random garbage" || bad "server died on garbage"

echo "== pairing =="
expect_fail "wrong PIN is rejected" "wrong PIN" c1 pair 127.0.0.1:$PORT --pin 00000000
expect_fail "malformed PIN is rejected" "8 digits" c1 pair 127.0.0.1:$PORT --pin abc
expect_ok   "correct PIN pairs" "paired with" c1 pair 127.0.0.1:$PORT --pin "$PIN"
expect_ok   "paired client can list" "127.0.0.1:$PORT\\)  paired" c1 list
[ -z "$(pin_of)" ] && ok "PIN is single-use" || bad "PIN survived a successful pairing"
expect_fail "a second client cannot reuse the PIN" "pairing is not open" c2 pair 127.0.0.1:$PORT --pin "$PIN"

echo "== lockout after repeated wrong PINs =="
srv pair > /dev/null; GOOD=$(pin_of)
for i in 1 2 3 4 5; do c2 pair 127.0.0.1:$PORT --pin 11111111 > /dev/null 2>&1; done
expect_fail "window closes after 5 wrong guesses (even the right PIN fails)" "pairing is not open" c2 pair 127.0.0.1:$PORT --pin "$GOOD"

echo "== man in the middle =="
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout "$W/mitm.key" -out "$W/mitm.crt" -days 1 -subj /CN=mitm 2>/dev/null
cat > "$W/mitm.py" <<EOF
import socket, ssl, threading, sys
def pump(a, b):
    try:
        while True:
            d = a.recv(65536)
            if not d: break
            b.sendall(d)
    except Exception: pass
    finally:
        for s in (a, b):
            try: s.shutdown(socket.SHUT_RDWR)
            except Exception: pass
sctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); sctx.minimum_version = ssl.TLSVersion.TLSv1_3
sctx.load_cert_chain("$W/mitm.crt", "$W/mitm.key")
cctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT); cctx.minimum_version = ssl.TLSVersion.TLSv1_3
cctx.check_hostname = False; cctx.verify_mode = ssl.CERT_NONE; cctx.load_cert_chain("$W/mitm.crt", "$W/mitm.key")
ls = socket.socket(); ls.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); ls.bind(("127.0.0.1", $RELAY)); ls.listen(5)
while True:
    c, _ = ls.accept()
    try:
        cs = sctx.wrap_socket(c, server_side=True)
        up = cctx.wrap_socket(socket.create_connection(("127.0.0.1", $PORT)), server_hostname="netusb")
    except Exception as e:
        continue
    threading.Thread(target=pump, args=(cs, up), daemon=True).start()
    threading.Thread(target=pump, args=(up, cs), daemon=True).start()
EOF
python3 "$W/mitm.py" & RPID=$!
sleep 1
srv pair > /dev/null; MPIN=$(pin_of)
expect_fail "pairing through a TLS-terminating relay fails even with the right PIN" "wrong PIN|did not prove|failed" c2 pair 127.0.0.1:$RELAY --pin "$MPIN"
[ -z "$(c2 servers)" ] || c2 servers | grep -q "no paired" && ok "client did not trust the relay" || bad "client stored the relay as trusted"
kill $RPID 2>/dev/null; RPID=""
srv pair > /dev/null; DPIN=$(pin_of)
expect_ok "direct pairing still works afterwards" "paired with" c2 pair 127.0.0.1:$PORT --pin "$DPIN"

echo "== revocation =="
FP=$(srv clients | grep -v "no paired" | tail -1 | cut -d' ' -f1)
expect_ok "server lists paired clients" "[0-9a-f]{64}" srv clients
srv revoke "$FP" > /dev/null
expect_ok   "revoked client is no longer paired" "127.0.0.1:$PORT\\)  not paired" c2 list

echo "== client pins the server =="
# Replace the server's identity: the paired client must refuse the new certificate.
kill $SPID; wait $SPID 2>/dev/null; rm -f "$S/server.pem"
NETUSB_STATE_DIR=$S NETUSB_IPC_DIR=$S/ipc "$SRV" run --no-mdns --port $PORT > "$W/server2.log" 2>&1 & SPID=$!
wait_service "$S" server
expect_fail "client refuses a server whose certificate changed" "not a paired server" c1 use 127.0.0.1:$PORT 99-1

echo
echo "result: $PASS passed, $FAIL failed"
[ $FAIL -eq 0 ]
