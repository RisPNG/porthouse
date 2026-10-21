# Use a USB device from one Linux machine on another Linux machine

The machine with the device is the **server**; the machine that wants it is the **client**.

**Server** (has the device):

```bash
tar xzf netusb-*-linux-x86_64.tar.gz && cd netusb-*-linux-x86_64
sudo ./install.sh server
sudo netusb-server pair              # prints a one-time PIN
```

Allow TCP port 7600 in your firewall from the client machine only.

**Client** (wants the device):

```bash
sudo ./install.sh client             # also loads the vhci-hcd kernel module it needs
sudo cat /var/lib/netusb/ui.url      # open this address in your browser
```

In the page: **Add a server by address** (or wait for it to appear on the local network), **Pair…** with the PIN,
then **Use** next to the device. **Release** gives it back.

From the command line instead:

```bash
sudo netusb-client pair SERVER_IP --pin 1234-5678
netusb-client list --host SERVER_IP
sudo netusb-client use SERVER_IP 5-1          # 5-1 is the device's id from the list
```

A mouse or keyboard disappears from the server while you use it; see [Share a Linux machine's mouse with
Windows](linux-to-windows.md#getting-it-back) for how to get one back. If something fails, see
[troubleshooting](troubleshooting.md).
