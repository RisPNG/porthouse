//! USB/IP URB wire format, as consumed by the Linux `vhci_hcd` virtual host controller and the
//! Windows `usbip-win2` driver. All integers are big-endian. Every URB message is a fixed 48-byte
//! header, optionally followed by payload.
//!
//! This is the public USB/IP specification (Linux `Documentation/usb/usbip_protocol.rst`), not
//! the netusb control protocol.

pub const HEADER_LEN: usize = 48;

pub const CMD_SUBMIT: u32 = 0x0001;
pub const CMD_UNLINK: u32 = 0x0002;
pub const RET_SUBMIT: u32 = 0x0003;
pub const RET_UNLINK: u32 = 0x0004;

pub const DIR_OUT: u32 = 0;
pub const DIR_IN: u32 = 1;

/// `number_of_packets` value some implementations (e.g. usbip-win) send for "not isochronous".
/// Linux `vhci_hcd` sends 0 instead, so both mean "no isochronous packets".
pub const NON_ISO: u32 = 0xffff_ffff;

/// True when a CMD_SUBMIT really carries isochronous packet descriptors.
pub fn is_isochronous(number_of_packets: u32) -> bool {
    number_of_packets != 0 && number_of_packets != NON_ISO
}
pub const ISO_DESC_LEN: usize = 16;

/// Largest data stage we accept in a single URB. Real devices stay far below this.
pub const MAX_URB_DATA: usize = 4 * 1024 * 1024;

// Negative errno values carried in `status` fields.
pub const ECONNRESET: i32 = -104;
pub const ENODEV: i32 = -19;
pub const EPIPE: i32 = -32;
pub const EPROTO: i32 = -71;
pub const EIO: i32 = -5;
pub const EINVAL: i32 = -22;
pub const EOVERFLOW: i32 = -75;
pub const ENOSYS: i32 = -38;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Basic {
    pub command: u32,
    pub seqnum: u32,
    pub devid: u32,
    pub direction: u32,
    pub ep: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urb {
    Submit {
        basic: Basic,
        transfer_flags: u32,
        buffer_len: u32,
        start_frame: i32,
        number_of_packets: u32,
        interval: u32,
        setup: [u8; 8],
    },
    Unlink {
        basic: Basic,
        unlink_seqnum: u32,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UrbError {
    #[error("unknown USB/IP command {0:#x}")]
    UnknownCommand(u32),
}

fn be(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

pub fn parse_header(h: &[u8; HEADER_LEN]) -> Result<Urb, UrbError> {
    let basic = Basic {
        command: be(h, 0),
        seqnum: be(h, 4),
        devid: be(h, 8),
        direction: be(h, 12),
        ep: be(h, 16),
    };
    match basic.command {
        CMD_SUBMIT => {
            let mut setup = [0u8; 8];
            setup.copy_from_slice(&h[40..48]);
            Ok(Urb::Submit {
                basic,
                transfer_flags: be(h, 20),
                buffer_len: be(h, 24),
                start_frame: be(h, 28) as i32,
                number_of_packets: be(h, 32),
                interval: be(h, 36),
                setup,
            })
        }
        CMD_UNLINK => Ok(Urb::Unlink {
            basic,
            unlink_seqnum: be(h, 20),
        }),
        other => Err(UrbError::UnknownCommand(other)),
    }
}

fn put(h: &mut [u8; HEADER_LEN], at: usize, v: u32) {
    h[at..at + 4].copy_from_slice(&v.to_be_bytes());
}

/// Build a RET_SUBMIT header. `devid`, `direction` and `ep` are zero in replies per the spec.
pub fn ret_submit(seqnum: u32, status: i32, actual_len: u32, number_of_packets: u32) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    put(&mut h, 0, RET_SUBMIT);
    put(&mut h, 4, seqnum);
    put(&mut h, 20, status as u32);
    put(&mut h, 24, actual_len);
    put(&mut h, 28, 0); // start_frame
    put(&mut h, 32, number_of_packets);
    put(&mut h, 36, 0); // error_count
    h
}

pub fn ret_unlink(seqnum: u32, status: i32) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    put(&mut h, 0, RET_UNLINK);
    put(&mut h, 4, seqnum);
    put(&mut h, 20, status as u32);
    h
}

/// Build a CMD_SUBMIT header (used by tests and by a future in-process client).
pub fn cmd_submit(seqnum: u32, devid: u32, direction: u32, ep: u32, buffer_len: u32, setup: [u8; 8]) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    put(&mut h, 0, CMD_SUBMIT);
    put(&mut h, 4, seqnum);
    put(&mut h, 8, devid);
    put(&mut h, 12, direction);
    put(&mut h, 16, ep);
    put(&mut h, 24, buffer_len);
    put(&mut h, 32, 0);
    h[40..48].copy_from_slice(&setup);
    h
}

pub fn cmd_unlink(seqnum: u32, devid: u32, unlink_seqnum: u32) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    put(&mut h, 0, CMD_UNLINK);
    put(&mut h, 4, seqnum);
    put(&mut h, 8, devid);
    put(&mut h, 20, unlink_seqnum);
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_round_trips_and_is_big_endian() {
        let setup = [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00];
        let h = cmd_submit(7, 0x0005_0002, DIR_IN, 0, 18, setup);
        assert_eq!(&h[0..4], &[0, 0, 0, 1]);
        match parse_header(&h).unwrap() {
            Urb::Submit {
                basic,
                buffer_len,
                number_of_packets,
                setup: s,
                ..
            } => {
                assert_eq!((basic.seqnum, basic.devid, basic.direction, basic.ep), (7, 0x0005_0002, DIR_IN, 0));
                assert_eq!(buffer_len, 18);
                assert_eq!(number_of_packets, 0);
                assert!(!is_isochronous(number_of_packets));
                assert_eq!(s, setup);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn isochronous_detection_accepts_both_non_iso_encodings() {
        assert!(!is_isochronous(0), "Linux vhci_hcd sends 0 for non-ISO");
        assert!(!is_isochronous(NON_ISO), "usbip-win sends 0xffffffff for non-ISO");
        assert!(is_isochronous(8));
    }

    #[test]
    fn unlink_round_trips() {
        let h = cmd_unlink(9, 3, 7);
        assert_eq!(
            parse_header(&h).unwrap(),
            Urb::Unlink {
                basic: Basic {
                    command: CMD_UNLINK,
                    seqnum: 9,
                    devid: 3,
                    direction: 0,
                    ep: 0
                },
                unlink_seqnum: 7
            }
        );
    }

    #[test]
    fn unknown_command_is_an_error() {
        let mut h = [0u8; HEADER_LEN];
        h[3] = 0x77;
        assert_eq!(parse_header(&h), Err(UrbError::UnknownCommand(0x77)));
    }

    #[test]
    fn negative_status_encodes_as_twos_complement() {
        let h = ret_unlink(1, ECONNRESET);
        assert_eq!(i32::from_be_bytes([h[20], h[21], h[22], h[23]]), -104);
        assert_eq!(&h[0..4], &[0, 0, 0, 4]);
        let r = ret_submit(2, EPIPE, 0, 0);
        assert_eq!(i32::from_be_bytes([r[20], r[21], r[22], r[23]]), -32);
        assert_eq!(&r[8..20], &[0u8; 12], "devid/direction/ep must be zero in replies");
    }
}
