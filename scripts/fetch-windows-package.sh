#!/usr/bin/env bash
# Assemble dist/netusb-windows-x64.zip from the Windows programs that CI built AND tested.
#
#   ./scripts/fetch-windows-package.sh [RUN_ID]      (default: the latest successful run on main)
#
# Needs the GitHub CLI (gh) logged in with access to this repository. The programs in the zip are
# the artifact of the "windows build, tests, smoke" job, built natively with MSVC and a statically
# linked C runtime. The installer scripts and README come from the working tree.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner)
RUN=${1:-$(gh run list --repo "$REPO" --branch main --workflow ci --status success --limit 1 --json databaseId -q '.[0].databaseId')}
[ -n "$RUN" ] || { echo "no successful CI run found" >&2; exit 1; }
echo "using CI run $RUN of $REPO"

W=$(mktemp -d); trap 'rm -rf "$W"' EXIT
gh run download "$RUN" --repo "$REPO" -n netusb-windows-x64-native -D "$W"
OUT=$ROOT/dist/netusb-windows-x64
rm -rf "$OUT"; mkdir -p "$OUT"
cp "$W/target/release/netusb-client.exe" "$W/target/release/netusb-server.exe" "$OUT/"
cp "$ROOT"/packaging/windows/install-windows.ps1 "$ROOT"/packaging/windows/uninstall-windows.ps1 "$ROOT"/packaging/windows/README-WINDOWS.txt "$OUT/"
echo "CI run: https://github.com/$REPO/actions/runs/$RUN" > "$OUT/BUILT-BY.txt"
( cd "$OUT" && sha256sum * > SHA256SUMS.txt )
( cd "$ROOT/dist" && rm -f netusb-windows-x64.zip && python3 -m zipfile -c netusb-windows-x64.zip netusb-windows-x64 )
ls -l "$ROOT/dist/netusb-windows-x64.zip"; cat "$OUT/SHA256SUMS.txt"
