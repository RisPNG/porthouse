# Troubleshooting

## The program says a service is not running

netusb shows a banner when it cannot reach the server service or the client service. Start the service and the page fills in by itself.

- Linux: `sudo systemctl start netusb-server netusb-client`. Check with `systemctl status netusb-server netusb-client`.
- Windows: open Services and start "netusb server" or "netusb client", or run `Start-Service netusb-server, netusb-client` in an Administrator PowerShell.

If the banner stays, your account may not be allowed to use the service. On Linux, add yourself to the `netusb` group with `sudo usermod -aG netusb "$USER"` and sign in again. The command line says so too: "not allowed to use this netusb service".

## The server does not appear in the client

Automatic discovery only works on the same local network. Over a VPN, or across subnets, type the server's address into the box at the bottom of Use devices, or run `netusb-client add ADDRESS`.

If a server shows "unreachable" after you add it:

- Check that the server service is running, as above.
- Check the port. The server listens on TCP 7600. Any firewall on the server (`ufw`, `firewalld`, Windows Firewall) has to allow it from the client. On a Windows server, VPN networks are often marked Public, which the installer's firewall rule does not cover, see [using a Windows device from Linux](windows-to-linux.md). Try `nc -vz ADDRESS 7600` from the client.

## "not a paired server" or "not paired"

The client has no trust relationship with that server, or the server removed it. Pair again. Open a pairing window on the server (Share devices > Allow a new computer to pair, or `netusb-server pair`) and press Pair next to the server, or run `netusb-client pair ADDRESS --pin 1234-5678`.

## Pairing fails

- "wrong PIN": PINs have 8 digits (`1234-5678`), work once, and expire after 10 minutes. Five wrong guesses close the window, so open a new one.
- "pairing is not open": open a pairing window on the server first. Nothing opens one by itself.
- "the server did not prove it knows the PIN": something between the two computers is interfering. Do not continue, and pair again from a trusted network.

## "cannot open the device (permission denied)" on Linux

The server could not open the device. The installed service runs as root and can. If you started `netusb-server` yourself as an ordinary user, run it with `sudo` or use the service.

## "every interface you share must use the WinUSB driver" on a Windows server

Windows is using its own driver for that device, which netusb cannot take over. See [using a Windows device from Linux](windows-to-linux.md).

## "could not find the USB/IP importer" on a Windows client

The usbip-win2 driver is not installed, or `usbip.exe` is not where netusb looks. Run the netusb installer again with "Use devices from other computers" selected, or install the driver from <https://github.com/vadimgrn/usbip-win2/releases>. netusb looks in `%ProgramFiles%\USBip`, on `PATH`, and at the path in the `NETUSB_USBIP_EXE` environment variable of the service.

## "vhci_hcd is not loaded" on a Linux client

Load the kernel module with `sudo modprobe vhci-hcd`. If it does not exist, install your kernel's extra modules (Ubuntu: `sudo apt install linux-modules-extra-$(uname -r)`). The package makes the module load at boot.

## A device is stuck on the wrong computer

- Press Release in the client's program.
- Unplug and replug the device. It comes back as a fresh device on the computer that owns it.
- Stop the server service: `sudo systemctl stop netusb-server` on Linux, or stop "netusb server" in Services on Windows. It hands every device back as it stops.
- If the client computer vanished through sleep, a crash, or network loss, the server notices in about 25 seconds.

## The notification-area icon is missing

netusb needs a notification area. KDE, Xfce, Cinnamon, MATE, and Windows have one. GNOME needs the AppIndicator extension. Without an icon the window still works, and closing it quits the program.

## Getting logs

- Linux: `journalctl -u netusb-server` and `journalctl -u netusb-client`.
- Windows: the services write to `%ProgramData%\netusb\server.log` and `client.log`. To watch a service live, stop it and run `netusb-server.exe run` or `netusb-clientd.exe run` in an Administrator PowerShell.
- For a detailed trace of every USB request, start the server with the environment variable `NETUSB_TRACE=1`. It is very verbose and shows device traffic, so share it with care.

Still stuck? Open an issue and include the versions (`netusb-server --version`), both operating systems, the exact error text, and the log lines around it. Do not post PINs or the contents of the state directory.
