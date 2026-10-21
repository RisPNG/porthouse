use std::path::Path;

use anyhow::{Context, Result};
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};

pub struct Identity {
    pub chain: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
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

pub fn load_or_create(dir: &Path, name: &str) -> Result<Identity> {
    let path = dir.join(format!("{name}.pem"));
    if !path.exists() {
        let ck = rcgen::generate_simple_self_signed(vec!["porthouse".to_string()]).context("generating certificate")?;
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
    }
    #[cfg(not(unix))]
    std::fs::write(path, data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_created_once_and_reloaded_identically() {
        let dir = std::env::temp_dir().join(format!("porthouse-id-test-{}", std::process::id()));
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
