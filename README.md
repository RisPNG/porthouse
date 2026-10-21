# netusb

Use a USB device that is plugged into another computer as if it were plugged into yours.

Plug a mouse, keyboard, serial adapter, flash drive, licence dongle, or programmer into one computer and use it from another over your network. The computer that has the device shares it, and the computer that wants it uses it. Both work on Linux and Windows, everything is encrypted, and a new computer has to be approved with a one-time PIN.

- A device moves, it is not copied. While one computer uses it, the other does not have it. Release it and it returns on its own.
- There is no cloud and no account. Computers talk directly to each other.
- It is safe by default: TLS 1.3 with pinned certificates, PIN pairing, and a device is handed back if the other computer disappears.

netusb is made of two background services and a program. The server service shares this computer's devices. The client service plugs in devices that other computers share. The netusb program has a window and a notification-area icon for both, so there is nothing to run by hand.

## Install

Download the file for your system from the Releases page.

| System | File | How |
|---|---|---|
| Debian 12 or 13, Ubuntu 24.04 | `netusb-<system>-<version>-amd64.deb` | `sudo apt install ./netusb-*.deb` |
| Fedora 43 | `netusb-fedora-43-<version>-x86_64.rpm` | `sudo dnf install ./netusb-*.rpm` |
| Arch Linux | `PKGBUILD` in `packaging/arch` | `makepkg -si` in that directory |
| Any Linux, program only | `netusb-<version>-x86_64.flatpak` | `flatpak install netusb-*.flatpak` |
| Windows 10 or 11 (x64) | `netusb-<version>-windows-x64-setup.exe` | run it |

The Linux packages start both services and add the program to the application menu. The Windows installer asks which parts to install, installs the usbip-win2 driver when this computer will use devices, opens the firewall for sharing, and starts the services. The installer is not code signed yet, so Windows SmartScreen asks for confirmation.

The Flatpak contains only the program. The services run on the host, so install one of the packages too. On Arch the services are installed but not started: run `sudo systemctl enable --now netusb-server netusb-client`.

## Use it

1. On the computer that has the device, open netusb, go to Share devices, and press Allow a new computer to pair. A PIN appears.
2. On the computer that wants the device, open netusb. If the other computer is not listed, add it by address. Press Pair next to it and enter the PIN.
3. Press Use next to the device. Press Release to give it back.

On Linux, anyone signed in at the computer can use the program. Other accounts need to be in the `netusb` group: `sudo usermod -aG netusb USER`, then sign in again.

### Walkthroughs

| You want to | Read |
|---|---|
| Share a Linux computer's mouse or keyboard with a Windows PC | [docs/linux-to-windows.md](docs/linux-to-windows.md) |
| Use a device from one Linux computer on another | [docs/linux-to-linux.md](docs/linux-to-linux.md) |
| Use a device plugged into a Windows PC from Linux | [docs/windows-to-linux.md](docs/windows-to-linux.md) |
| Fix something that does not work, or a device stuck on the wrong computer | [docs/troubleshooting.md](docs/troubleshooting.md) |

There is also a command line, `netusb-client` and `netusb-server`, for scripts and servers without a screen.

## Security in brief

Anyone you pair with can use any shareable device on that server, including your keyboard and mouse, so only pair computers you control. The server listens on TCP port 7600 on all interfaces. Restrict it with your firewall to networks you trust and never expose it to the internet. The details, and how to report a problem, are in [SECURITY.md](SECURITY.md) and [docs/how-it-works.md](docs/how-it-works.md).

## What works and what does not yet

| | |
|---|---|
| Mice, keyboards, game controllers, serial adapters, storage, dongles, programmers | works |
| Webcams, USB audio, and anything else that needs isochronous transfers | not supported |
| Sharing from Windows | the device has to use the WinUSB driver, see [docs/windows-to-linux.md](docs/windows-to-linux.md) |
| Using a device on Windows | needs the usbip-win2 driver, which the installer adds |
| macOS | not available |
| Linux on ARM | not packaged yet, build it yourself with [docs/building.md](docs/building.md) |
| openSUSE | no package yet |

How well is it tested? Every change runs on real Windows and Linux machines in continuous integration with the real kernel drivers and software test devices. That covers attaching a virtual mouse and watching the pointer move, sharing a device through the Windows and Linux engines, installing every package on a clean system, installing and removing the Windows services, and attacking the pairing. By hand, a physical mouse was passed through the Linux kernel's virtual USB controller on one machine, and the Windows server was used from Linux against a real Windows desktop over a VPN. Not covered yet: the Windows client on a desktop edition of Windows, because CI uses Windows Server 2022, moving a physical mouse between two separate computers, and a wide range of physical devices. Details are in [docs/how-it-works.md](docs/how-it-works.md#testing).

## More

- [docs/how-it-works.md](docs/how-it-works.md) covers the architecture, protocol, security model, and testing.
- [docs/building.md](docs/building.md) covers building from source, running the tests, and making a release.
- [CHANGELOG.md](CHANGELOG.md) lists what changed.

## Licence

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

netusb uses the public USB/IP protocol format and, on Windows, the third-party usbip-win2 driver, which is BSD-2-Clause licensed.
