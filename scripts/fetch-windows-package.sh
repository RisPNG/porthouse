#!/usr/bin/env bash
# Download the Windows zip that CI built and tested, into dist/.
#   ./scripts/fetch-windows-package.sh [RUN_ID]      (default: the latest successful run on main)
# Needs the GitHub CLI (gh), logged in with access to the repository.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner)
RUN=${1:-$(gh run list --repo "$REPO" --branch main --workflow ci --status success --limit 1 --json databaseId -q '.[0].databaseId')}
[ -n "$RUN" ] || { echo "no successful CI run found" >&2; exit 1; }
mkdir -p "$ROOT/dist"
gh run download "$RUN" --repo "$REPO" -n netusb-windows-x64 -D "$ROOT/dist"
echo "from CI run https://github.com/$REPO/actions/runs/$RUN"
ls -l "$ROOT"/dist/netusb-*-windows-x64.zip
( cd "$ROOT/dist" && sha256sum netusb-*-windows-x64.zip )
