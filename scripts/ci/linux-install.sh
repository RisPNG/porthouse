#!/usr/bin/env bash
# Does what the README tells a Linux user to do, on a real systemd machine, with the shipped archive:
# install both services, open pairing as root (sudo), pair as an ordinary user, list devices, load the
# web interface, then uninstall. Needs root (run through sudo) and the archive given as argument.
set -u
ARCHIVE=${1:?usage: linux-install.sh path/to/netusb-linux.tar.gz}
USER_NAME=${SUDO_USER:?run through sudo from an ordinary user}
W=$(mktemp -d /tmp/netusb-install.XXXXXX)
PASS=0; FAIL=0
ok()  { echo "  PASS  $1"; PASS=$((PASS+1)); }
bad() { echo "  FAIL  $1"; FAIL=$((FAIL+1)); }
as_user() { sudo -u "$USER_NAME" -H env "PATH=$PATH" "$@"; }
trap 'systemctl status netusb-server netusb-client --no-pager 2>&1 | head -30; journalctl -u netusb-server -u netusb-client --no-pager -n 40 2>&1 | tail -40; rm -rf "$W"' EXIT

tar -xzf "$ARCHIVE" -C "$W"
cd "$W"/netusb-*

echo "== checksums and static linking"
sha256sum -c SHA256SUMS > /dev/null && ok "checksums verify" || bad "checksums"
if file netusb-server | grep -q "statically linked"; then ok "the server is a static binary"; else echo "  NOTE  $(file netusb-server | cut -c1-120)"; fi

echo "== install both services"
./install.sh both > "$W/install.out" 2>&1 && ok "installer succeeded" || { bad "installer failed"; cat "$W/install.out"; }
sleep 2
systemctl is-active --quiet netusb-server && ok "netusb-server service is running" || bad "server service not running"
systemctl is-active --quiet netusb-client && ok "netusb-client service is running" || bad "client service not running"
ss -ltn | grep -q ':7600 ' && ok "the server listens on 7600" || bad "nothing on 7600"
[ -d /var/lib/netusb ] && [ "$(stat -c %U /var/lib/netusb)" = root ] && ok "state lives in /var/lib/netusb, owned by root" || bad "state directory"

echo "== pairing exactly as documented: sudo netusb-server pair, then an ordinary user pairs"
PIN=$(sudo -u root env SUDO_USER="$USER_NAME" /usr/local/bin/netusb-server pair | sed -n 's/.*PIN:  \([0-9-]*\).*/\1/p')
[ -n "$PIN" ] && ok "'sudo netusb-server pair' printed a PIN" || bad "no PIN"
as_user /usr/local/bin/netusb-client pair 127.0.0.1 --pin "$PIN" 2>&1 | grep -q "paired with" && ok "the user paired with the running service (so the PIN reached /var/lib/netusb)" || bad "pairing with the service failed"
as_user /usr/local/bin/netusb-client list --host 127.0.0.1 2>&1 | grep -q "127.0.0.1:7600" && ok "the paired user can list" || bad "list failed"
[ ! -e "/home/$USER_NAME/.local/state/netusb/pairing.pin" ] && ok "no stray PIN file landed in the user's home" || bad "a PIN file was written to the user's home"
[ "$(stat -c %U /var/lib/netusb/server.pem)" = root ] && ok "server key is still owned by root" || bad "server key ownership changed"

echo "== client web interface (from the service)"
for _ in $(seq 1 20); do [ -s /var/lib/netusb/ui.url ] && break; sleep 0.5; done
URL=$(cat /var/lib/netusb/ui.url 2>/dev/null)
[ -n "$URL" ] && ok "ui.url exists" || bad "no ui.url"
curl -s -L -c "$W/jar" -b "$W/jar" "$URL" | grep -q '<title>netusb</title>' && ok "the web interface loads after sign-in" || bad "web interface did not load"
[ "$(stat -c %a /var/lib/netusb/ui.url)" = 600 ] && ok "ui.url is private (0600)" || bad "ui.url permissions"

echo "== uninstall"
./install.sh uninstall --purge > /dev/null 2>&1 && ok "uninstaller succeeded" || bad "uninstaller"
systemctl is-active --quiet netusb-server && bad "server still running" || ok "server stopped"
[ ! -e /usr/local/bin/netusb-server ] && [ ! -e /etc/systemd/system/netusb-server.service ] && ok "files removed" || bad "files left behind"
[ ! -e /var/lib/netusb ] && ok "--purge removed the state directory" || bad "state left behind"

echo; echo "result: $PASS passed, $FAIL failed"; [ $FAIL -eq 0 ]
