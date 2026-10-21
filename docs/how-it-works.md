# How it works

## Overview

```
 machine A (has the device)                     machine B (wants the device)

 USB device                                      kernel virtual USB controller
    │ usbfs (Linux) / WinUSB (Windows)             (Linux: vhci_hcd, Windows: usbip-win2 driver)
 netusb-server ◄══ TLS 1.3, both sides authenticated ══► netusb-client
    claims the device, runs its USB requests            relay, discovery, pairing, web interface
```

- **Control connection** (`crates/proto`): length-prefixed frames, optionally LZ4-compressed, carrying typed
  messages: hello, list devices, pair, attach, ping.
- **Data connection:** after `attach`, the *same* encrypted connection carries raw
  [USB/IP](https://docs.kernel.org/usb/usbip_protocol.html) URBs (USB request blocks), the format Linux's
  `vhci_hcd` and the `usbip-win2` driver already speak. The client hands the kernel a private loopback socket
  and relays its other end over TLS, which is how USB/IP gets encryption it does not have by itself.
- **Server engine** (`crates/server/src/engine.rs`): opens the device, claims every interface from the host
  driver, runs one worker per bulk or interrupt endpoint, forwards control transfers, handles cancellation, and
  intercepts the requests the operating system must own (set configuration, set interface, clear halt).
- **On Windows** the `usbip-win2` driver opens its *own* connection and performs the standard USB/IP import.
  netusb answers that on a loopback port that only accepts a random one-time token, then relays the driver's
  traffic over TLS (`crates/client/src/usbip_tool.rs`).
- **Discovery:** servers announce themselves over mDNS (`_netusb._tcp`) on the local network; elsewhere you add
  them by address.

## Security model

- **No certificate authorities.** Every machine has a self-signed certificate; trust is pinning its SHA-256
  fingerprint, and TLS 1.3 only.
- **Pairing is the only way to establish trust.** The server shows a random 8-digit PIN. The client proves it
  knows it with an HMAC over a secret both sides derive from *that TLS session* (the RFC 5705 exporter). A
  man-in-the-middle terminates two different TLS sessions, so the proof does not verify; the server proves it
  knows the PIN back, so a fake server cannot get a client to trust it.
- A PIN works once, expires after 10 minutes, and is destroyed after 5 wrong guesses.
- Unpaired clients can do nothing except pair. Paired clients can be revoked (`netusb-server revoke`).
- The web interface listens on `127.0.0.1`, requires a secret token (HttpOnly, SameSite=Strict cookie), checks
  `Host` and `Origin` against DNS rebinding and cross-site requests, and renders names from other machines as
  text, never HTML.
- State (keys, pairings) is owner-only: `/var/lib/netusb` and `%ProgramData%\netusb` for a service, the user's
  own state directory otherwise.

## Handing a device back

A shared device always returns to its owner: when the client releases it, when the service stops, after a crash
(a journal in the state directory is replayed on the next start), and when the client vanishes without closing
its connection (TCP keepalive on both ends detects that in about 25 seconds).

## Testing

Everything below runs in CI on GitHub-hosted machines (`.github/workflows/ci.yml`), and most of it can be run
by hand from `scripts/`.

| Job | What it proves |
|---|---|
| build, lint, tests | format, clippy with warnings as errors, unit tests, the pairing and authorization attack suite, and the web-interface suite |
| linux attach | real `vhci_hcd`: the virtual mouse enumerates and delivers pointer events, plus raw interrupt reports read through libusb |
| linux engine | a kernel device shared by the real `usbfs` engine and driven through control, bulk, cancel, ordering and overflow checks |
| linux dead-client | a client whose network vanishes releases its device within a minute |
| linux release archive | the static archive, installed on real systemd exactly as the README says, including `sudo netusb-server pair` and uninstall |
| windows build, tests, smoke | the Windows programs run, pair, refuse unpaired clients and list devices |
| windows attach | the real `usbip-win2` driver attaches a virtual mouse, Windows enumerates a HID mouse, the pointer moves (still before attach and after release) |
| windows engine | Windows binds `winusb.sys` to a virtual device itself (from Microsoft OS descriptors) and the native Windows server shares it through the same checks |
| windows installer | the PowerShell installer and uninstaller: scheduled tasks, firewall rules, protected state directory, pairing, web interface, removal |

Hardware-free testing uses two built-in software devices, enabled with `netusb-server run --test-device
mouse|loopback`: a HID mouse that draws a square with the pointer, and a high-speed bulk echo device with
Microsoft OS descriptors. They are opt-in because the mouse moves a pointer.

**Not covered by automated tests:** physical USB devices (beyond manual runs), the Windows client on a desktop
edition (CI uses Windows Server 2022), composite WinUSB devices, and isochronous transfers (unsupported).

## Protocol and format references

- USB/IP: <https://docs.kernel.org/usb/usbip_protocol.html>. netusb uses its URB message format between the
  client and the kernel/driver. The control protocol and everything around it (pairing, framing, discovery,
  the web interface) is netusb's own and is not compatible with other USB-over-network products.
- Microsoft OS 1.0 descriptors (used only by the test device) tell Windows to bind WinUSB automatically.
