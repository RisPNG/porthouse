#!/usr/bin/env bash
# Build the Windows x64 programs and assemble dist/netusb-windows-x64.zip.
#
#   ./scripts/package-windows.sh
#
# Cross-compiles from Linux with a zig toolchain acting as the C compiler and linker, because the
# TLS library (ring) contains C and assembly. Needs, outside this repository:
#   ~/tools/zig/zig                      portable zig (https://ziglang.org/download/), checksum-verified
#   ~/tools/zig-shims/*                  tiny wrappers that make `zig cc` look like MinGW (see below)
#   rustup target add x86_64-pc-windows-gnu --toolchain 1.89.0
# The shims are created by this script if zig is present.

set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
ZIG=${ZIG:-$HOME/tools/zig/zig}
SHIMS=$HOME/tools/zig-shims
[ -x "$ZIG" ] || { echo "zig not found at $ZIG (set ZIG=...)" >&2; exit 1; }
mkdir -p "$SHIMS"

# cc-rs looks for these MinGW tool names; rustc needs a linker and dlltool for import libraries.
cat > "$SHIMS/x86_64-w64-mingw32-gcc" <<EOS
#!/bin/sh
# cc-rs adds --target=x86_64-pc-windows-gnu; zig does not know the "pc" vendor, so drop it.
for a in "\$@"; do shift; case "\$a" in --target=*) ;; *) set -- "\$@" "\$a";; esac; done
exec $ZIG cc -target x86_64-windows-gnu "\$@"
EOS
printf '#!/bin/sh\nexec %s ar "$@"\n' "$ZIG" > "$SHIMS/x86_64-w64-mingw32-ar"
printf '#!/bin/sh\nexec %s dlltool "$@"\n' "$ZIG" > "$SHIMS/x86_64-w64-mingw32-dlltool"
cat > "$SHIMS/zig-link-windows" <<EOS
#!/bin/sh
# Linker wrapper: drop the runtime libraries zig provides itself, and add its libunwind.
for a in "\$@"; do shift; case "\$a" in --target=*|-lgcc_eh|-lgcc|-l:libpthread.a|-lmsvcrt|-lmingw32|-lmingwex|-nodefaultlibs) ;; *) set -- "\$@" "\$a";; esac; done
exec $ZIG cc -target x86_64-windows-gnu "\$@" -lunwind
EOS
chmod +x "$SHIMS"/*

cd "$ROOT"
PATH="$SHIMS:$PATH" CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="$SHIMS/zig-link-windows" \
  cargo build --release --target x86_64-pc-windows-gnu

OUT=$ROOT/dist/netusb-windows-x64
rm -rf "$OUT"; mkdir -p "$OUT"
cp target/x86_64-pc-windows-gnu/release/netusb-client.exe target/x86_64-pc-windows-gnu/release/netusb-server.exe "$OUT/"
cp packaging/windows/install-windows.ps1 packaging/windows/uninstall-windows.ps1 packaging/windows/README-WINDOWS.txt "$OUT/"
( cd "$OUT" && sha256sum * > SHA256SUMS.txt )
( cd "$ROOT/dist" && rm -f netusb-windows-x64.zip && python3 -m zipfile -c netusb-windows-x64.zip netusb-windows-x64 )
echo
ls -l "$ROOT/dist/netusb-windows-x64.zip"
cat "$OUT/SHA256SUMS.txt"
