#!/usr/bin/env bash
# Tests the Linux server engine (usbfs, bulk and control transfers, cancellation) against a real
# kernel USB device that needs no hardware:
#
#   server A (virtual loopback device) --TLS--> client --vhci_hcd--> kernel sees USB device 1209:0002
#   server B (the real engine, nusb/usbfs)  shares that kernel device
#   fake driver --TLS--> server B: control, bulk echo, cancel, ordering
#
# Needs root and vhci-hcd. Run from the repository root after `cargo build`.
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
. "$ROOT/scripts/lib.sh"
W=$(mktemp -d /tmp/netusb-linux-chain.XXXXXX)
PASS=0; FAIL=0; PIDS=()
ok()  { echo "  PASS  $1"; PASS=$((PASS+1)); }
bad() { echo "  FAIL  $1"; FAIL=$((FAIL+1)); }
cleanup() {
  for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; sleep 1
  echo "--- server A log"; cat "$W/a.log" 2>/dev/null; echo "--- server B log"; cat "$W/b.log" 2>/dev/null
  echo "--- client service 1"; cat "$W/c1d.log" 2>/dev/null; echo "--- client 1 (virtual device attach)"; cat "$W/c1.out" "$W/c1.err" 2>/dev/null
  echo "--- client service 2"; cat "$W/c2d.log" 2>/dev/null; echo "--- client 2 (fake driver)"; cat "$W/c2.out" "$W/c2.err" 2>/dev/null
}
trap cleanup EXIT
[ "$(id -u)" -eq 0 ] || { echo "run as root" >&2; exit 1; }
modprobe vhci-hcd 2>&1 || true
[ -d /sys/devices/platform/vhci_hcd.0 ] || { echo "vhci_hcd unavailable on $(uname -r)"; exit 3; }
mkdir -p "$W"/{a,b,c1,c2,out}

echo "== server A shares the virtual loopback device; client 1 plugs it into this kernel"
NETUSB_STATE_DIR=$W/a NETUSB_IPC_DIR=$W/a/ipc "$SRV" run --no-mdns --port 7600 --test-device loopback > "$W/a.log" 2>&1 & PIDS+=($!)
NETUSB_STATE_DIR=$W/c1 NETUSB_IPC_DIR=$W/c1/ipc "$CLID" run > "$W/c1d.log" 2>&1 & PIDS+=($!)
wait_service "$W/a" server && wait_service "$W/c1" client || exit 1
PIN=$(sv "$W/a" pair | pin_from)
cl "$W/c1" pair 127.0.0.1:7600 --pin "$PIN" > /dev/null && ok "client 1 paired with A" || bad "pairing with A"
cl "$W/c1" use 127.0.0.1:7600 99-2 --seconds 120 > "$W/c1.out" 2> "$W/c1.err"
for _ in $(seq 1 40); do lsusb 2>/dev/null | grep -qi 1209:0002 && break; sleep 0.5; done
lsusb 2>/dev/null | grep -i 1209:0002 && ok "the kernel enumerated the virtual device (1209:0002)" || bad "device never appeared"
sleep 1

echo "== server B (real engine) shares that kernel device"
NETUSB_STATE_DIR=$W/b NETUSB_IPC_DIR=$W/b/ipc "$SRV" run --no-mdns --port 7601 > "$W/b.log" 2>&1 & PIDS+=($!)
FAKE_OUT=$W/out NETUSB_STATE_DIR=$W/c2 NETUSB_IPC_DIR=$W/c2/ipc "$CLID" run --importer "$ROOT/scripts/ci/fake-usbip.py" > "$W/c2d.log" 2>&1 & PIDS+=($!)
wait_service "$W/b" server && wait_service "$W/c2" client || exit 1
PIN=$(sv "$W/b" pair | pin_from)
cl "$W/c2" pair 127.0.0.1:7601 --pin "$PIN" > /dev/null && ok "client 2 paired with B" || bad "pairing with B"
cl "$W/c2" add 127.0.0.1:7601
LIST=$(cl "$W/c2" list 2>&1); echo "$LIST"
BUSID=$(echo "$LIST" | awk '/1209:0002/{print $1; exit}')
[ -n "$BUSID" ] && ok "server B lists the kernel device as $BUSID" || { bad "server B does not list it"; exit 1; }

echo "== fake driver exercises server B's engine"
cl "$W/c2" use 127.0.0.1:7601 "$BUSID" --seconds 25 > "$W/c2.out" 2> "$W/c2.err"
for _ in $(seq 1 60); do [ -e "$W/out/done" ] && break; sleep 0.5; done
[ -e "$W/out/done" ] && ok "the exercise finished" || bad "the exercise never finished"
cat "$W/out/results.txt" 2>/dev/null | cut -c1-150
N_PASS=$(grep -c '^PASS' "$W/out/results.txt" 2>/dev/null); N_FAIL=$(grep -c '^FAIL' "$W/out/results.txt" 2>/dev/null)
[ "${N_PASS:-0}" -eq 14 ] && [ "${N_FAIL:-1}" -eq 0 ] && ok "all 14 engine checks passed" || bad "engine checks: ${N_PASS:-0} passed, ${N_FAIL:-?} failed"
sleep 2
grep -q "attached $BUSID" "$W/b.log" && ok "server B logged the attach" || bad "no attach logged by B"

echo; echo "result: $PASS passed, $FAIL failed"; [ $FAIL -eq 0 ]
