# Checking your download

Every release has a file called `SHA256SUMS` that lists the checksum of each download. Compare it with what you downloaded:

```bash
sha256sum -c SHA256SUMS --ignore-missing
```

On Windows, `Get-FileHash FILE` prints the checksum to compare by eye.

## Linux downloads

Releases made with a signing key also have a `.asc` file next to every download and next to `SHA256SUMS`, and the `.rpm` packages are signed inside as well. To check one:

```bash
curl -fsSLO https://raw.githubusercontent.com/RisPNG/porthouse/main/porthouse-signing.asc
gpg --import porthouse-signing.asc
gpg --verify porthouse-fedora-43-0.3.0-x86_64.rpm.asc porthouse-fedora-43-0.3.0-x86_64.rpm
```

If a release has no `.asc` files, it was published before the signing key existed.

## The Windows installer

Open the installer's properties, then Digital Signatures. A signed installer names SignPath Foundation as the publisher. Windows also checks this itself, and SmartScreen stays quiet about signed installers once they have built up some reputation. An installer with no signature tab has not been signed yet, and Windows will warn about it.
