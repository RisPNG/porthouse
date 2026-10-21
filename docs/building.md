# Building, testing, and releasing

## Build the services

You need a Rust toolchain. `rust-toolchain.toml` pins the version and `rustup` installs it automatically. Building also needs a C compiler, because the TLS library contains C and assembly.

```bash
cargo build --release          # target/release/netusb-server, netusb-clientd, and netusb-client
cargo test --workspace
```

Layout: `crates/proto` (wire formats), `crates/common` (identity, trust, TLS, paths, Windows service support), `crates/ipc` (control channel), `crates/server`, `crates/client` (library, service, and command line), `gui/` (the Qt program), `packaging/` (Linux, Arch, Flatpak, and Windows packaging), `scripts/` (test suites and packaging), `docs/`.

## Build the program

The program needs Qt 6.4 or later with Qt Quick Controls, and CMake.

```bash
cmake -S gui -B build/gui -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build/gui
QT_QPA_PLATFORM=offscreen ctest --test-dir build/gui --output-on-failure
```

`netusb --screenshot FILE --tab use|share|activity` saves a picture of the window and exits, which is how CI checks that every tab draws.

## Run the tests

```bash
cargo test --workspace                 # unit tests
./scripts/e2e-security.sh              # attacks on pairing and authorization (needs nc, openssl, python3)
./scripts/e2e-claim.sh 5-1             # claim and release of a REAL local device (see the warning in the script)
./scripts/e2e-windows-path.sh 5-1      # the Windows attach route end to end on Linux
sudo ./scripts/live-test.sh 5-1 30     # a real device re-attached through the Linux kernel
```

The scripts that use a real device take it away from the computer for a few seconds and always give it back, so use a device that you can spare. `NETUSB_BIN_DIR=/path/to/binaries` makes the suites test a different build, such as an installed package. Each script starts its own services with a private state directory and control socket, so they do not touch the installed ones.

## Build the packages

CI builds them all (`.github/workflows/packages.yml` and `windows.yml`), and the steps there are the reference. In short:

```bash
# Linux package for the system you are on (needs the Rust programs built for musl)
cargo build --release --target x86_64-unknown-linux-musl -p netusb-server -p netusb-client
cmake -S . -B build -G Ninja -DCMAKE_INSTALL_PREFIX=/usr -DNETUSB_PACKAGE_FLAVOR=debian-12 -DNETUSB_DEB_DEPENDS="..."
cmake --build build && (cd build && cpack -G DEB)     # or -G RPM with -DNETUSB_RPM_REQUIRES

# Arch
(cd packaging/arch && makepkg -si)

# Flatpak
flatpak-builder --user --install --force-clean build-flatpak packaging/flatpak/io.github.rispng.netusb.yml

# Windows installer, after building the Rust programs and the program, with Inno Setup installed
./scripts/package-windows.ps1 -GuiBuild build\gui
```

## Make a release

1. Raise `version` in `Cargo.toml` and add a section to `CHANGELOG.md`.
2. Commit and wait for CI to pass.
3. Tag and push: `git tag vx.y.z && git push origin vx.y.z`.

The `release` workflow checks that the tag matches the version, builds every package, and publishes them with `SHA256SUMS`, using the changelog section as the notes.

## Debugging

`NETUSB_TRACE=1` on the server logs every USB request and reply. `NETUSB_STATE_DIR=/some/dir` points a program at an isolated state directory and `NETUSB_IPC_DIR=/some/dir` at an isolated control channel, which is how the tests run several computers on one host.
