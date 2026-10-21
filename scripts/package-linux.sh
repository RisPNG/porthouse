#!/usr/bin/env bash
# Build the Linux release archive: dist/netusb-<version>-linux-<arch>.tar.gz
#
#   ./scripts/package-linux.sh                       static x86_64 build (needs: rustup target add x86_64-unknown-linux-musl, musl-tools)
#   TARGET=x86_64-unknown-linux-gnu ./scripts/package-linux.sh   a dynamically linked build for this machine
#
# The archive is self-contained: programs, systemd units, installer, licenses and checksums.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
TARGET=${TARGET:-x86_64-unknown-linux-musl}
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ARCH=${TARGET%%-*}
NAME=netusb-$VERSION-linux-$ARCH

cargo build --release --target "$TARGET" -p netusb-server -p netusb-client
OUT=$ROOT/dist/$NAME
rm -rf "$OUT"; mkdir -p "$OUT"
cp "target/$TARGET/release/netusb-server" "target/$TARGET/release/netusb-client" "$OUT/"
cp packaging/netusb-server.service packaging/netusb-client.service packaging/linux/install.sh packaging/linux/README-LINUX.txt "$OUT/"
cp LICENSE-MIT LICENSE-APACHE "$OUT/"
( cd "$OUT" && sha256sum netusb-server netusb-client > SHA256SUMS )
( cd "$ROOT/dist" && tar --owner=0 --group=0 -czf "$NAME.tar.gz" "$NAME" )
echo "built dist/$NAME.tar.gz"
( cd "$ROOT/dist" && sha256sum "$NAME.tar.gz" )
