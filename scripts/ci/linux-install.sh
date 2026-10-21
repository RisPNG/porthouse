#!/usr/bin/env bash
# Installs the Debian package on a real systemd machine and uses it the way a person would:
# services start, a user in the porthouse group pairs and attaches a device, the program shows both
# sides, and removing the package cleans up. Needs root, the vhci-hcd module and a .deb file.
set -u
PKG=${1:?usage: linux-install.sh porthouse.deb}
PKG=$(readlink -f "$PKG")
USER_NAME=${SUDO_USER:?run with sudo from a normal account}
PASS=0; FAIL=0
ok()  { echo "  PASS  $1"; PASS=$((PASS+1)); }
bad() { echo "  FAIL  $1"; FAIL=$((FAIL+1)); }
W=$(mktemp -d /tmp/porthouse-install.XXXXXX); chmod 777 "$W"
trap 'systemctl status porthouse-server porthouse-client --no-pager 2>&1 | head -30; journalctl -u porthouse-server -u porthouse-client --no-pager -n 60 2>&1 | tail -60; rm -rf "$W"' EXIT
as_member() { sudo -u "$USER_NAME" sg porthouse -c "$*"; }
as_plain()  { sudo -u "$USER_NAME" "$@"; }

echo "== install"
modprobe vhci-hcd 2>/dev/null || true
DEBIAN_FRONTEND=noninteractive apt-get install -y "$PKG" usbutils > "$W/apt.log" 2>&1 && ok "the package installs with its dependencies" || { bad "apt install failed"; tail -20 "$W/apt.log"; exit 1; }
systemctl is-active --quiet porthouse-server && ok "the server service is running" || bad "server service not running"
systemctl is-active --quiet porthouse-client && ok "the client service is running" || bad "client service not running"
systemctl is-enabled --quiet porthouse-server && systemctl is-enabled --quiet porthouse-client && ok "both services start at boot" || bad "services are not enabled"
getent group porthouse > /dev/null && ok "the porthouse group exists" || bad "no porthouse group"

echo "== access"
UID_NOW=$(id -u "$USER_NAME")
if grep -qE '^STATE=(active|online)$' "/run/systemd/users/$UID_NOW" 2>/dev/null; then
  echo "  SKIP  $USER_NAME has a login session here, so access without the group is allowed by design"
else
  as_plain porthouse-client list > "$W/denied.out" 2>&1
  grep -qi "not allowed" "$W/denied.out" && ok "a user outside the group is refused, with a message that says what to do" || { bad "unexpected answer for a user outside the group"; cat "$W/denied.out"; }
fi
usermod -aG porthouse "$USER_NAME"
as_member porthouse-client list > "$W/list.out" 2>&1 && ok "a member of the porthouse group can talk to the client service" || { bad "group member refused"; cat "$W/list.out"; }

echo "== a test device, pairing, attaching"
mkdir -p /etc/systemd/system/porthouse-server.service.d
printf '[Service]\nExecStart=\nExecStart=/usr/bin/porthouse-server run --test-device mouse\n' > /etc/systemd/system/porthouse-server.service.d/test.conf
systemctl daemon-reload && systemctl restart porthouse-server
for _ in $(seq 1 50); do as_member porthouse-server clients > /dev/null 2>&1 && break; sleep 0.2; done
PIN=$(as_member porthouse-server pair | sed -n 's/.*PIN:  \([0-9-]*\).*/\1/p')
[ -n "$PIN" ] && ok "a group member opened a pairing window through the service" || bad "no PIN"
as_member porthouse-client pair 127.0.0.1 --pin "$PIN" 2>&1 | grep -q "paired with" && ok "the client service paired with the server service" || bad "pairing failed"
as_member porthouse-client list 2>&1 | grep -q "99-1" && ok "the virtual mouse is listed" || bad "virtual mouse not listed"
as_member porthouse-client use 127.0.0.1 99-1 --seconds 600 2>&1 | grep -q attached && ok "attach command succeeded" || bad "attach failed"
for _ in $(seq 1 40); do lsusb 2>/dev/null | grep -qi 1209:0001 && break; sleep 0.5; done
lsusb 2>/dev/null | grep -qi 1209:0001 && ok "the kernel has the device, attached by the hardened service" || bad "device not on the virtual bus"

echo "== the program shows both sides"
for tab in use share activity; do
  as_member "QT_QPA_PLATFORM=offscreen porthouse --screenshot $W/$tab.png --tab $tab" 2>&1 | grep -v 'does not support raise'
  [ -s "$W/$tab.png" ] && ok "the $tab tab was drawn against the live services" || bad "the $tab tab was not drawn"
done

echo "== release and remove"
as_member porthouse-client release 127.0.0.1 99-1 > /dev/null; sleep 2
lsusb 2>/dev/null | grep -qi 1209:0001 && bad "device still present after release" || ok "the virtual device is gone after release"
rm -rf /etc/systemd/system/porthouse-server.service.d; systemctl daemon-reload
apt-get remove -y porthouse > /dev/null 2>&1
systemctl is-active --quiet porthouse-server && bad "server still running after removal" || ok "services stopped by removal"
[ ! -e /usr/bin/porthouse-server ] && [ ! -e /usr/lib/systemd/system/porthouse-server.service ] && ok "files removed" || bad "files left behind"
[ -d /var/lib/porthouse ] && ok "keys and pairings are kept on a plain removal" || bad "state removed too early"
apt-get purge -y porthouse > /dev/null 2>&1
[ ! -e /var/lib/porthouse ] && ok "purge removes keys and pairings" || bad "purge left /var/lib/porthouse"

echo; echo "result: $PASS passed, $FAIL failed"; [ $FAIL -eq 0 ]
