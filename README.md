# netusb

**Use a USB device that is plugged into another computer, as if it were plugged into yours.**

Plug a mouse, keyboard, serial adapter, flash drive, licence dongle or programmer into one machine,
and use it from another over your network. The machine with the device runs the **server**; the machine
that wants it runs the **client**. Both work on Linux and Windows, everything is encrypted, and a new
machine has to be approved with a one-time PIN.

- **Moves a device, does not copy it.** While you use it on one machine, the other machine does not have it.
  Give it back and it returns on its own.
- **No cloud, no account.** Machines talk directly to each other.
- **Safe by default.** TLS 1.3 with pinned certificates, PIN pairing, and a device is handed back if the
  other machine disappears.

## Install

Download the archive for your system from the **Releases** page and check it against `SHA256SUMS`.

### Linux (x86_64)

```bash
tar xzf netusb-*-linux-x86_64.tar.gz && cd netusb-*-linux-x86_64
sha256sum -c SHA256SUMS
sudo ./install.sh server     # this machine shares its USB devices
sudo ./install.sh client     # this machine uses devices shared by others
```

`sudo ./install.sh uninstall` removes it (add `--purge` to delete keys and pairings too).

### Windows 10 / 11 (x64)

1. **Only if this PC will *use* devices from another machine:** install the virtual USB driver
   [usbip-win2](https://github.com/vadimgrn/usbip-win2/releases) (its Microsoft-signed x64 installer; version 0.9.8.1 is the one tested).
   Create a restore point first, as that project advises.
2. Unzip the archive, open **PowerShell as Administrator** in the unzipped folder, and run:

```powershell
powershell -ExecutionPolicy Bypass -File .\install-windows.ps1 -Role client   # or -Role server, or both
```

Details and caveats are in `README-WINDOWS.txt` inside the zip.

## Use it

The same three steps every time:

1. **Server:** on the machine that has the device, open a pairing window and note the PIN it prints:
   `sudo netusb-server pair` (Linux) or `netusb-server.exe pair` (Windows, as Administrator).
2. **Client:** open the web interface (Linux: `sudo cat /var/lib/netusb/ui.url` and open that address;
   Windows: **netusb** in the Start Menu). If the server is not listed, **add it by address**.
   Press **Pair…** and enter the PIN.
3. Press **Use** next to the device. Press **Release** to give it back.

### Walkthroughs

| You want to… | Read |
|---|---|
| **Share a Linux machine's mouse or keyboard with a Windows PC** | [docs/linux-to-windows.md](docs/linux-to-windows.md) |
| Use a USB device from one Linux machine on another Linux machine | [docs/linux-to-linux.md](docs/linux-to-linux.md) |
| Use a device plugged into a Windows PC from Linux | [docs/windows-to-linux.md](docs/windows-to-linux.md) |
| Something is not working, or a device is stuck on the wrong machine | [docs/troubleshooting.md](docs/troubleshooting.md) |

## Security in brief

Anyone you pair with can use **any** shareable device on that server, including your keyboard and mouse,
so only pair machines you control. The server listens on TCP port 7600 on all interfaces: restrict it with
your firewall to networks you trust and never expose it to the internet. Full details, and how to report a
problem, are in [SECURITY.md](SECURITY.md) and [docs/how-it-works.md](docs/how-it-works.md).

## What works and what does not (yet)

| | |
|---|---|
| Mice, keyboards, game controllers, serial adapters, storage, dongles, programmers | works |
| Webcams, USB audio and anything else that needs isochronous transfers | **not supported** |
| Sharing from Windows | a device must use the **WinUSB** driver (see [docs/windows-to-linux.md](docs/windows-to-linux.md)) |
| Using a device on Windows | needs the `usbip-win2` driver and Administrator rights |
| macOS | not available |
| Linux on ARM (Raspberry Pi) | not packaged yet; build it yourself ([docs/building.md](docs/building.md)) |

**How well tested is it?** Every change runs on real Windows and Linux machines in continuous
integration, with the real kernel drivers and software test devices: attaching a virtual mouse
(and watching the pointer move), sharing a device through the Windows and Linux engines, installing and
uninstalling the services, and attacking the pairing. By hand, a physical mouse was passed through the
Linux kernel's virtual USB controller on one machine, and the Windows server was exercised from a
Linux machine against a real Windows desktop over a VPN. **Not yet covered:** the Windows *client* on a
desktop edition of Windows (CI uses Windows Server 2022), moving a physical mouse between two separate
machines, and a wide range of physical devices. Details in
[docs/how-it-works.md](docs/how-it-works.md#testing).

## More

- [docs/how-it-works.md](docs/how-it-works.md): architecture, protocol, security model, testing
- [docs/building.md](docs/building.md): build from source, run the tests, make a release
- [CHANGELOG.md](CHANGELOG.md)

## License

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

netusb uses the public USB/IP protocol format and, on Windows, the third-party `usbip-win2` driver.
