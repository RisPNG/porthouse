# Security policy

## Reporting a vulnerability

Please report security problems privately, not in a public issue: use GitHub's
**Security → Report a vulnerability** on this repository. Include what you found, how to reproduce it,
and what you think the impact is. You will get an acknowledgement, and we will agree a disclosure date
with you once the fix is understood.

## What netusb protects, and what it does not

netusb lets a paired machine take over USB devices on another machine, including keyboards and
mice. That is powerful by design, so the model is strict:

- **Paired machines are trusted completely.** Anyone who is paired can use any shareable device on that
  server. Only pair machines you control. Revoke one with `sudo netusb-server revoke <fingerprint>`.
- **Pairing needs a one-time PIN** that is shown on the server's machine, expires after 10 minutes, dies
  after 5 wrong guesses, and is bound to the TLS session.
- **Everything is encrypted** (TLS 1.3, both sides authenticate with pinned certificates), including the
  USB traffic itself.
- **The server listens on every network interface.** Restrict the port (7600) with your firewall to the
  networks you trust. Do not expose it to the internet.
- **The server runs as root / SYSTEM** because opening raw USB devices needs it. Its keys and pairings
  live in `/var/lib/netusb` (Linux) or `%ProgramData%\netusb` (Windows), readable only by administrators.
- **The client's web interface** listens on `127.0.0.1` only and needs a secret token that is stored in
  an owner-only file.

In scope: the pairing and authorization logic, TLS handling, the web interface, parsing of network input,
and anything that lets an unpaired party reach a device or lets a device be left stranded.
Out of scope: attacks that need an already-paired machine to be malicious, physical access, and the
security of the third-party `usbip-win2` driver and of Windows itself.

## Supported versions

The latest release.
