#!/usr/bin/env bash
# netusb installer for Linux with systemd.
#
#   sudo ./install.sh server      share this machine's USB devices
#   sudo ./install.sh client      use USB devices shared by other machines
#   sudo ./install.sh both
#   sudo ./install.sh uninstall   [--purge]   remove it (--purge also deletes keys and pairings)
#
# Options (put after the command):
#   --no-start        install but do not enable or start the services
# Environment:
#   PREFIX=/usr/local   where the programs go (a "bin" directory is created under it)
#   DESTDIR=/some/dir   stage the files under a directory instead of the real system (no root needed,
#                       no services started): used for testing and for building packages
#   BIN_DIR=...         where the netusb-server and netusb-client programs are (default: next to this script)
#
# Run from the extracted release archive, or from a source checkout after `cargo build --release`:
#   sudo BIN_DIR=../../target/release ./packaging/linux/install.sh server

set -eu

CMD=${1:-}
shift || true
START=1; PURGE=0
for a in "$@"; do
  case "$a" in
    --no-start) START=0 ;;
    --purge) PURGE=1 ;;
    *) echo "unknown option: $a" >&2; exit 2 ;;
  esac
done
case "$CMD" in server|client|both|uninstall) ;; *) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;; esac

HERE=$(cd "$(dirname "$0")" && pwd)
PREFIX=${PREFIX:-/usr/local}
DESTDIR=${DESTDIR:-}
BIN_DIR=${BIN_DIR:-$HERE}
UNIT_SRC=$HERE
[ -f "$UNIT_SRC/netusb-server.service" ] || UNIT_SRC=$HERE/..
UNIT_DIR=$DESTDIR/etc/systemd/system

if [ -z "$DESTDIR" ] && [ "$(id -u)" -ne 0 ]; then
  echo "This needs root. Run it with sudo." >&2
  exit 1
fi
HAVE_SYSTEMD=0
[ -z "$DESTDIR" ] && command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ] && HAVE_SYSTEMD=1

if [ "$CMD" = uninstall ]; then
  if [ "$HAVE_SYSTEMD" = 1 ]; then
    for u in netusb-server netusb-client; do
      systemctl disable --now "$u.service" 2>/dev/null || true
    done
  fi
  rm -f "$UNIT_DIR/netusb-server.service" "$UNIT_DIR/netusb-client.service" \
        "$DESTDIR/etc/modules-load.d/netusb.conf" \
        "$DESTDIR$PREFIX/bin/netusb-server" "$DESTDIR$PREFIX/bin/netusb-client"
  [ "$HAVE_SYSTEMD" = 1 ] && systemctl daemon-reload
  if [ "$PURGE" = 1 ]; then
    rm -rf "$DESTDIR/var/lib/netusb"
    echo "Removed netusb, including its keys and pairings (/var/lib/netusb)."
    echo "Per-user data in ~/.local/state/netusb is yours to delete."
  else
    echo "Removed netusb. Keys and pairings were kept in /var/lib/netusb (use --purge to delete them)."
  fi
  exit 0
fi

for f in netusb-server netusb-client; do
  [ -x "$BIN_DIR/$f" ] || { echo "cannot find $f in $BIN_DIR (set BIN_DIR, or run from the extracted archive)" >&2; exit 1; }
done
for f in netusb-server.service netusb-client.service; do
  [ -f "$UNIT_SRC/$f" ] || { echo "cannot find $f next to this script" >&2; exit 1; }
done

install -d "$DESTDIR$PREFIX/bin" "$UNIT_DIR"
install -m 0755 "$BIN_DIR/netusb-server" "$BIN_DIR/netusb-client" "$DESTDIR$PREFIX/bin/"
echo "Installed netusb-server and netusb-client to $DESTDIR$PREFIX/bin"

install_unit() {
  sed "s|@PREFIX@|$PREFIX|g" "$UNIT_SRC/$1" > "$UNIT_DIR/$1"
  chmod 0644 "$UNIT_DIR/$1"
}

if [ "$CMD" = server ] || [ "$CMD" = both ]; then
  install_unit netusb-server.service
fi
if [ "$CMD" = client ] || [ "$CMD" = both ]; then
  install_unit netusb-client.service
  install -d "$DESTDIR/etc/modules-load.d"
  echo vhci-hcd > "$DESTDIR/etc/modules-load.d/netusb.conf"
  if [ -z "$DESTDIR" ] && ! modprobe vhci-hcd 2>/dev/null; then
    echo "WARNING: could not load the vhci-hcd kernel module, which the client needs to attach devices." >&2
    echo "         On Ubuntu try: sudo apt install linux-modules-extra-\$(uname -r)" >&2
  fi
fi

if [ "$HAVE_SYSTEMD" = 1 ]; then
  systemctl daemon-reload
  if [ "$START" = 1 ]; then
    [ "$CMD" = server ] || [ "$CMD" = both ] && systemctl enable --now netusb-server.service
    [ "$CMD" = client ] || [ "$CMD" = both ] && systemctl enable --now netusb-client.service
  fi
fi

echo
if [ "$CMD" = server ] || [ "$CMD" = both ]; then
  echo "SERVER: it listens on TCP port 7600 on every network interface. If you run a firewall, allow that port"
  echo "        only from networks you trust (for example: sudo ufw allow from 192.168.1.0/24 to any port 7600)."
  echo "        To let another machine use your devices, open a pairing window and give it the PIN:"
  echo "            sudo netusb-server pair"
fi
if [ "$CMD" = client ] || [ "$CMD" = both ]; then
  echo "CLIENT: open the web interface with:   sudo cat /var/lib/netusb/ui.url"
  echo "        (paste the address into your browser; it works for about as long as the service runs)"
fi
