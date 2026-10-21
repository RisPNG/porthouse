# Changelog

## Unreleased

### Changed
- Porthouse is now released under the 0BSD licence instead of MIT or Apache-2.0.

### Removed
- The Flatpak. It could only hold the window, not the services that do the sharing and attaching, so it needed a host package anyway.

### Added
- The licences of the Rust libraries, Qt, and the usbip-win2 driver are installed with every package and with the Windows installer, and listed in `THIRD-PARTY-LICENSES.md`.

## 0.3.0 (21 October 2026)

First public release. The project was called netusb before this release and is now Porthouse. Nothing carries over from earlier builds, so computers have to be paired again.

### Added
- Share a USB device from one computer and use it from another as if it were plugged in locally, on Linux and Windows. Bulk, interrupt, and control devices work, such as mice, keyboards, serial adapters, storage, dongles, and programmers.
- The Porthouse program: a window and a notification-area icon that cover both sides. Use devices from other computers, share this computer's devices, pair, release, and see recent activity. Taking an input device asks first, and the question can be turned off.
- Encryption and authentication. TLS 1.3 with certificates on both sides, pinned by fingerprint, and one-time PIN pairing bound to the TLS session so that a man-in-the-middle cannot relay it. Unpaired computers can do nothing but pair. A pairing window opens only when someone asks for it.
- Native packages: .deb for Debian 12, Debian 13, and Ubuntu 24.04, .rpm for Fedora 43, an Arch recipe, and a Windows installer that also installs the usbip-win2 driver.
- Services: `porthouse-server` and `porthouse-clientd` run as systemd services on Linux and as Windows services, and restart after a failure. The program and the command line talk to them through a local socket or named pipe, so ordinary users can pair, attach, and release.
- A command line, `porthouse-client` and `porthouse-server`.
- Safe hand-back. A shared device returns to its owner when the client leaves, on service shutdown, after a crash, and when a client vanishes without closing its connection (about 25 seconds).
- Built-in software test devices (`--test-device mouse` and `loopback`) that CI uses to exercise real drivers without hardware.

### Known limitations
- Isochronous devices such as webcams and USB audio are not supported.
- On Windows a device can only be shared if it uses the WinUSB driver.
- Changing a device's active configuration from the client is not supported.
- Windows is tested on Windows Server 2022 in CI and on one desktop machine for the server side. The client attach path has not been run on a Windows desktop edition yet.
- The notification-area icon has been checked in screenshots only, not on real desktops.
- The Windows installer is not signed yet, and downloads are not signed with a key yet. See [docs/verifying-downloads.md](docs/verifying-downloads.md).
