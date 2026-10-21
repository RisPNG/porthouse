netusb for Windows (x64)
========================

TESTED: these exact programs were built and tested on GitHub's Windows Server 2022 machines
(real Windows, real drivers, an interactive desktop). In that environment they:
  - paired and listed over TLS;
  - attached a virtual USB mouse through the real usbip-win2 driver, which Windows enumerated as a
    HID mouse, and the pointer moved (and stayed still before attach and after release);
  - shared a Windows WinUSB device through the native server (control, bulk, cancel, ordering);
  - installed, ran as scheduled tasks, and uninstalled cleanly with the PowerShell scripts below.

NOT TESTED: physical USB hardware, desktop editions (Windows 10/11; the tests ran on Server 2022),
ARM64, and composite devices. Please report exactly what you see if something differs.

What is in this folder
  netusb-client.exe   use USB devices shared by other machines (command line + web interface)
  netusb-server.exe   share this machine's USB devices
  install-windows.ps1 / uninstall-windows.ps1
  No Visual C++ runtime is needed; the programs depend only on Windows itself.

USING DEVICES FROM ANOTHER MACHINE (client)
  1. Install the virtual USB driver (usbip-win2) from
       https://github.com/vadimgrn/usbip-win2/releases
     Download the x64 installer and run it. No reboot was needed in testing. Its drivers are signed
     by Microsoft; do not change any Windows signing setting for it. (Version tested: 0.9.8.1.)
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
  The server runs as a scheduled task as SYSTEM; its keys and pairings live in
  %ProgramData%\netusb (administrators only).

NOT SUPPORTED YET: webcams and USB audio (isochronous transfers), and changing a device's
configuration from the client.
