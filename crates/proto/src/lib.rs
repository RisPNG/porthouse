//! netusb control-plane protocol.
//!
//! Frame layout (all integers little-endian):
//!
//! ```text
//! u32 body_len | u8 flags | body[body_len]
//! ```
//!
//! `flags & FLAG_LZ4` means `body` is an LZ4 block with a prepended `u32` uncompressed size.
//! The body of every frame is one `postcard`-encoded message.

use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub mod usbip;

pub const PROTO_VERSION: u16 = 4;
pub const DEFAULT_PORT: u16 = 7600;
pub const MDNS_SERVICE: &str = "_netusb._tcp.local.";

/// Hard cap on a single frame body, compressed or not. Guards against hostile length prefixes.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;
/// Bodies at least this long are tried for compression.
pub const COMPRESS_THRESHOLD: usize = 512;

const FLAG_LZ4: u8 = 0x01;
const HEADER_LEN: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("frame of {0} bytes exceeds the {MAX_FRAME} byte limit")]
    FrameTooLarge(usize),
    #[error("unknown frame flags {0:#04x}")]
    BadFlags(u8),
    #[error("corrupt compressed frame")]
    BadCompression,
    #[error("message decode failed: {0}")]
    Decode(#[from] postcard::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Stable per-server identifier of the physical port, e.g. "1-2.3".
    pub busid: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub class: u8,
    pub speed: Speed,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    /// Name of the client currently using the device, if any.
    pub in_use_by: Option<String>,
    /// A mouse, keyboard or other human-interface device. The machine sharing it loses it while
    /// it is attached elsewhere, so clients should warn before taking one.
    pub input: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Speed {
    Unknown,
    Low,
    Full,
    High,
    Super,
    SuperPlus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello {
        proto: u16,
        client_name: String,
    },
    ListDevices,
    Ping(u64),
    /// Claim `busid` for this connection. On success the server replies `Attached` and the
    /// connection then carries raw USB/IP URB traffic instead of netusb frames.
    Attach {
        busid: String,
    },
    /// Prove knowledge of the pairing PIN, bound to this TLS session. Only valid before the
    /// client is authorized.
    Pair {
        proof: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMsg {
    Hello {
        proto: u16,
        server_id: String,
        server_name: String,
        version: String,
        authorized: bool,
    },
    Devices(Vec<DeviceInfo>),
    Pong(u64),
    Attached(AttachInfo),
    /// Pairing succeeded; `proof` shows the server also knows the PIN.
    Paired {
        proof: Vec<u8>,
    },
    Error {
        code: u16,
        message: String,
    },
}

/// What the client needs to create the virtual device (the USB/IP "import" reply).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachInfo {
    pub busid: String,
    pub busnum: u32,
    pub devnum: u32,
    /// Linux `enum usb_device_speed` value: 1 low, 2 full, 3 high, 5 super.
    pub speed: u32,
    pub vendor_id: u16,
    pub product_id: u16,
    pub bcd_device: u16,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    /// Best-effort values for importers that report them (Linux `vhci_hcd` ignores them).
    pub configuration_value: u8,
    pub num_configurations: u8,
    pub num_interfaces: u8,
}

impl AttachInfo {
    /// The 32-bit device id used in every URB header.
    pub fn devid(&self) -> u32 {
        (self.busnum << 16) | self.devnum
    }
}

/// Encode one message as a complete frame.
pub fn encode_frame<T: Serialize>(msg: &T) -> Result<Vec<u8>, ProtoError> {
    let raw = postcard::to_stdvec(msg)?;
    let (flags, body) = if raw.len() >= COMPRESS_THRESHOLD {
        let packed = lz4_flex::compress_prepend_size(&raw);
        if packed.len() < raw.len() {
            (FLAG_LZ4, packed)
        } else {
            (0, raw)
        }
    } else {
        (0, raw)
    };
    if body.len() > MAX_FRAME {
        return Err(ProtoError::FrameTooLarge(body.len()));
    }
    let mut out = Vec::with_capacity(HEADER_LEN + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.push(flags);
    out.extend_from_slice(&body);
    Ok(out)
}

/// Try to decode one frame from the front of `buf`.
///
/// Returns `Ok(None)` when more bytes are needed, otherwise the message and the number of bytes
/// consumed.
pub fn decode_frame<T: DeserializeOwned>(buf: &[u8]) -> Result<Option<(T, usize)>, ProtoError> {
    if buf.len() < HEADER_LEN {
        return Ok(None);
    }
    let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len > MAX_FRAME {
        return Err(ProtoError::FrameTooLarge(len));
    }
    let flags = buf[4];
    if flags & !FLAG_LZ4 != 0 {
        return Err(ProtoError::BadFlags(flags));
    }
    if buf.len() < HEADER_LEN + len {
        return Ok(None);
    }
    let body = &buf[HEADER_LEN..HEADER_LEN + len];
    let msg = if flags & FLAG_LZ4 != 0 {
        if body.len() < 4 {
            return Err(ProtoError::BadCompression);
        }
        let claimed = u32::from_le_bytes([body[0], body[1], body[2], body[3]]) as usize;
        if claimed > MAX_FRAME {
            return Err(ProtoError::FrameTooLarge(claimed));
        }
        let raw = lz4_flex::decompress_size_prepended(body).map_err(|_| ProtoError::BadCompression)?;
        postcard::from_bytes(&raw)?
    } else {
        postcard::from_bytes(body)?
    };
    Ok(Some((msg, HEADER_LEN + len)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(n: usize) -> DeviceInfo {
        DeviceInfo {
            busid: format!("1-{n}"),
            vendor_id: 0x1532,
            product_id: 0x0098,
            class: 0,
            speed: Speed::Full,
            manufacturer: Some("Razer".into()),
            product: Some("DeathAdder Essential".into()),
            serial: None,
            in_use_by: None,
            input: false,
        }
    }

    #[test]
    fn small_message_is_not_compressed() {
        let f = encode_frame(&ClientMsg::Ping(7)).unwrap();
        assert_eq!(f[4], 0);
        let (m, used): (ClientMsg, usize) = decode_frame(&f).unwrap().unwrap();
        assert_eq!(m, ClientMsg::Ping(7));
        assert_eq!(used, f.len());
    }

    #[test]
    fn large_device_list_is_compressed_and_round_trips() {
        let msg = ServerMsg::Devices((0..64).map(dev).collect());
        let f = encode_frame(&msg).unwrap();
        assert_eq!(f[4], FLAG_LZ4);
        let (m, used): (ServerMsg, usize) = decode_frame(&f).unwrap().unwrap();
        assert_eq!(m, msg);
        assert_eq!(used, f.len());
    }

    #[test]
    fn partial_input_asks_for_more() {
        let f = encode_frame(&ClientMsg::ListDevices).unwrap();
        for cut in 0..f.len() {
            assert!(decode_frame::<ClientMsg>(&f[..cut]).unwrap().is_none(), "cut {cut}");
        }
    }

    #[test]
    fn two_frames_back_to_back() {
        let mut buf = encode_frame(&ClientMsg::Ping(1)).unwrap();
        buf.extend(encode_frame(&ClientMsg::Ping(2)).unwrap());
        let (a, n): (ClientMsg, usize) = decode_frame(&buf).unwrap().unwrap();
        let (b, _): (ClientMsg, usize) = decode_frame(&buf[n..]).unwrap().unwrap();
        assert_eq!((a, b), (ClientMsg::Ping(1), ClientMsg::Ping(2)));
    }

    #[test]
    fn hostile_length_prefix_is_rejected() {
        let mut f = vec![0xff, 0xff, 0xff, 0x7f, 0];
        f.extend([0u8; 8]);
        assert!(matches!(decode_frame::<ClientMsg>(&f), Err(ProtoError::FrameTooLarge(_))));
    }

    #[test]
    fn hostile_decompressed_size_is_rejected() {
        let mut body = (u32::MAX).to_le_bytes().to_vec();
        body.extend([0u8; 4]);
        let mut f = (body.len() as u32).to_le_bytes().to_vec();
        f.push(FLAG_LZ4);
        f.extend(body);
        assert!(matches!(decode_frame::<ClientMsg>(&f), Err(ProtoError::FrameTooLarge(_))));
    }

    #[test]
    fn unknown_flags_are_rejected() {
        let mut f = encode_frame(&ClientMsg::ListDevices).unwrap();
        f[4] = 0x80;
        assert!(matches!(decode_frame::<ClientMsg>(&f), Err(ProtoError::BadFlags(0x80))));
    }

    #[test]
    fn corrupt_lz4_is_an_error_not_a_panic() {
        let msg = ServerMsg::Devices((0..64).map(dev).collect());
        let mut f = encode_frame(&msg).unwrap();
        let n = f.len();
        for b in &mut f[n - 20..] {
            *b ^= 0xa5;
        }
        assert!(decode_frame::<ServerMsg>(&f).is_err());
    }
}
