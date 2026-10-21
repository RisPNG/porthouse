//! Identity, trust and TLS plumbing shared by the netusb server and client.
//!
//! Trust model: no certificate authorities. Every peer has a self-signed certificate and trust is
//! decided by pinning its SHA-256 fingerprint. A fingerprint becomes trusted only through
//! pairing, which proves both sides know a short-lived PIN over the very TLS session they are
//! using (channel binding), so a man-in-the-middle cannot relay it.

pub mod identity;
pub mod paths;
pub mod tls;
pub mod trust;
