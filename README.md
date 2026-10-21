# Porthouse

Plug a USB device into one computer and use it on another, as if it were plugged in right there. Think of it as a port house for your USB ports: each computer keeps its ports, and any of them can be reached from the others.

> [!IMPORTANT]  
> This project is entirely "vibe-coded". Which means maintenance are not guaranteed as it was made only for my own convenience.
> 
> You can still submit issues and pull requests, but I won't promise to work on them or review them. If anything, it is better that you fork the project instead.
> 
> Although I believe that I provide meaningful research, direction, engineering, and architecture as an experienced developer in guiding it during the development of this project, I am aware that this project is not something exclusive to me. I am confident that anyone, with enough time and determination, can develop a similar or better project that can serve the same purpose. That is not to say I am pointing the blame to the AI if anything do goes wrong in the result of this project. That responsibility still is entirely on me and to each of you that uses it. Please do your own due diligence. Thank you for understanding.

## What can you do with it?

- Use the mouse or keyboard that is plugged into your Linux computer on your Windows PC, and move it back whenever you like.
- Reach a serial adapter, flash drive, licence dongle, or programmer that is plugged into a computer in another room.
- Share a device between a laptop and a desktop without moving cables.

A device moves, it does not get copied. While one computer uses it, the other does not have it. Release it and it goes back on its own, and it also goes back if the computer using it disappears. There is no cloud and no account. Computers talk directly to each other, and everything is encrypted.

## Install

Download the file for your system from the [Releases page](https://github.com/RisPNG/porthouse/releases/latest).

| Your system | Download | Then |
|---|---|---|
| Windows 10 or 11 | `porthouse-<version>-windows-x64-setup.exe` | run it |
| Ubuntu 24.04, Debian 12 or 13 | `porthouse-<system>-<version>-amd64.deb` | `sudo apt install ./porthouse-*.deb` |
| Fedora 43 | `porthouse-fedora-43-<version>-x86_64.rpm` | `sudo dnf install ./porthouse-*.rpm` |
| Arch Linux | `PKGBUILD` from [packaging/arch](packaging/arch) | `makepkg -si` in that folder, then `sudo systemctl enable --now porthouse-server porthouse-client` |
| Any Linux, program only | `porthouse-<version>-x86_64.flatpak` | `flatpak install porthouse-*.flatpak` (install one of the packages above as well, because the Flatpak is only the window) |

Both Linux and Windows installs start everything for you and add Porthouse to your applications. The Windows installer asks what you want to do on that PC and installs the USB/IP driver it needs when you want to use devices from other computers. See [how to check your download](docs/verifying-downloads.md) if you like to verify what you run. Windows shows a SmartScreen warning for installers that are not signed.

## Use it

1. On the computer that has the device, open Porthouse, go to Share devices, and press Allow a new computer to pair. A PIN appears.
2. On the computer that should use the device, open Porthouse. If the other computer is not listed, add it by its address. Press Pair next to it and type the PIN.
3. Press Use next to the device. Press Release when you are done.

Porthouse keeps a small icon in your notification area so you can see what is in use at a glance. Closing the window does not stop anything.

Step by step guides:

- [Share a Linux computer's mouse or keyboard with a Windows PC](docs/linux-to-windows.md)
- [Use a device from one Linux computer on another](docs/linux-to-linux.md)
- [Use a device plugged into a Windows PC from Linux](docs/windows-to-linux.md)
- [Something does not work](docs/troubleshooting.md)

## Good to know

- Anyone you pair with can use any shareable device on that computer, including its keyboard and mouse. Only pair computers you control.
- Porthouse listens on TCP port 7600. Allow it in your firewall for your own network only, and never expose it to the internet. [SECURITY.md](SECURITY.md) has the details.
- Mice, keyboards, game controllers, serial adapters, storage, dongles, and programmers work. Webcams and USB audio do not, because they need a kind of transfer that is not supported.
- Sharing from a Windows PC only works for devices that use the WinUSB driver, see [that guide](docs/windows-to-linux.md).
- There is no macOS version, and no ready-made package for ARM or openSUSE yet.
- It has been tested a lot with software test devices and real drivers on every change, and by hand with a physical mouse. It has not been tested on many different physical devices, or with the Windows client on a desktop edition of Windows.

## For developers

How it works inside, how to build it, how it is tested, and how releases are made are in [dev/README.md](dev/README.md). The list of changes is in [CHANGELOG.md](CHANGELOG.md).

## Licence

Porthouse is released under the [0BSD licence](LICENSE), which lets you do anything with it, no strings attached.

The programs also contain or use other people's work under their own licences: Rust libraries, Qt, and on Windows the usbip-win2 driver. They are listed in [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md), and the full texts are installed with Porthouse.
