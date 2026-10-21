# Security policy

## Reporting a vulnerability

Please report security problems privately and not in a public issue. Use GitHub's Security > Report a vulnerability on this repository. Include what you found, how to reproduce it, and what you think the impact is. You will get an acknowledgement, and a disclosure date will be agreed with you once the fix is understood.

## What netusb protects, and what it does not

netusb lets a paired computer take over USB devices on another computer, including keyboards and mice. That is powerful by design, so the model is strict.

- Paired computers are trusted completely. Anyone who is paired can use any shareable device on that server. Only pair computers you control. Revoke one in the program, or with `netusb-server revoke FINGERPRINT`.
- Pairing needs a one-time PIN that is shown on the server's computer. It expires after 10 minutes, dies after 5 wrong guesses, and is bound to the TLS session. A pairing window opens only when someone asks for it.
- Everything is encrypted, including the USB traffic itself. TLS 1.3 is used and both sides authenticate with pinned certificates.
- The server listens on every network interface. Restrict the port (7600) with your firewall to the networks you trust, and do not expose it to the internet.
- The services run as root or SYSTEM, because opening raw USB devices and attaching virtual ones needs it. Their keys and pairings live in `/var/lib/netusb` on Linux and `%ProgramData%\netusb` on Windows, readable only by administrators.
- The program talks to the services through a local socket or named pipe. On Linux, root, the service account, members of the `netusb` group, and users with an active local login session may use it. On Windows, SYSTEM, administrators, and interactive users may. Whoever can use it can pair, attach, and revoke, which is the same trust as sitting at the computer.
- The Windows installer is not code signed yet, so SmartScreen warns. Check the checksum in `SHA256SUMS` against the file you downloaded.

In scope are the pairing and authorization logic, TLS handling, the control channel and who may use it, parsing of network input, and anything that lets an unpaired party reach a device or lets a device be left stranded. Out of scope are attacks that need an already paired computer to be malicious, physical access, and the security of the third-party usbip-win2 driver and of Windows itself.

## Supported versions

The latest release.
