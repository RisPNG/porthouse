# Building, testing and releasing

## Build

You need a Rust toolchain; `rust-toolchain.toml` pins the version and `rustup` installs it automatically.
Building also needs a C compiler (the TLS library contains C and assembly).

```bash
cargo build --release          # target/release/netusb-server and netusb-client
cargo test --workspace
```

Layout: `crates/proto` (wire formats), `crates/common` (identity, trust, TLS, paths), `crates/server`,
`crates/client` (library, command line, web interface), `packaging/` (installers, service units),
`scripts/` (test suites, packaging), `docs/`.

## Run the tests

```bash
cargo test --workspace                 # unit tests
./scripts/e2e-security.sh              # attacks on pairing and authorization (needs nc, openssl, python3)
./scripts/e2e-ui.sh                    # the web interface
./scripts/e2e-claim.sh 5-1             # claim/release of a REAL local device (see the warning in the script)
./scripts/e2e-windows-path.sh 5-1      # the Windows attach route end to end on Linux
sudo ./scripts/live-test.sh 5-1 30     # a real device re-attached through the Linux kernel
```

The scripts that use a real device take it away from the machine for a few seconds and always give it back.
Use a device you can spare. `NETUSB_BIN_DIR=/path/to/binaries` makes the suites test a different build, such as
the extracted release archive.

## Packages

```bash
./scripts/package-linux.sh                                  # static x86_64 archive (needs the musl target and musl-tools)
TARGET=x86_64-unknown-linux-gnu ./scripts/package-linux.sh  # a dynamic build for this machine
./scripts/package-windows.ps1                               # Windows zip, after `cargo build --release` on Windows
./scripts/fetch-windows-package.sh                          # download the Windows zip that CI built and tested
```

## Make a release

1. Bump `version` in `Cargo.toml` and add a `## [x.y.z]` section to `CHANGELOG.md`.
2. Commit, wait for CI to pass.
3. Tag and push: `git tag vx.y.z && git push origin vx.y.z`.

The `release` workflow checks that the tag matches the version, builds the Linux archive and the Windows zip,
and publishes them with `SHA256SUMS`, using the changelog section as the notes.

## Debugging

`NETUSB_TRACE=1` on the server logs every USB request and reply. `NETUSB_STATE_DIR=/some/dir` points a program
at an isolated state directory, which is how the tests run several machines on one host.
