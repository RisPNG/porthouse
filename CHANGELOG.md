# Changelog

## [0.2.0] - 2026-10-21

First release.

### Added
- **Server and client for Linux and Windows.** Share a USB device from one machine and use it from another as if it were plugged in locally. Bulk, interrupt and control devices work (mice, keyboards, serial adapters, storage, dongles, programmers).
- **Encrypted and authenticated.** TLS 1.3 with certificates on both sides, pinned by fingerprint, and one-time PIN pairing bound to the TLS session so a man-in-the-middle cannot relay it. Unpaired machines can do nothing but pair.
- **Web interface** for the client: finds servers on the local network, shows their devices, pairs, attaches and releases. Warns before taking a mouse or keyboard, because the sharing machine loses it.
- **Command line** (`netusb-client list|pair|use|servers|forget|gui|open`, `netusb-server run|pair|clients|revoke`).
- **Linux:** static x86_64 archive with an installer that sets up systemd services (`sudo ./install.sh server|client|both`, `uninstall [--purge]`).
- **Windows:** zip with PowerShell installer and uninstaller (scheduled tasks, firewall rules, protected state directory). The client attaches devices through the `usbip-win2` driver.
- **Safe hand-back.** A shared device returns to its owner when the client leaves, on service shutdown, after a crash (recovery journal), and when a client vanishes without closing its connection (TCP keepalive, about 25 seconds).
- Built-in software test devices (`--test-device mouse|loopback`) used by CI to exercise real drivers without hardware.

### Known limitations
- Isochronous devices (webcams, USB audio) are not supported.
- On Windows a device can only be shared if it uses the WinUSB driver.
- Changing a device's active configuration from the client is not supported.
- Windows is tested on Windows Server 2022 in CI and on one desktop machine for the server side; the client attach path has not yet been run on a Windows desktop edition.
