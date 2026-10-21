use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::identity::write_private;

pub const PIN_LIFETIME_SECS: u64 = 600;
pub const MAX_PIN_FAILURES: u32 = 5;

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Which side is proving, so a proof can never be replayed in the other direction.
#[derive(Clone, Copy)]
pub enum Role {
    Client,
    Server,
}

/// HMAC-SHA256(PIN; session binding || role || own certificate fingerprint).
pub fn proof(pin: &str, binding: &[u8; 32], role: Role, own_fingerprint: &str) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(pin.as_bytes()).expect("HMAC accepts any key length");
    mac.update(binding);
    mac.update(match role {
        Role::Client => b"client",
        Role::Server => b"server",
    });
    mac.update(own_fingerprint.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

pub fn verify(pin: &str, binding: &[u8; 32], role: Role, fingerprint: &str, given: &[u8]) -> bool {
    let expected = proof(pin, binding, role, fingerprint);
    expected.ct_eq(given).into()
}

pub fn generate_pin() -> Result<String> {
    loop {
        let mut b = [0u8; 4];
        getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("random source failed: {e}"))?;
        let n = u32::from_le_bytes(b);
        if n < (u32::MAX / 100_000_000) * 100_000_000 {
            let n = n % 100_000_000;
            return Ok(format!("{:04}-{:04}", n / 10_000, n % 10_000));
        }
    }
}

pub fn normalize_pin(s: &str) -> Option<String> {
    let d: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    (d.len() == 8).then(|| format!("{}-{}", &d[..4], &d[4..]))
}

pub struct PinFile(PathBuf);

impl PinFile {
    pub fn new(dir: &Path) -> Self {
        PinFile(dir.join("pairing.pin"))
    }

    pub fn open_window(&self) -> Result<(String, u64)> {
        let pin = generate_pin()?;
        let expires = now() + PIN_LIFETIME_SECS;
        write_private(&self.0, format!("{pin}\n{expires}\n0\n").as_bytes())?;
        Ok((pin, expires))
    }

    pub fn active(&self) -> Option<String> {
        self.window().map(|(pin, _)| pin)
    }

    pub fn window(&self) -> Option<(String, u64)> {
        let (pin, expires, fails) = self.read()?;
        if now() >= expires || fails >= MAX_PIN_FAILURES {
            let _ = std::fs::remove_file(&self.0);
            return None;
        }
        Some((pin, expires))
    }

    pub fn record_failure(&self) {
        if let Some((pin, expires, fails)) = self.read() {
            if fails + 1 >= MAX_PIN_FAILURES {
                let _ = std::fs::remove_file(&self.0);
            } else {
                let _ = write_private(&self.0, format!("{pin}\n{expires}\n{}\n", fails + 1).as_bytes());
            }
        }
    }

    pub fn consume(&self) {
        let _ = std::fs::remove_file(&self.0);
    }

    fn read(&self) -> Option<(String, u64, u32)> {
        let t = std::fs::read_to_string(&self.0).ok()?;
        let mut l = t.lines();
        Some((l.next()?.to_string(), l.next()?.parse().ok()?, l.next()?.parse().ok()?))
    }
}

pub struct TrustStore(PathBuf);

impl TrustStore {
    pub fn new(dir: &Path, file: &str) -> Self {
        TrustStore(dir.join(file))
    }

    pub fn entries(&self) -> Vec<(String, String)> {
        std::fs::read_to_string(&self.0)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_once('\t').map(|(f, n)| (f.to_string(), n.to_string())))
            .collect()
    }

    pub fn contains(&self, fingerprint: &str) -> bool {
        self.entries().iter().any(|(f, _)| f == fingerprint)
    }

    pub fn name_of(&self, fingerprint: &str) -> Option<String> {
        self.entries().into_iter().find(|(f, _)| f == fingerprint).map(|(_, n)| n)
    }

    pub fn add(&self, fingerprint: &str, name: &str) -> Result<()> {
        let mut all: Vec<(String, String)> = self.entries().into_iter().filter(|(f, _)| f != fingerprint).collect();
        let name: String = name.chars().filter(|c| !c.is_control()).collect();
        all.push((fingerprint.to_string(), name));
        let text: String = all.iter().map(|(f, n)| format!("{f}\t{n}\n")).collect();
        write_private(&self.0, text.as_bytes()).with_context(|| format!("writing {}", self.0.display()))
    }

    pub fn remove(&self, fingerprint: &str) -> Result<bool> {
        let before = self.entries();
        let after: Vec<_> = before.iter().filter(|(f, _)| f != fingerprint).collect();
        if after.len() == before.len() {
            return Ok(false);
        }
        let text: String = after.iter().map(|(f, n)| format!("{f}\t{n}\n")).collect();
        write_private(&self.0, text.as_bytes())?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("netusb-trust-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn proof_verifies_only_with_the_same_pin_session_role_and_fingerprint() {
        let b = [7u8; 32];
        let p = proof("1234-5678", &b, Role::Client, "aa");
        assert!(verify("1234-5678", &b, Role::Client, "aa", &p));
        assert!(!verify("1234-5679", &b, Role::Client, "aa", &p), "wrong PIN");
        assert!(
            !verify("1234-5678", &[8u8; 32], Role::Client, "aa", &p),
            "different TLS session (relay)"
        );
        assert!(!verify("1234-5678", &b, Role::Server, "aa", &p), "reflected in the other direction");
        assert!(!verify("1234-5678", &b, Role::Client, "bb", &p), "different certificate");
        assert!(!verify("1234-5678", &b, Role::Client, "aa", &p[..16]), "truncated proof");
    }

    #[test]
    fn pins_are_well_formed_and_vary() {
        let a = generate_pin().unwrap();
        assert_eq!(a.len(), 9);
        assert_eq!(&a[4..5], "-");
        assert!(a.chars().filter(|c| *c != '-').all(|c| c.is_ascii_digit()));
        let many: std::collections::HashSet<_> = (0..50).map(|_| generate_pin().unwrap()).collect();
        assert!(many.len() > 40);
    }

    #[test]
    fn user_typed_pins_are_normalised() {
        assert_eq!(normalize_pin("12345678").as_deref(), Some("1234-5678"));
        assert_eq!(normalize_pin(" 1234 5678\n").as_deref(), Some("1234-5678"));
        assert_eq!(normalize_pin("1234-567"), None);
        assert_eq!(normalize_pin("abcd-efgh"), None);
    }

    #[test]
    fn pin_window_expires_after_too_many_failures_and_is_single_use() {
        let d = tmp("pin");
        let w = PinFile::new(&d);
        assert!(w.active().is_none());
        let (pin, _) = w.open_window().unwrap();
        assert_eq!(w.active().as_deref(), Some(pin.as_str()));
        for _ in 0..MAX_PIN_FAILURES - 1 {
            w.record_failure();
            assert!(w.active().is_some());
        }
        w.record_failure();
        assert!(w.active().is_none(), "window must close after {MAX_PIN_FAILURES} failures");
        let (_, _) = w.open_window().unwrap();
        w.consume();
        assert!(w.active().is_none(), "PIN is single-use");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn trust_store_adds_replaces_and_removes() {
        let d = tmp("store");
        let s = TrustStore::new(&d, "clients");
        assert!(!s.contains("ab"));
        s.add("ab", "laptop").unwrap();
        s.add("cd", "phone\nINJECTED\tline").unwrap();
        assert!(s.contains("ab"));
        assert_eq!(s.entries().len(), 2, "a newline in a name must not forge an entry");
        s.add("ab", "renamed").unwrap();
        assert_eq!(s.name_of("ab").as_deref(), Some("renamed"));
        assert_eq!(s.entries().len(), 2);
        assert!(s.remove("ab").unwrap());
        assert!(!s.remove("ab").unwrap());
        let _ = std::fs::remove_dir_all(&d);
    }
}
