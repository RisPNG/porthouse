# netusb

Share USB devices over the network. Run a **server** on the machine the device is plugged into, and
use the device from another machine as if it were plugged in locally: a camera, a licence dongle,
a programmer, a mouse.

The server runs as a service and announces itself on the network. The client finds servers
automatically, shows their devices in a small web interface, and attaches a device with one click.
Everything is encrypted, and a new client must be approved with a one-time PIN.

> **Status:** Linux and Windows server and client are implemented and exercised end to end in CI on
> real machines with real drivers (see [Continuous integration](#continuous-integration)). What has
> *not* been done is a run on your own hardware: physical USB devices, Windows desktop editions, and
> a person moving a physical mouse. See [Status and limitations](#status-and-limitations).

## Quick start (Linux)

Build once (Rust 1.89, pinned by `rust-toolchain.toml`):

```bash
cargo build --release
```

**On the machine with the USB device** (needs root for the service, not for trying it out):

```bash
sudo ./scripts/install-linux.sh server      # installs and starts the systemd service
sudo netusb-server pair                     # prints a one-time 8-digit PIN, valid 10 minutes
```

**On the machine that wants to use the device:**

```bash
sudo ./scripts/install-linux.sh client      # loads vhci_hcd, starts the client service
sudo cat /var/lib/netusb/ui.url             # open this address in your browser
```

In the browser, press **Pair…** on the server, enter the PIN, then **Use** next to a device.

Without installing anything, from the source tree:

```bash
./target/debug/netusb-server run                        # machine A (no root needed for a device you own)
./target/debug/netusb-server pair                       # machine A: prints a PIN
sudo -E ./target/debug/netusb-client gui                # machine B: prints the web address
```

Command line equivalents: `netusb-client list`, `pair HOST`, `use HOST BUSID`, `ports`, `servers`,
`forget`; and on the server `pair`, `clients`, `revoke`.

## How it works

```
 machine A (has the device)                     machine B (wants the device)

 USB device                                      kernel vhci_hcd (virtual USB host controller)
    │ usbfs / WinUSB via nusb                          ▲ loopback socket owned by the kernel
 netusb-server ◄══ TLS 1.3, mutual, pinned ══► netusb-client (relay + web UI)
    │  claims device, runs URBs                        │  mDNS discovery, pairing, attach manager
    └─ mDNS announce (_netusb._tcp)                    └─ browser UI on 127.0.0.1
```

* **Control plane** (`crates/proto`): length-prefixed frames, optional LZ4, typed messages. Hello,
  list devices, pair, attach, ping.
* **Data plane**: after `Attach` the same encrypted connection carries raw
  [USB/IP](https://docs.kernel.org/usb/usbip_protocol.html) URBs, the format Linux's `vhci_hcd`
  already understands. The client hands the kernel a private loopback socket and relays its other
  end over TLS, which is how plain USB/IP gets encryption it does not have by itself.
* **Server USB engine** (`crates/server/src/engine.rs`): claims every interface, runs one worker per
  bulk or interrupt endpoint, forwards control transfers, handles cancellation (unlink), and
  intercepts the requests the operating system must own (set configuration, set interface, clear
  halt).
* **Device hand-back**: when a client leaves, on `SIGTERM`, and after a crash (a journal in the state
  directory is replayed on the next start) the device goes back to its normal host driver.

## Security model

* TLS 1.3 only, with certificates on both sides. There are **no certificate authorities**. Each side
  pins the other's certificate fingerprint.
* **Pairing** is the only way to establish trust. The server shows a random 8-digit PIN; the client
  proves it knows it. The proof is an HMAC bound to *that TLS session* (RFC 5705 exporter), so a
  man-in-the-middle cannot relay it, and the server proves it knows the PIN back, so a fake server
  cannot trick a client into trusting it.
* A PIN is single-use, expires after 10 minutes, and dies after 5 wrong guesses.
* Unpaired clients can do nothing but pair. Paired clients can be revoked (`netusb-server revoke`).
* The web interface listens on `127.0.0.1` only and requires a secret token (HttpOnly,
  SameSite=Strict cookie), a loopback `Host` header, and for writes a JSON content type and
  same-origin `Origin`. Names from other machines are rendered as text, never HTML.
* The server runs as root to open USB devices. Anyone who is paired can take any shareable device on
  that server, so only pair machines you trust. The UI token file (`ui.url`) is owner-only.

All of this is exercised by `scripts/e2e-security.sh` and `scripts/e2e-ui.sh`, which try to break it
(wrong and reused PINs, lockout, a TLS-terminating relay, revoked clients, a changed server
certificate, forged `Host`, cross-site writes, plaintext and random bytes).

## Testing

```bash
cargo test                       # 54 unit tests (framing, USB/IP, pairing, trust stores, vhci parsing, import shim)
./scripts/e2e-security.sh        # 17 attack tests against pairing and authorization; no root
./scripts/e2e-ui.sh              # 25 tests of the web interface; no root
./scripts/e2e-claim.sh 5-1       # claim and release of a REAL device: leaving, SIGTERM, SIGKILL recovery
./scripts/e2e-windows-path.sh 5-1  # the Windows attach route end to end on Linux, with a fake driver, no root
sudo ./scripts/live-test.sh 5-1  # full loopback: a local device re-attached through vhci_hcd
```

`e2e-claim.sh` and `live-test.sh` take the device away from the host for a few seconds at a time and
always restore it. Use a device you can spare, and see the notes at the top of each script.

## Continuous integration

Every push runs these on GitHub-hosted machines (`.github/workflows/ci.yml`):

| Job | What it proves |
|---|---|
| linux build, lint, tests | format, clippy with warnings as errors, 54 unit tests, the pairing/authorization attack suite and the web-interface suite |
| linux attach | real `vhci_hcd`: the virtual mouse enumerates and delivers pointer events (and, independent of any HID driver, raw interrupt reports read through libusb) |
| linux engine | a kernel device (the virtual loopback device attached via `vhci_hcd`) shared by a second server using the real `usbfs` engine, driven by a fake importer |
| windows build, tests, smoke | the Windows programs run, pair, refuse unpaired clients, list devices |
| windows attach | installs the real `usbip-win2` driver, attaches the virtual mouse, checks Windows enumerates a HID mouse and the pointer moves (with a still-pointer control before and after) |
| windows engine | the virtual loopback device is bound to `winusb.sys` by Windows itself (from Microsoft OS descriptors), shared by the native Windows server, and driven through control, bulk, cancel, ordering and overflow checks |
| windows installer | the PowerShell installer and uninstaller: scheduled tasks as SYSTEM and as the user, firewall rules, protected state directory, pairing between them, web interface, clean removal |

The Windows programs built there use a statically linked C runtime and import only Windows system DLLs.

## Testing without hardware: the virtual devices

`netusb-server run --test-device mouse` adds a software HID mouse (`99-1`, VID:PID 1209:0001) that
draws a small square with the pointer of whoever attaches it; `--test-device loopback` adds a
high-speed vendor device (`99-2`, 1209:0002) with a bulk OUT and a bulk IN endpoint that echo data
and Microsoft OS descriptors that make Windows bind WinUSB to it. Together they make the whole attach
path, kernel or driver included, testable on a machine with no spare USB hardware, with real
interrupt and bulk traffic. They are opt-in because the mouse moves a pointer.

`netusb-client use HOST BUSID --seconds N` attaches for a fixed time, and `NETUSB_TRACE=1` makes the
server log every URB it handles.

## Status and limitations

| Area | State |
|---|---|
| Linux server (control, TLS, pairing, claim/release, crash recovery) | done, tested on hardware |
| Linux client attach via `vhci_hcd` | done; enumeration, descriptors and HID setup verified with a real mouse |
| Mouse movement over the full path | **not yet confirmed**: the test machine was driven remotely; needs a run at the physical mouse |
| Web interface, mDNS discovery, pairing UI | done, tested (API, security, and a browser pass) |
| systemd units and installer | written; unit files validated with `systemd-analyze`, installer **not run** |
| Windows client | **tested on real Windows in CI** (GitHub Windows Server 2022): attaches through the real `usbip-win2` driver; Windows enumerates a HID mouse and the pointer moves. Not yet run on a desktop edition or by you |
| Windows server (WinUSB) | **tested on real Windows in CI**: shares a virtual WinUSB device bound by `winusb.sys` itself (control, bulk, cancel, ordering, overflow). Physical hardware and composite devices untested; devices must use the WinUSB driver (e.g. via Zadig) |
| Windows installer, scheduled tasks, firewall, protected state dir | **tested on real Windows in CI**: install, SYSTEM server, user client, pairing between them, web interface, uninstall |
| Linux engine on a real kernel device | **tested in CI**: bulk and control through `usbfs`, cancel, ordering, overflow |
| Isochronous transfers (webcams, USB audio) | **not supported**: such URBs are refused. Bulk, interrupt and control devices work |
| Changing the active configuration from the client | not supported (refused); the device's current configuration is used |
| Multiple controllers, IPv6-only networks, auto-reattach, per-device access rules | not done |

## Windows

```bash
./scripts/fetch-windows-package.sh   # -> dist/netusb-windows-x64.zip, built and tested by CI
```

The zip contains the programs from the CI run that passed on real Windows (a statically linked C
runtime, nothing to install besides the `usbip-win2` driver for the client), the PowerShell installer
and uninstaller, and `README-WINDOWS.txt`. `./scripts/package-windows.sh` is an alternative that
cross-builds from Linux with zig, but those binaries are *not* what CI tests.

On Windows the `usbip-win2` driver opens its own connection and performs the standard USB/IP import,
so netusb answers that on a loopback port protected by a random one-time token, and relays the
traffic over TLS.
