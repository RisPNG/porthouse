# Changelog

## Unreleased

### Added
- The netusb program: a Qt window and a notification-area icon that cover both sides. Use devices from other computers, share this computer's devices, pair, release, and see recent activity. It runs on Linux and Windows.
- Native packages: .deb for Debian 12, Debian 13, and Ubuntu 24.04, .rpm for Fedora 43, an Arch recipe, a Flatpak for the program, and a Windows installer built with Inno Setup that also installs the usbip-win2 driver.
- Services instead of scripts: `netusb-server` and `netusb-clientd` run as systemd services on Linux and as Windows services on Windows, and restart after a failure.
- A control channel for each service (a Unix socket on Linux and a named pipe on Windows), used by the program and by the command line.
- The command line works against the services, so `netusb-client use` and `netusb-server pair` need no root. `netusb-client release`, `add`, and `remove` are new.
- A "Do not ask again" option for the question before an input device is taken.
- Servers that were added by address are remembered by the client service.

### Changed
- A pairing window opens only when someone asks for it. The server no longer opens one on its first start.
- The client attaches devices through its own service, so an ordinary user can do it.

### Removed
- The web interface and its sign-in address file, replaced by the program.
- The install scripts and the archives that came with them, replaced by the packages.

## 0.2.0 (21 October 2026)

First release.

### Added
- Server and client for Linux and Windows. Share a USB device from one computer and use it from another as if it were plugged in locally. Bulk, interrupt, and control devices work, such as mice, keyboards, serial adapters, storage, dongles, and programmers.
- Encryption and authentication. TLS 1.3 with certificates on both sides, pinned by fingerprint, and one-time PIN pairing bound to the TLS session so that a man-in-the-middle cannot relay it. Unpaired computers can do nothing but pair.
- A web interface for the client that finds servers on the local network, shows their devices, pairs, attaches, and releases. It warns before taking an input device, because the sharing computer loses it.
- A command line for both programs.
- Linux: a static x86_64 archive with an installer that sets up systemd services.
- Windows: a zip with a PowerShell installer and uninstaller. The client attaches devices through the usbip-win2 driver.
- Safe hand-back. A shared device returns to its owner when the client leaves, on service shutdown, after a crash, and when a client vanishes without closing its connection (about 25 seconds).
- Built-in software test devices (`--test-device mouse` and `loopback`) that CI uses to exercise real drivers without hardware.

### Known limitations
- Isochronous devices such as webcams and USB audio are not supported.
- On Windows a device can only be shared if it uses the WinUSB driver.
- Changing a device's active configuration from the client is not supported.
- Windows is tested on Windows Server 2022 in CI and on one desktop machine for the server side. The client attach path has not been run on a Windows desktop edition yet.
