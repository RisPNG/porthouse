# Use a device plugged into a Windows PC from Linux

The Windows PC is the **server**; the Linux machine is the **client**.

## 1. On the Windows PC: share the device

The Windows server can only share a device that uses the **WinUSB** driver. Most devices use another driver
(Windows' own mouse, keyboard, audio or storage drivers), and netusb will tell you so: *"every interface you
share must use the WinUSB driver"*.

To share a device you must give it the WinUSB driver, for example with [Zadig](https://zadig.akeo.ie):

> **Warning:** this *replaces* the driver Windows normally uses for that device, so the device **stops working
> on the Windows PC** while it has WinUSB (and while it is shared). Do this only for a device you can spare, and
> never for the mouse or keyboard you are using on that PC. Undo it later by uninstalling the device's driver in
> Device Manager, then unplugging and replugging it.

Then, in PowerShell as Administrator in the unzipped Windows folder:

```powershell
powershell -ExecutionPolicy Bypass -File .\install-windows.ps1 -Role server
& "$env:ProgramFiles\netusb\netusb-server.exe" pair      # prints a one-time PIN
```

Windows Firewall: the installer opens TCP port 7600 for **private and domain** networks only. VPN adapters such as
ZeroTier are often classified as **Public** by Windows, in which case the client cannot connect. Either set that
network to Private (Settings → Network & internet → the network → Network profile type), or add a rule for it
yourself, limited to the addresses you trust:

```powershell
New-NetFirewallRule -DisplayName "netusb server (VPN)" -Direction Inbound -Protocol TCP -LocalPort 7600 -Action Allow -Profile Public -RemoteAddress 10.0.0.0/24
```

## 2. On the Linux machine: use it

```bash
sudo ./install.sh client                 # from the extracted Linux archive
sudo cat /var/lib/netusb/ui.url          # open this address in your browser
```

**Add a server by address** (the Windows PC's IP), **Pair…** with the PIN, then **Use** next to the device.

Composite devices (one device with several functions) are the least likely to work on a Windows server. If
you hit a problem, see [troubleshooting](troubleshooting.md).
