# Share a Linux computer's mouse or keyboard with a Windows PC

You have a mouse plugged into a Linux computer and you want to use it on a Windows PC over the network. The Linux computer shares the device. The Windows PC uses it.

The mouse moves, it is not copied. While Windows is using it, the Linux computer has no mouse. Before you start, make sure you can still control Linux, with a second mouse, a touchpad, or at least a keyboard. Read [getting it back](#getting-it-back) first so you know how.

## What you need

- The Linux computer and the Windows PC can reach each other over a network. That can be the same home network or a VPN such as ZeroTier or Tailscale.
- On Linux, a package from the Releases page that matches your system, and `sudo`.
- On Windows, Administrator rights to run the installer.
- The Linux computer's IP address as the Windows PC sees it. On Linux, `ip -br addr` lists them. With a VPN, use the VPN address.

## 1. On the Linux computer, the one with the mouse

Install the package, for example `sudo apt install ./porthouse-ubuntu-24.04-*.deb` or `sudo dnf install ./porthouse-fedora-43-*.rpm`. The services start by themselves and Porthouse appears in the application menu.

The server listens on TCP port 7600. If you run a firewall, allow that port only from the Windows PC or from your trusted network, for example:

```bash
sudo ufw allow from 192.168.1.0/24 to any port 7600
```

Open Porthouse, go to Share devices, and press Allow a new computer to pair. A PIN appears. It is good for 10 minutes and works once. Leave the window open.

## 2. On the Windows PC, the one that will use the mouse

1. Run the installer from the Releases page. Keep "Use devices from other computers" selected. It installs the usbip-win2 driver, which adds a virtual USB controller, and starts the client service. Windows SmartScreen may ask for confirmation because the installer is not code signed yet.
2. Open Porthouse from the Start Menu.
3. On Use devices, type the Linux computer's address (for example `192.168.1.20`) into the box at the bottom and press Add. Computers on the same local network may appear by themselves, but over a VPN you add them by address.
4. Press Pair next to the server, type the PIN from step 1, and press Pair. The server now shows "paired" and lists its USB devices.

## 3. Move the mouse

Find the mouse in the list. It is marked "input device". Press Use and confirm the question. You can tick "Do not ask again" to skip it from then on.

For a moment the Linux pointer freezes. Then the mouse controls the Windows PC. It is listed under "In use on this computer".

## Getting it back

Any of these returns the mouse to Linux:

- Press Release next to it in Porthouse on Windows. This is the clean way.
- Unplug and replug the mouse. It comes back as a fresh device that Linux picks up normally.
- From a Linux terminal, run `sudo systemctl stop porthouse-server`. The server hands every device back as it stops.
- If the Windows PC goes to sleep, crashes, or loses the network, the server notices within about 25 seconds and gives the mouse back by itself.

## Good to know

- Pointer speed, acceleration, and button mapping now come from the Windows settings and any vendor software installed there, because Windows sees the mouse as if it were plugged in.
- Keyboards work the same way. A keyboard that you are typing on disappears from Linux while Windows uses it, so have another one, or release it with the mouse first.
- Only one computer can use a device at a time. To share a different device, repeat step 3 for it.
- To stop sharing for good, remove the package on Linux (`sudo apt remove porthouse` or `sudo dnf remove porthouse`) and uninstall Porthouse from Windows Settings.

If something does not work, see [troubleshooting](troubleshooting.md).
