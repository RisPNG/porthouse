# shellcheck shell=bash disable=SC2034
# Helpers shared by the test scripts. Source it after setting ROOT.
#
# Every server and client service in a test lives in its own directory DIR: its state is DIR and
# its control socket is DIR/ipc, so several can run side by side without touching the real ones.

BIN_DIR=${PORTHOUSE_BIN_DIR:-$ROOT/target/debug}
SRV=$BIN_DIR/porthouse-server
CLI=$BIN_DIR/porthouse-client
CLID=$BIN_DIR/porthouse-clientd

# sv DIR ARGS...  run a porthouse-server command against the server whose files are in DIR
sv() { PORTHOUSE_STATE_DIR=$1 PORTHOUSE_IPC_DIR=$1/ipc "$SRV" "${@:2}"; }
# cl DIR ARGS...  run a porthouse-client command against the client service whose files are in DIR
cl() { PORTHOUSE_IPC_DIR=$1/ipc "$CLI" "${@:2}"; }
# wait_service DIR server|client  wait until the service in DIR answers on its control socket
wait_service() {
  for _ in $(seq 1 100); do [ -S "$1/ipc/$2.sock" ] && return 0; sleep 0.1; done
  echo "the $2 service in $1 never opened its control socket" >&2
  return 1
}
# pin_from  read the PIN out of `porthouse-server pair` output
pin_from() { sed -n 's/.*PIN:  \([0-9-]*\).*/\1/p'; }
