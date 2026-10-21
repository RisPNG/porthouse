# Helpers shared by the test scripts. Source it after setting ROOT.
#
# Every server and client service in a test lives in its own directory DIR: its state is DIR and
# its control socket is DIR/ipc, so several can run side by side without touching the real ones.

BIN_DIR=${NETUSB_BIN_DIR:-$ROOT/target/debug}
SRV=$BIN_DIR/netusb-server
CLI=$BIN_DIR/netusb-client
CLID=$BIN_DIR/netusb-clientd

# sv DIR ARGS...  run a netusb-server command against the server whose files are in DIR
sv() { NETUSB_STATE_DIR=$1 NETUSB_IPC_DIR=$1/ipc "$SRV" "${@:2}"; }
# cl DIR ARGS...  run a netusb-client command against the client service whose files are in DIR
cl() { NETUSB_IPC_DIR=$1/ipc "$CLI" "${@:2}"; }
# wait_service DIR server|client  wait until the service in DIR answers on its control socket
wait_service() {
  for _ in $(seq 1 100); do [ -S "$1/ipc/$2.sock" ] && return 0; sleep 0.1; done
  echo "the $2 service in $1 never opened its control socket" >&2
  return 1
}
# pin_from  read the PIN out of `netusb-server pair` output
pin_from() { sed -n 's/.*PIN:  \([0-9-]*\).*/\1/p'; }
