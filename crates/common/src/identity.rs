//! Long-term self-signed identity of a server or client.

use std::path::Path;

use anyhow::{Context, Result};
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};

pub struct Identity {
    pub chain: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
    /// Lowercase hex SHA-256 of the leaf certificate (DER).
    pub fingerprint: String,
}

impl Identity {
    pub fn clone_parts(&self) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
        (self.chain.clone(), self.key.clone_key())
    }
}

pub fn fingerprint(cert: &CertificateDer<'_>) -> String {
    hex(&Sha256::digest(cert.as_ref()))
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Load `<dir>/<name>.pem`, creating a fresh identity the first time.
pub fn load_or_create(dir: &Path, name: &str) -> Result<Identity> {
    let path = dir.join(format!("{name}.pem"));
    if !path.exists() {
        let ck = rcgen::generate_simple_self_signed(vec!["netusb".to_string()]).context("generating certificate")?;
        let pem = format!("{}{}", ck.cert.pem(), ck.signing_key.serialize_pem());
        write_private(&path, pem.as_bytes())?;
    }
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    parse(&bytes).with_context(|| format!("parsing {}", path.display()))
}

pub fn parse(pem: &[u8]) -> Result<Identity> {
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(pem)
        .collect::<Result<_, _>>()
        .context("reading certificate")?;
    let key = PrivateKeyDer::from_pem_slice(pem).context("reading private key")?;
    let leaf = chain.first().context("no certificate in identity file")?;
    let fingerprint = fingerprint(leaf);
    Ok(Identity { chain, key, fingerprint })
}

/// Write a secret file readable only by the owner.
pub fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(data)?;
        give_to_sudo_user(path);
    }
    #[cfg(not(unix))]
    std::fs::write(path, data)?;
    Ok(())
}

/// When running under sudo, hand a file we just created back to the invoking user, so a later
/// unprivileged run can still read it.
#[cfg(unix)]
pub fn give_to_sudo_user(path: &Path) {
    let id = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<u32>().ok());
    if let (Some(uid), Some(gid)) = (id("SUDO_UID"), id("SUDO_GID")) {
        let _ = std::os::unix::fs::chown(path, Some(uid), Some(gid));
    }
}

#[cfg(not(unix))]
pub fn give_to_sudo_user(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_created_once_and_reloaded_identically() {
        let dir = std::env::temp_dir().join(format!("netusb-id-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = load_or_create(&dir, "t").unwrap();
        let b = load_or_create(&dir, "t").unwrap();
        assert_eq!(a.fingerprint, b.fingerprint);
        assert_eq!(a.fingerprint.len(), 64);
        let c = load_or_create(&dir, "other").unwrap();
        assert_ne!(a.fingerprint, c.fingerprint);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("t.pem")).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "identity file must not be group/world accessible");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
