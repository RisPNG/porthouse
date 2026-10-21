#!/usr/bin/env bash
# End-to-end tests for the client daemon's web interface. No root, no USB claims.
#
#   ./scripts/e2e-ui.sh

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SRV=$ROOT/target/debug/netusb-server
CLI=$ROOT/target/debug/netusb-client
W=$(mktemp -d /tmp/netusb-ui.XXXXXX)
SPORT=7660; UPORT=7661
BASE=http://127.0.0.1:$UPORT
PASS=0; FAIL=0; SPID=""; DPID=""
cleanup() { [ -n "$SPID" ] && kill "$SPID" 2>/dev/null; [ -n "$DPID" ] && kill "$DPID" 2>/dev/null; rm -rf "$W"; }
trap cleanup EXIT
ok()  { echo "  PASS  $1"; PASS=$((PASS+1)); }
bad() { echo "  FAIL  $1"; FAIL=$((FAIL+1)); }
# status of a curl request
code() { curl -s -o /dev/null -w '%{http_code}' --max-time 5 "$@"; }
check() { local n=$1 want=$2; shift 2; local got; got=$(code "$@"); [ "$got" = "$want" ] && ok "$n" || bad "$n (wanted $want, got $got)"; }

mkdir -p "$W/s" "$W/c"
HOSTILE='<img src=x onerror=alert(1)>'
NETUSB_STATE_DIR=$W/s "$SRV" run --no-mdns --port $SPORT --test-device mouse --name "$HOSTILE" > "$W/server.log" 2>&1 & SPID=$!
sleep 1.5
NETUSB_STATE_DIR=$W/s "$SRV" pair > "$W/pair.out"
PIN=$(sed -n 's/.*PIN:  \([0-9-]*\).*/\1/p' "$W/pair.out")

NETUSB_STATE_DIR=$W/c "$CLI" gui --port $UPORT > "$W/daemon.out" 2>&1 & DPID=$!
sleep 1.5
TOKEN=$(sed -n 's/.*token=\([0-9a-f]*\).*/\1/p' "$W/daemon.out" | head -1)
[ -n "$TOKEN" ] && ok "daemon prints a login URL" || { bad "no token printed: $(cat "$W/daemon.out")"; exit 1; }
[ "$(stat -c %a "$W/c/ui.url")" = "600" ] && ok "login URL file is private (0600)" || bad "ui.url permissions"

echo "== authentication =="
check "no cookie -> 401" 401 $BASE/api/state
check "wrong token -> 401" 401 "$BASE/?token=deadbeef" -b "x=y"
check "forged Host header -> 403 (DNS rebinding)" 403 -H "Host: evil.example:$UPORT" "$BASE/?token=$TOKEN"
check "Host without port -> 403" 403 -H "Host: 127.0.0.1" "$BASE/?token=$TOKEN"
curl -s -D "$W/h" -o /dev/null --max-time 5 "$BASE/?token=$TOKEN"
grep -qi '^HTTP.* 303' "$W/h" && grep -qi 'set-cookie: netusb=.*HttpOnly.*SameSite=Strict' "$W/h" && ok "token exchange sets an HttpOnly SameSite=Strict cookie" || bad "cookie headers: $(cat "$W/h")"
COOKIE="netusb=$TOKEN"
check "cookie grants access to the page" 200 -b "$COOKIE" $BASE/
check "cookie grants access to the API" 200 -b "$COOKIE" $BASE/api/state
curl -s -D "$W/h2" -o /dev/null -b "$COOKIE" $BASE/
grep -qi "content-security-policy: default-src 'none'" "$W/h2" && ok "page is served with a strict CSP" || bad "no CSP"
grep -qi 'x-frame-options: DENY' "$W/h2" && ok "page cannot be framed" || bad "framing allowed"

echo "== write protection =="
J='-H Content-Type:application/json'
check "POST without JSON content type -> 403" 403 -b "$COOKIE" -X POST -d '{"addr":"x"}' $BASE/api/add_server
check "POST with form content type -> 403" 403 -b "$COOKIE" -X POST -H 'Content-Type: application/x-www-form-urlencoded' -d 'addr=x' $BASE/api/add_server
check "cross-site Origin -> 403" 403 -b "$COOKIE" -X POST $J -H 'Origin: https://evil.example' -d '{"addr":"x"}' $BASE/api/add_server
check "POST without cookie -> 401" 401 -X POST $J -d '{"addr":"x"}' $BASE/api/add_server
check "valid write is accepted" 200 -b "$COOKIE" -X POST $J -H "Origin: $BASE" -d "{\"addr\":\"127.0.0.1:$SPORT\"}" $BASE/api/add_server
check "address with control characters -> 400" 400 -b "$COOKIE" -X POST $J -d '{"addr":"a\nb"}' $BASE/api/add_server

echo "== function =="
sleep 4
STATE=$(curl -s -b "$COOKIE" $BASE/api/state)
echo "$STATE" | python3 -c 'import sys,json; s=json.load(sys.stdin); sv=s["servers"][0]; assert sv["paired"] is False; print("   server:",repr(sv["name"]))' \
  && ok "server appears as not paired" || bad "state: $STATE"
echo "$STATE" | python3 -c 'import sys,json; s=json.load(sys.stdin); assert s["servers"][0]["name"]=="'"$HOSTILE"'"' \
  && ok "hostile server name is returned verbatim as data (UI renders it as text)" || bad "name mangled"
grep -q 'innerHTML' "$ROOT/crates/client/src/ui.html" && bad "UI uses innerHTML" || ok "UI never uses innerHTML"
check "pairing with a wrong PIN -> 502" 502 -b "$COOKIE" -X POST $J -d "{\"addr\":\"127.0.0.1:$SPORT\",\"pin\":\"00000000\"}" $BASE/api/pair
check "pairing with the right PIN -> 200" 200 -b "$COOKIE" -X POST $J -d "{\"addr\":\"127.0.0.1:$SPORT\",\"pin\":\"$PIN\"}" $BASE/api/pair
sleep 4
curl -s -b "$COOKIE" $BASE/api/state | python3 -c 'import sys,json; s=json.load(sys.stdin); sv=s["servers"][0]; assert sv["paired"] and len(sv["devices"])>0; print("   devices:",len(sv["devices"]))' \
  && ok "after pairing the server lists its devices" || bad "devices not listed"
OUT=$(curl -s -b "$COOKIE" -X POST $J -d "{\"addr\":\"127.0.0.1:$SPORT\",\"busid\":\"99-9\"}" $BASE/api/use)
echo "$OUT" | grep -q 'no such device' && ok "attaching a missing device fails cleanly" || bad "use response: $OUT"
kill -0 $DPID 2>/dev/null && ok "daemon is still running" || bad "daemon died"

echo
echo "result: $PASS passed, $FAIL failed"
[ $FAIL -eq 0 ]
