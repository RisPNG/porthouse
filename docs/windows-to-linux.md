# Use a device plugged into a Windows PC from Linux

The Windows PC shares the device. The Linux computer uses it.

## 1. On the Windows PC, share the device

The Windows server can only share a device that uses the WinUSB driver. Most devices use another driver, such as the Windows mouse, keyboard, audio, or storage drivers, and Porthouse says so with the message "every interface you share must use the WinUSB driver".

To share a device you have to give it the WinUSB driver, for example with [Zadig](https://zadig.akeo.ie). This replaces the driver that Windows normally uses for that device, so the device stops working on the Windows PC while it has WinUSB. Do this only for a device you can spare, and never for the mouse or keyboard that you use on that PC. To undo it, uninstall the device's driver in Device Manager and then unplug and replug the device.

Run the installer from the Releases page and keep "Share this computer's devices" selected. It opens TCP port 7600 for private and domain networks and starts the server service. Then open Porthouse, go to Share devices, and press Allow a new computer to pair.

VPN adapters such as ZeroTier are often classified as Public by Windows, so the rule does not cover them and the Linux computer cannot connect. Either set that network to Private (Settings > Network and internet > the network > Network profile type), or add a rule for it yourself, limited to the addresses you trust:

```powershell
New-NetFirewallRule -DisplayName "porthouse server (VPN)" -Direction Inbound -Protocol TCP -LocalPort 7600 -Action Allow -Profile Public -RemoteAddress 10.0.0.0/24
```

## 2. On the Linux computer, use it

Install the package for your system, open Porthouse, add the Windows PC by address (its IP), press Pair with the PIN, and press Use next to the device.

Composite devices, which have several functions in one device, are the least likely to work on a Windows server. If you hit a problem, see [troubleshooting](troubleshooting.md).
