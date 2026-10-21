# Share a Linux machine's mouse (or keyboard) with a Windows PC

You have a mouse plugged into a **Linux** machine and you want to use it on a **Windows** PC, over the
network. The Linux machine is the **server** (it has the device). The Windows PC is the **client** (it wants it).

> **The mouse moves, it is not copied.** While Windows is using it, the Linux machine has no mouse. Before you
> start, make sure you can still control Linux: a second mouse or touchpad, or at least a keyboard with a terminal
> open. See [Getting it back](#getting-it-back) now, so you know how.

## What you need

- The Linux machine and the Windows PC can reach each other over a network: the same home network, or a VPN
  such as ZeroTier or Tailscale.
- On Linux: root access (`sudo`).
- On Windows: Administrator rights, and the [usbip-win2](https://github.com/vadimgrn/usbip-win2/releases)
  driver (step 2).
- The Linux machine's IP address as the Windows PC sees it. On Linux, `ip -br addr` lists them; with a VPN, use
  the VPN address.

## 1. On the Linux machine (the one with the mouse)

Download the Linux archive from the **Releases** page, then:

```bash
tar xzf netusb-*-linux-x86_64.tar.gz && cd netusb-*-linux-x86_64
sha256sum -c SHA256SUMS
sudo ./install.sh server
```

This starts the server as a service. It listens on TCP port **7600**. If you run a firewall, allow that port
**only from the Windows PC or your trusted network**, for example:

```bash
sudo ufw allow from 192.168.1.0/24 to any port 7600      # use your own network
```

Now open a pairing window and keep the PIN it prints (it is good for 10 minutes and works once):

```bash
sudo netusb-server pair
```

## 2. On the Windows PC (the one that will use the mouse)

1. Install the **usbip-win2** driver: download the x64 installer from
   <https://github.com/vadimgrn/usbip-win2/releases> and run it. Create a restore point first. It installs a
   virtual USB controller; no reboot was needed in testing.
2. Download the Windows zip from the **Releases** page and unzip it.
3. Open **PowerShell as Administrator** in the unzipped folder and run:

```powershell
powershell -ExecutionPolicy Bypass -File .\install-windows.ps1 -Role client
```

4. Open **netusb** from the Start Menu. Your browser opens the netusb page.
5. Under **Servers**, type the Linux machine's address (for example `192.168.1.20`) in **Add a server by
   address** and press **Add**. (Servers on the same local network may appear by themselves; over a VPN you add
   them by address.)
6. Press **Pair…** next to the server, type the PIN from step 1, and press **Pair**. The server now shows
   **paired** and lists its USB devices.

## 3. Move the mouse

Find your mouse in the list. It is marked **input device**. Press **Use** and confirm the warning.

For a moment the Linux pointer freezes. Then the mouse controls the Windows PC. It is listed under
**In use on this computer**.

## Getting it back

Any of these returns the mouse to Linux:

- Press **Release** next to it in the netusb page on Windows. (This is the clean way.)
- **Unplug and replug** the mouse. It should come back as a fresh device that Linux picks up normally.
- From a Linux terminal: `sudo systemctl stop netusb-server` (the server hands every device back as it stops).
- If the Windows PC goes to sleep, crashes or loses the network, the server notices within about **25 seconds**
  and gives the mouse back by itself.

## Good to know

- Pointer speed, acceleration and button mapping now come from **Windows** settings and any vendor software
  installed there, because Windows sees the mouse as if it were plugged in.
- It works for keyboards the same way. A keyboard you are typing on disappears from Linux while Windows uses it,
  so have another one, or release it with a mouse click on the Windows page first.
- To share a different device, repeat step 3 for it. Only one machine can use a device at a time.
- To stop sharing permanently: `sudo ./install.sh uninstall` on Linux; `uninstall-windows.ps1` on Windows.

If something does not work, see [troubleshooting](troubleshooting.md).
