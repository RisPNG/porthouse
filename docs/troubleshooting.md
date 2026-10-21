# Troubleshooting

## The server does not appear in the client

Automatic discovery only works on the same local network. Over a VPN, or across subnets, type the server's
address in **Add a server by address** (or use `--host ADDRESS` on the command line).

If adding it by address shows *unreachable*:

- Check the server is running: Linux `systemctl status netusb-server`; Windows, Task Scheduler → "netusb server".
- Check the port: the server listens on **TCP 7600**. On a Windows server, VPN networks are often marked *Public*, which the installer's firewall rule does not cover; see [Using a Windows device from Linux](windows-to-linux.md).
   A firewall on the server (`ufw`, `firewalld`, Windows
  Firewall) must allow it from the client. `ping` and then try `nc -vz ADDRESS 7600` from the client.

## "not a paired server" / "no longer authorized"

The client has no trust relationship with that server, or the server removed it. Pair again:
open a pairing window on the server (`sudo netusb-server pair`) and press **Pair…** (or
`netusb-client pair ADDRESS --pin 1234-5678`).

## Pairing fails

- **"wrong PIN"**: PINs are 8 digits (`1234-5678`), work once, and expire after 10 minutes. Five wrong guesses
  close the window; run `netusb-server pair` again.
- **"pairing is not open"**: run `netusb-server pair` on the server. Linux needs `sudo`; Windows needs an
  Administrator PowerShell.
- **"the server did not prove it knows the PIN"**: something between you and the server is interfering. Do
  not continue; pair again from a trusted network.

## "cannot open the device (permission denied)" on Linux

The server could not open the device. The installed service runs as root and can; if you started
`netusb-server` yourself as an ordinary user, run it with `sudo` or install the service.

## "every interface you share must use the WinUSB driver" (Windows server)

Windows is using its own driver for that device, which netusb cannot take over. See
[Using a Windows device from Linux](windows-to-linux.md).

## "could not find the USB/IP importer" (Windows client)

The `usbip-win2` driver is not installed, or `usbip.exe` is not where netusb looks. Install it from
<https://github.com/vadimgrn/usbip-win2/releases>. netusb looks in `%ProgramFiles%\USBip`, on `PATH`, or at
the path in the `NETUSB_USBIP_EXE` environment variable. Attaching also needs Administrator rights.

## "vhci_hcd is not loaded" (Linux client)

Load the kernel module: `sudo modprobe vhci-hcd`. If it does not exist, install your kernel's extra modules
(Ubuntu: `sudo apt install linux-modules-extra-$(uname -r)`). The installer makes it load at boot.

## A device is stuck on the wrong machine

- Press **Release** in the client's page.
- Unplug and replug the device: it should come back as a fresh device on the machine that owns it.
- Stop the server: `sudo systemctl stop netusb-server` (Windows: end the "netusb server" task). It hands every
  device back as it stops.
- If the client machine vanished (sleep, crash, network loss), the server notices in about 25 seconds.

## The web interface does not open

- Linux: `sudo cat /var/lib/netusb/ui.url` prints the address, including a secret token. Open exactly that.
- Windows: **netusb** in the Start Menu, or `netusb-client.exe open`.
- Port 7601 busy: another netusb client interface is already running. Use `netusb-client gui --port 7602`.

## Getting logs

- Linux: `journalctl -u netusb-server` and `journalctl -u netusb-client`.
- Windows: run the program by hand in a PowerShell window to watch its output, for example
  `& "$env:ProgramFiles\netusb\netusb-server.exe" run`.
- For a detailed trace of every USB request, start the server with the environment variable
  `NETUSB_TRACE=1`. It is very verbose and shows device traffic, so share it with care.

Still stuck? Open an issue and include your versions (`netusb-server --version`), both operating systems, the
exact error text, and the log lines around it. Do not post PINs or the contents of the state directory.
