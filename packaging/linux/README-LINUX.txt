netusb for Linux (x86_64, static binaries)
==========================================

  netusb-server   share this machine's USB devices
  netusb-client   use devices shared by other machines (command line + web interface)
  install.sh      installs them as systemd services

Share devices from this machine:
    sudo ./install.sh server
    sudo netusb-server pair            # prints a one-time PIN to give to the other machine

Use devices from another machine:
    sudo ./install.sh client
    sudo cat /var/lib/netusb/ui.url    # open that address in a browser, pair, press "Use"

Remove it:
    sudo ./install.sh uninstall        (add --purge to delete keys and pairings too)

Full documentation: the README and docs/ in the source repository.
Check the download first:  sha256sum -c SHA256SUMS
