# How it works

## Overview

```
 computer A (has the device)                   computer B (wants the device)

 USB device                                      kernel virtual USB controller
    | usbfs (Linux) or WinUSB (Windows)            (Linux: vhci_hcd, Windows: usbip-win2 driver)
 netusb-server <== TLS 1.3, both sides authenticated ==> netusb-clientd
    claims the device, runs its USB requests            relay, discovery, pairing, attachments
    ^                                                   ^
    | control channel                                   | control channel
 netusb program (window and tray icon), netusb-client command line
```

- The control connection (`crates/proto`) carries length-prefixed frames, optionally LZ4 compressed, with typed messages: hello, list devices, pair, attach, and ping.
- After an attach, the same encrypted connection carries raw [USB/IP](https://docs.kernel.org/usb/usbip_protocol.html) URBs (USB request blocks), the format that Linux's `vhci_hcd` and the usbip-win2 driver already speak. The client service hands the kernel a private loopback socket and relays its other end over TLS, which is how USB/IP gets encryption it does not have by itself.
- The server engine (`crates/server/src/engine.rs`) opens the device, claims every interface from the host driver, runs one worker per bulk or interrupt endpoint, forwards control transfers, handles cancellation, and intercepts the requests that the operating system must own: set configuration, set interface, and clear halt.
- On Windows the usbip-win2 driver opens its own connection and performs the standard USB/IP import. The client service answers that on a loopback port that only accepts a random one-time token, then relays the driver's traffic over TLS (`crates/client/src/usbip_tool.rs`).
- Servers announce themselves over mDNS (`_netusb._tcp`) on the local network. Elsewhere, computers are added by address.

## Services and programs

Attaching a device needs root on Linux and Administrator rights on Windows, and opening raw USB devices on the sharing side needs the same. Those parts therefore run as services, and everything a person touches runs as an ordinary user.

| Part | Runs as | Does |
|---|---|---|
| `netusb-server` | root or SYSTEM | shares devices, answers pairing, serves the control channel |
| `netusb-clientd` | root or SYSTEM | finds servers, pairs, attaches and keeps attachments alive, serves the control channel |
| `netusb` | the user | window and notification-area icon for both services |
| `netusb-client`, `netusb-server pair` | the user | command line for the same control channels |

Each service has a control channel in `crates/ipc`: one JSON object per line over a Unix socket (`/run/netusb/server.sock` and `client.sock`) or a named pipe (`\\.\pipe\netusb-server` and `netusb-client`). The reply to `{"id":1,"method":"state"}` is `{"id":1,"result":{...}}` or `{"id":1,"error":"..."}`.

On Linux the service decides per connection from the peer's credentials. Root, the account running the service, members of the `netusb` group, and users with an active local login session are allowed. A refused peer receives an error that says to join the group. On Windows the pipe is limited to SYSTEM, administrators, and interactive users. Pairing, attaching, and revoking are therefore available to whoever is signed in at the computer, which is the same trust as having the keyboard in front of it.

## Security model

- There are no certificate authorities. Every computer has a self-signed certificate, trust means pinning its SHA-256 fingerprint, and only TLS 1.3 is accepted.
- Pairing is the only way to establish trust. The server shows a random 8-digit PIN. The client proves that it knows the PIN with an HMAC over a secret that both sides derive from that TLS session (the RFC 5705 exporter). A man-in-the-middle terminates two different TLS sessions, so the proof does not verify. The server proves that it knows the PIN back, so a fake server cannot get a client to trust it.
- A PIN works once, expires after 10 minutes, and is destroyed after 5 wrong guesses. A pairing window opens only when someone asks for it.
- Unpaired clients can do nothing except pair. Paired clients can be revoked.
- Names that arrive from other computers are shown as text, never interpreted.
- Keys and pairings are readable only by the service account: `/var/lib/netusb` on Linux and `%ProgramData%\netusb` on Windows, limited to SYSTEM and administrators.

## Handing a device back

A shared device always returns to its owner: when the client releases it, when the service stops, after a crash (a journal in the state directory is replayed on the next start), and when the client vanishes without closing its connection. TCP keepalive on both ends detects that last case in about 25 seconds.

## Packaging

Linux packages come from one CMake project (`CMakeLists.txt`) that installs the three Rust programs (static musl builds), the Qt program from `gui/`, the systemd units, the module list that loads `vhci_hcd`, the autostart entry, and the licences. CPack builds the .deb and .rpm inside a container of each target system, so the library dependencies come from that system. The Arch recipe and the Flatpak manifest are in `packaging/`. The Windows installer is built by Inno Setup (`packaging/windows/netusb.iss`) from the Rust programs, the Qt program with its runtime from `windeployqt`, and the pinned usbip-win2 installer.

## Testing

Everything below runs in continuous integration on GitHub-hosted machines, and most of it can be run by hand from `scripts/`.

| Workflow and job | What it proves |
|---|---|
| ci: build, lint, tests | formatting, clippy with warnings as errors, unit tests, and the pairing and authorization attack suite |
| ci: linux attach | real `vhci_hcd`: the virtual mouse enumerates and delivers pointer events, plus raw interrupt reports read through libusb |
| ci: linux engine | a kernel device shared by the real `usbfs` engine and driven through control, bulk, cancel, ordering, and overflow checks |
| ci: linux dead-client | a client whose network vanishes releases its device within a minute |
| packages: build and install | each Debian, Ubuntu, and Fedora package is built in its own container, installed on a clean system, draws every tab of the program, and passes the attack suite |
| packages: systemd | the Ubuntu package on a real systemd machine: services start, a group member pairs and attaches the virtual mouse through the hardened service, the program draws against the live services, removal and purge clean up |
| packages: Arch and Flatpak | the Arch recipe builds and installs, and the Flatpak builds and runs |
| windows: programs | the Windows programs run, pair, refuse unpaired clients, and list devices |
| windows: attach | the real usbip-win2 driver attaches a virtual mouse, Windows enumerates a HID mouse, and the pointer moves (and stays still before attach and after release) |
| windows: engine | Windows binds `winusb.sys` to a virtual device by itself (from Microsoft OS descriptors) and the native Windows server shares it through the same checks |
| windows: installer | the installer on a clean machine: files, services, firewall rules, protected key directory, driver, shortcuts, pairing through the control pipes, the program against the live services, and removal |

Hardware-free testing uses two built-in software devices, enabled with `netusb-server run --test-device mouse` or `loopback`. One is a HID mouse that draws a square with the pointer and the other is a high-speed bulk echo device with Microsoft OS descriptors. They are opt-in because the mouse moves a pointer.

Not covered by automated tests: physical USB devices (beyond manual runs), the Windows client on a desktop edition (CI uses Windows Server 2022), the notification-area icon on real desktops, composite WinUSB devices, and isochronous transfers, which are unsupported.

## References

- USB/IP: <https://docs.kernel.org/usb/usbip_protocol.html>. netusb uses its URB message format between the client service and the kernel or driver. The control protocol and everything around it (pairing, framing, discovery, the control channel) is netusb's own and is not compatible with other USB-over-network products.
- Microsoft OS 1.0 descriptors, used only by the test device, tell Windows to bind WinUSB automatically.
