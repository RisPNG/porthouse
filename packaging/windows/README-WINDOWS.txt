netusb for Windows (x64)
========================

STATUS: these programs were cross-compiled from Linux and have NOT been run on Windows yet.
Expect to find problems. Please report exactly what you see.

What is in this folder
  netusb-client.exe   use USB devices shared by other machines (command line + web interface)
  netusb-server.exe   share this machine's USB devices
  install-windows.ps1 / uninstall-windows.ps1

USING DEVICES FROM ANOTHER MACHINE (client)
  1. Install the virtual USB driver (usbip-win2) from
       https://github.com/vadimgrn/usbip-win2/releases
     Download the x64 installer, run it, reboot if asked. Its drivers are signed; if Windows
     complains about signatures, see that project's page before changing any Windows setting.
  2. From an elevated PowerShell in this folder:
       powershell -ExecutionPolicy Bypass -File .\install-windows.ps1 -Role client
  3. Open "netusb" from the Start Menu. On the server machine run `netusb-server pair`, press
     "Pair..." in the page, enter the PIN, then press "Use" next to a device.

  Without the installer you can run it by hand from an elevated prompt:
       netusb-client.exe gui          (prints the address to open)
       netusb-client.exe list
       netusb-client.exe pair HOST    (PIN from the server)
       netusb-client.exe use HOST BUSID

  The importer is found at %ProgramFiles%\USBip\usbip.exe, on PATH, or at NETUSB_USBIP_EXE.
  Attaching a device needs administrator rights.

SHARING DEVICES FROM THIS MACHINE (server)
  The server uses the WinUSB driver. A device can only be shared if WinUSB is its driver.
  Replace a device's driver with WinUSB using a tool such as Zadig (https://zadig.akeo.ie), which
  makes it unusable by its normal Windows driver while shared. There is no automatic driver install.
  Composite devices (several interfaces) are the least likely to work.
       powershell -ExecutionPolicy Bypass -File .\install-windows.ps1 -Role server
       & "$env:ProgramFiles\netusb\netusb-server.exe" pair

NOT SUPPORTED YET: webcams and USB audio (isochronous transfers), and changing a device's
configuration from the client.

Runtime: Windows 10 or later (uses the Universal C Runtime). No other DLLs are required.
