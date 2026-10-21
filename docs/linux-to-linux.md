# Use a USB device from one Linux computer on another

The computer with the device shares it. The computer that wants it uses it.

On the computer with the device, install the package for your system (`sudo apt install ./netusb-*.deb` or `sudo dnf install ./netusb-*.rpm`, or build the Arch package from `packaging/arch`). Open netusb, go to Share devices, and press Allow a new computer to pair. Allow TCP port 7600 in your firewall from the other computer only.

On the computer that wants the device, install the same package. The client service loads the `vhci-hcd` kernel module that it needs. Open netusb. Add the first computer by address if it does not appear by itself, press Pair and enter the PIN, then press Use next to the device. Release gives it back.

The same steps work from a terminal, which is useful for servers without a screen:

```bash
netusb-server pair                          # on the computer with the device: prints a one-time PIN
netusb-client pair SERVER_IP --pin 1234-5678
netusb-client list                          # shows servers, their devices, and what is in use here
netusb-client use SERVER_IP 5-1             # 5-1 is the device's id from the list
netusb-client release SERVER_IP 5-1
```

If your account cannot reach the services, add it to the `netusb` group with `sudo usermod -aG netusb "$USER"` and sign in again.

A mouse or keyboard disappears from the sharing computer while you use it. [Sharing a Linux mouse with Windows](linux-to-windows.md#getting-it-back) explains how to get one back. If something fails, see [troubleshooting](troubleshooting.md).
