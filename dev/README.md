# Porthouse development

This is everything about working on Porthouse: building it, how it is put together, testing, packaging, and releasing. If you only want to use it, the [main README](../README.md) is the place to start.

## Build

### The services

You need a Rust toolchain. `rust-toolchain.toml` pins the version and `rustup` installs it automatically. Building also needs a C compiler, because the TLS library contains C and assembly.

```bash
cargo build --release          # target/release/porthouse-server, porthouse-clientd, and porthouse-client
cargo test --workspace
```

Layout: `crates/proto` (wire formats), `crates/common` (identity, trust, TLS, paths, Windows service support), `crates/ipc` (control channel), `crates/server`, `crates/client` (library, service, and command line), `gui/` (the Qt program), `packaging/` (Linux, Arch, and Windows packaging), `scripts/` (test suites and packaging), `docs/`.

### The program

The program needs Qt 6.4 or later with Qt Quick Controls, CMake, and Python 3.11 or later.

```bash
cmake -S gui -B build/gui -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build/gui
QT_QPA_PLATFORM=offscreen ctest --test-dir build/gui --output-on-failure
```

`porthouse --screenshot FILE --tab use|share|activity` saves a picture of the window and exits, which is how CI checks that every tab draws.

### Run the tests

```bash
cargo test --workspace                 # unit tests
./scripts/e2e-security.sh              # attacks on pairing and authorization (needs nc, openssl, python3)
./scripts/e2e-claim.sh 5-1             # claim and release of a REAL local device (see the warning in the script)
./scripts/e2e-windows-path.sh 5-1      # the Windows attach route end to end on Linux
sudo ./scripts/live-test.sh 5-1 30     # a real device re-attached through the Linux kernel
```

The scripts that use a real device take it away from the computer for a few seconds and always give it back, so use a device that you can spare. `PORTHOUSE_BIN_DIR=/path/to/binaries` makes the suites test a different build, such as an installed package. Each script starts its own services with a private state directory and control socket, so they do not touch the installed ones.

### Build the packages

CI builds them all (`.github/workflows/packages.yml` and `windows.yml`), and the steps there are the reference. In short:

```bash
# Linux package for the system you are on (needs the Rust programs built for musl)
cargo build --release --target x86_64-unknown-linux-musl -p porthouse-server -p porthouse-client
cmake -S . -B build -G Ninja -DCMAKE_INSTALL_PREFIX=/usr -DPORTHOUSE_PACKAGE_FLAVOR=debian-12 -DPORTHOUSE_DEB_DEPENDS="..."
cmake --build build && (cd build && cpack -G DEB)     # or -G RPM with -DPORTHOUSE_RPM_REQUIRES

# Arch
(cd packaging/arch && makepkg -si)

# Windows installer, after building the Rust programs and the program, with Inno Setup installed
./scripts/package-windows.ps1 -GuiBuild build\gui
```


### Debugging

`PORTHOUSE_TRACE=1` on the server logs every USB request and reply. `PORTHOUSE_STATE_DIR=/some/dir` points a program at an isolated state directory and `PORTHOUSE_IPC_DIR=/some/dir` at an isolated control channel, which is how the tests run several computers on one host.

## How it works

### Overview

```
 computer A (has the device)                   computer B (wants the device)

 USB device                                      kernel virtual USB controller
    | usbfs (Linux) or WinUSB (Windows)            (Linux: vhci_hcd, Windows: usbip-win2 driver)
 porthouse-server <== TLS 1.3, both sides authenticated ==> porthouse-clientd
    claims the device, runs its USB requests            relay, discovery, pairing, attachments
    ^                                                   ^
    | control channel                                   | control channel
 Porthouse program (window and tray icon), porthouse-client command line
```

- The control connection (`crates/proto`) carries length-prefixed frames, optionally LZ4 compressed, with typed messages: hello, list devices, pair, attach, and ping.
- After an attach, the same encrypted connection carries raw [USB/IP](https://docs.kernel.org/usb/usbip_protocol.html) URBs (USB request blocks), the format that Linux's `vhci_hcd` and the usbip-win2 driver already speak. The client service hands the kernel a private loopback socket and relays its other end over TLS, which is how USB/IP gets encryption it does not have by itself.
- The server engine (`crates/server/src/engine.rs`) opens the device, claims every interface from the host driver, runs one worker per bulk or interrupt endpoint, forwards control transfers, handles cancellation, and intercepts the requests that the operating system must own: set configuration, set interface, and clear halt.
- On Windows the usbip-win2 driver opens its own connection and performs the standard USB/IP import. The client service answers that on a loopback port that only accepts a random one-time token, then relays the driver's traffic over TLS (`crates/client/src/usbip_tool.rs`).
- Servers announce themselves over mDNS (`_porthouse._tcp`) on the local network. Elsewhere, computers are added by address.

### Services and programs

Attaching a device needs root on Linux and Administrator rights on Windows, and opening raw USB devices on the sharing side needs the same. Those parts therefore run as services, and everything a person touches runs as an ordinary user.

| Part | Runs as | Does |
|---|---|---|
| `porthouse-server` | root or SYSTEM | shares devices, answers pairing, serves the control channel |
| `porthouse-clientd` | root or SYSTEM | finds servers, pairs, attaches and keeps attachments alive, serves the control channel |
| `porthouse` | the user | window and notification-area icon for both services |
| `porthouse-client`, `porthouse-server pair` | the user | command line for the same control channels |

Each service has a control channel in `crates/ipc`: one JSON object per line over a Unix socket (`/run/porthouse/server.sock` and `client.sock`) or a named pipe (`\\.\pipe\porthouse-server` and `porthouse-client`). The reply to `{"id":1,"method":"state"}` is `{"id":1,"result":{...}}` or `{"id":1,"error":"..."}`.

On Linux the service decides per connection from the peer's credentials. Root, the account running the service, members of the `porthouse` group, and users with an active local login session are allowed. A refused peer receives an error that says to join the group. On Windows the pipe is limited to SYSTEM, administrators, and interactive users. Pairing, attaching, and revoking are therefore available to whoever is signed in at the computer, which is the same trust as having the keyboard in front of it.

### Security model

- There are no certificate authorities. Every computer has a self-signed certificate, trust means pinning its SHA-256 fingerprint, and only TLS 1.3 is accepted.
- Pairing is the only way to establish trust. The server shows a random 8-digit PIN. The client proves that it knows the PIN with an HMAC over a secret that both sides derive from that TLS session (the RFC 5705 exporter). A man-in-the-middle terminates two different TLS sessions, so the proof does not verify. The server proves that it knows the PIN back, so a fake server cannot get a client to trust it.
- A PIN works once, expires after 10 minutes, and is destroyed after 5 wrong guesses. A pairing window opens only when someone asks for it.
- Unpaired clients can do nothing except pair. Paired clients can be revoked.
- Names that arrive from other computers are shown as text, never interpreted.
- Keys and pairings are readable only by the service account: `/var/lib/porthouse` on Linux and `%ProgramData%\porthouse` on Windows, limited to SYSTEM and administrators.

### Handing a device back

A shared device always returns to its owner: when the client releases it, when the service stops, after a crash (a journal in the state directory is replayed on the next start), and when the client vanishes without closing its connection. TCP keepalive on both ends detects that last case in about 25 seconds.

### Packaging

Linux packages come from one CMake project (`CMakeLists.txt`) that installs the three Rust programs (static musl builds), the Qt program from `gui/`, the systemd units, the module list that loads `vhci_hcd`, the autostart entry, and the licences. CPack builds the .deb and .rpm inside a container of each target system, so the library dependencies come from that system. The Arch recipe is in `packaging/`. The Windows installer is built by Inno Setup (`packaging/windows/porthouse.iss`) from the Rust programs, the Qt program with its runtime from `windeployqt`, and the pinned usbip-win2 installer.

### Testing

Everything below runs in continuous integration on GitHub-hosted machines, and most of it can be run by hand from `scripts/`.

| Workflow and job | What it proves |
|---|---|
| ci: build, lint, tests | Clippy correctness and suspicious-code checks, unit tests, and the pairing and authorisation attack suite |
| ci: linux attach | real `vhci_hcd`: the virtual mouse enumerates and delivers pointer events, plus raw interrupt reports read through libusb |
| ci: linux engine | a kernel device shared by the real `usbfs` engine and driven through control, bulk, cancel, ordering, and overflow checks |
| ci: linux dead-client | a client whose network vanishes releases its device within a minute |
| packages: build and install | each Debian, Ubuntu, and Fedora package is built in its own container, installed on a clean system, draws every tab of the program, and passes the attack suite |
| packages: systemd | the Ubuntu package on a real systemd machine: services start, a group member pairs and attaches the virtual mouse through the hardened service, the program draws against the live services, removal and purge clean up |
| packages: Arch | the Arch recipe builds and installs |
| windows: programs | the Windows programs run, pair, refuse unpaired clients, and list devices |
| windows: attach | the real usbip-win2 driver attaches a virtual mouse, Windows enumerates a HID mouse, and the pointer moves (and stays still before attach and after release) |
| windows: engine | Windows binds `winusb.sys` to a virtual device by itself (from Microsoft OS descriptors) and the native Windows server shares it through the same checks |
| windows: installer | the installer on a clean machine: files, services, firewall rules, protected key directory, driver, shortcuts, pairing through the control pipes, the program against the live services, and removal |

Hardware-free testing uses two built-in software devices, enabled with `porthouse-server run --test-device mouse` or `loopback`. One is a HID mouse that draws a square with the pointer and the other is a high-speed bulk echo device with Microsoft OS descriptors. They are opt-in because the mouse moves a pointer.

Not covered by automated tests: physical USB devices (beyond manual runs), the Windows client on a desktop edition (CI uses Windows Server 2022), the notification-area icon on real desktops, composite WinUSB devices, and isochronous transfers, which are unsupported.

### References

- USB/IP: <https://docs.kernel.org/usb/usbip_protocol.html>. Porthouse uses its URB message format between the client service and the kernel or driver. The control protocol and everything around it (pairing, framing, discovery, the control channel) is Porthouse's own and is not compatible with other USB-over-network products.
- Microsoft OS 1.0 descriptors, used only by the test device, tell Windows to bind WinUSB automatically.

## Licensing

Porthouse's own code is under the [0BSD licence](../LICENSE). Third-party code keeps its own licence and the notices have to travel with the binaries:

- `licenses/rust-crates.txt` lists every Rust crate with its licence text. Regenerate it with `python3 scripts/third-party-licenses.py` whenever `Cargo.lock` changes. CI fails when it is stale.
- `licenses/LGPL-3.0.txt` and `GPL-3.0.txt` accompany the Qt files that the Windows installer ships, and `THIRD-PARTY-LICENSES.md` explains how to replace them and where the Qt source is. Do not add Qt modules that are GPL-only or commercial-only to the program.
- `licenses/usbip-win2.txt` is the BSD-2-Clause text of the driver whose installer the Windows installer bundles.
- The Linux packages, the Arch package, and the Windows installer install `LICENSE`, `THIRD-PARTY-LICENSES.md`, and the `licenses` folder. Add every new kind of bundled component to `THIRD-PARTY-LICENSES.md`.

## Releasing and signing

### Make a release

1. Raise `version` in `Cargo.toml` and rename the "Unreleased" section of `CHANGELOG.md` to `## x.y.z (date)`.
2. Commit and wait for CI to pass.
3. Tag and push: `git tag vx.y.z && git push origin vx.y.z`.

The `release` workflow checks that the tag matches the version, builds every package, signs what it can, and publishes everything with `SHA256SUMS`, using the changelog section as the notes.

### Windows code signing (SignPath Foundation)

The Windows installer is signed by [SignPath Foundation](https://signpath.org/), which signs open-source projects for free with its own certificate. The workflow already contains the signing step. It runs only when the repository secret `SIGNPATH_API_TOKEN` and the repository variables `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`, and `SIGNPATH_SIGNING_POLICY_SLUG` exist, so releases made before that point ship an unsigned installer.

To set it up once:

1. Apply at <https://signpath.org/apply>. The repository has to be public, under an OSI-approved licence, and the project has to be released and in use. Porthouse meets the licence and public requirements.
2. When approved, create the project in SignPath with the artifact configuration described in `packaging/windows/signpath-artifact.xml` and a release signing policy that requires approval by you.
3. Create an API token for the CI user in SignPath and add it as the repository secret `SIGNPATH_API_TOKEN`. Add the three repository variables from the list above.
4. Add the attribution that SignPath asks for to the README: free code signing provided by SignPath.io, certificate by SignPath Foundation, plus the code signing policy and privacy statement text that they give you.

### Linux signing (GPG)

Every release asset gets a detached signature (`.asc`), `SHA256SUMS` is signed, and the `.rpm` files carry an embedded signature. The public key is published as `porthouse-signing.asc` in the repository. The workflow signs only when the repository secrets `GPG_PRIVATE_KEY` and `GPG_PASSPHRASE` exist.

To set it up once:

```bash
gpg --quick-generate-key "Porthouse releases <hello@rispeng.com>" ed25519 sign 3y
KEY=$(gpg --list-secret-keys --with-colons "Porthouse releases" | awk -F: '/^fpr/{print $10; exit}')
gpg --armor --export "$KEY" > porthouse-signing.asc  # commit this file to the repository root
gpg --armor --export-secret-keys "$KEY" | gh secret set GPG_PRIVATE_KEY --repo RisPNG/porthouse
gh secret set GPG_PASSPHRASE --repo RisPNG/porthouse   # the key's passphrase, or skip this when the key has none
```

Keep a backup of the private key somewhere safe and offline. Anyone can then check a download with `gpg --import porthouse-signing.asc` and `gpg --verify FILE.asc FILE`.
