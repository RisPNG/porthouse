use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use netusb_proto::usbip::{self, Urb, DIR_IN, ISO_DESC_LEN, MAX_URB_DATA};
use netusb_proto::AttachInfo;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, watch, Notify};
use tokio::task::AbortHandle;

pub const VENDOR_ID: u16 = 0x1209; // pid.codes
const REPORT_EVERY: Duration = Duration::from_millis(10);
/// The vendor request code announced in the Microsoft OS string descriptor.
const MS_VENDOR_CODE: u8 = 0x21;
const BULK_PACKET: usize = 512;
const LOOPBACK_GUID: &str = "{3a9d2d1c-6a4e-4f50-9b0e-5c1b7d2e8f41}";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Mouse,
    Loopback,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "mouse" => Some(Kind::Mouse),
            "loopback" => Some(Kind::Loopback),
            _ => None,
        }
    }

    pub fn from_busid(busid: &str) -> Option<Kind> {
        [Kind::Mouse, Kind::Loopback].into_iter().find(|k| k.busid() == busid)
    }

    pub fn busid(self) -> &'static str {
        match self {
            Kind::Mouse => "99-1",
            Kind::Loopback => "99-2",
        }
    }

    pub fn product_id(self) -> u16 {
        match self {
            Kind::Mouse => 0x0001, // pid.codes "test" product id
            Kind::Loopback => 0x0002,
        }
    }

    pub fn product_name(self) -> &'static str {
        match self {
            Kind::Mouse => "netusb virtual mouse",
            Kind::Loopback => "netusb loopback device",
        }
    }

    pub fn is_input(self) -> bool {
        self == Kind::Mouse
    }

    pub fn serial(self) -> &'static str {
        match self {
            Kind::Mouse => "0001",
            Kind::Loopback => "0002",
        }
    }

    fn speed(self) -> u32 {
        match self {
            Kind::Mouse => 2,    // full
            Kind::Loopback => 3, // high
        }
    }

    pub fn speed_class(self) -> netusb_proto::Speed {
        match self {
            Kind::Mouse => netusb_proto::Speed::Full,
            Kind::Loopback => netusb_proto::Speed::High,
        }
    }

    fn devnum(self) -> u32 {
        match self {
            Kind::Mouse => 1,
            Kind::Loopback => 2,
        }
    }

    pub fn attach_info(self) -> AttachInfo {
        AttachInfo {
            busid: self.busid().into(),
            busnum: 99,
            devnum: self.devnum(),
            speed: self.speed(),
            vendor_id: VENDOR_ID,
            product_id: self.product_id(),
            bcd_device: 0x0100,
            class: 0,
            subclass: 0,
            protocol: 0,
            configuration_value: 1,
            num_configurations: 1,
            num_interfaces: 1,
        }
    }
}

const REPORT_DESCRIPTOR: [u8; 52] = [
    0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x09, 0x01, 0xA1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03,
    0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x05, 0x81, 0x03, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x09, 0x38, 0x15, 0x81, 0x25, 0x7F,
    0x75, 0x08, 0x95, 0x03, 0x81, 0x06, 0xC0, 0xC0,
];

fn device_descriptor(kind: Kind) -> Vec<u8> {
    let (vid, pid) = (VENDOR_ID.to_le_bytes(), kind.product_id().to_le_bytes());
    vec![
        18, 1, 0x00, 0x02, // bcdUSB 2.00
        0, 0, 0, 64, // class/subclass/protocol are in the interface; ep0 max packet 64
        vid[0], vid[1], pid[0], pid[1], 0x00, 0x01, // bcdDevice 1.00
        1, 2, 3, // manufacturer, product, serial string indexes
        1, // one configuration
    ]
}

fn hid_descriptor() -> Vec<u8> {
    vec![9, 0x21, 0x11, 0x01, 0, 1, 0x22, REPORT_DESCRIPTOR.len() as u8, 0]
}

fn configuration_descriptor(kind: Kind) -> Vec<u8> {
    match kind {
        Kind::Mouse => {
            let mut c = vec![
                9, 2, 34, 0, 1, 1, 0, 0xA0, 50, // configuration: 34 bytes total, 1 interface, bus powered
                9, 4, 0, 0, 1, 3, 1, 2, 0, // interface 0: HID, boot subclass, mouse protocol, 1 endpoint
            ];
            c.extend(hid_descriptor());
            c.extend([7, 5, 0x81, 3, 4, 0, 10]); // endpoint 1 IN, interrupt, 4 bytes, every 10 ms
            debug_assert_eq!(c.len(), 34);
            c
        }
        Kind::Loopback => vec![
            9, 2, 32, 0, 1, 1, 0, 0x80, 50, // configuration: 32 bytes total, 1 interface, bus powered
            9, 4, 0, 0, 2, 0xFF, 0, 0, 0, // interface 0: vendor specific, 2 endpoints
            7, 5, 0x01, 2, 0x00, 0x02, 0, // endpoint 1 OUT, bulk, 512 bytes (high speed)
            7, 5, 0x81, 2, 0x00, 0x02, 0, // endpoint 1 IN, bulk, 512 bytes (high speed)
        ],
    }
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
}

fn string_descriptor(kind: Kind, index: u8) -> Option<Vec<u8>> {
    let text = match index {
        0 => return Some(vec![4, 3, 0x09, 0x04]), // supported languages: US English
        1 => "netusb",
        2 => kind.product_name(),
        3 => kind.serial(),
        // Microsoft OS string descriptor: "MSFT100" and the vendor request code. Only the loopback
        // device speaks it, so Windows binds WinUSB to that one and the normal HID driver to the mouse.
        0xEE if kind == Kind::Loopback => {
            let mut d = vec![0x12, 3];
            d.extend(utf16("MSFT100"));
            d.extend([MS_VENDOR_CODE, 0]);
            return Some(d);
        }
        _ => return None,
    };
    let body = utf16(text);
    let mut d = vec![(2 + body.len()) as u8, 3];
    d.extend(body);
    Some(d)
}

/// Microsoft OS 1.0 "Extended Compat ID": tells Windows to use the WinUSB driver.
fn ms_compat_id() -> Vec<u8> {
    let mut d = Vec::new();
    d.extend(40u32.to_le_bytes()); // dwLength
    d.extend(0x0100u16.to_le_bytes()); // bcdVersion
    d.extend(4u16.to_le_bytes()); // wIndex
    d.push(1); // bCount: one function
    d.extend([0u8; 7]);
    d.push(0); // bFirstInterfaceNumber
    d.push(1); // reserved
    d.extend(b"WINUSB\0\0"); // compatibleID
    d.extend([0u8; 8]); // subCompatibleID
    d.extend([0u8; 6]);
    debug_assert_eq!(d.len(), 40);
    d
}

/// Microsoft OS 1.0 "Extended Properties": registers a device interface GUID, which is what lets an
/// application (here `nusb`) open the device through WinUSB without an INF file.
fn ms_ext_properties() -> Vec<u8> {
    let name = {
        let mut n = utf16("DeviceInterfaceGUIDs");
        n.extend([0, 0]);
        n
    };
    let data = {
        let mut v = utf16(LOOPBACK_GUID);
        v.extend([0, 0, 0, 0]); // REG_MULTI_SZ: string terminator plus list terminator
        v
    };
    let prop_size = 4 + 4 + 2 + name.len() + 4 + data.len();
    let mut d = Vec::new();
    d.extend(((10 + prop_size) as u32).to_le_bytes()); // dwLength
    d.extend(0x0100u16.to_le_bytes()); // bcdVersion
    d.extend(5u16.to_le_bytes()); // wIndex
    d.extend(1u16.to_le_bytes()); // wCount
    d.extend((prop_size as u32).to_le_bytes()); // dwSize
    d.extend(7u32.to_le_bytes()); // dwPropertyDataType: REG_MULTI_SZ
    d.extend((name.len() as u16).to_le_bytes());
    d.extend(name);
    d.extend((data.len() as u32).to_le_bytes());
    d.extend(data);
    d
}

pub fn report(n: u64) -> [u8; 4] {
    let (dx, dy): (i8, i8) = match (n / 50) % 4 {
        0 => (3, 0),
        1 => (0, 3),
        2 => (-3, 0),
        _ => (0, -3),
    };
    [0, dx as u8, dy as u8, 0]
}

enum Control {
    Data(Vec<u8>),
    Stall,
}

fn control(kind: Kind, setup: [u8; 8]) -> Control {
    let (bm, req) = (setup[0], setup[1]);
    let value = u16::from_le_bytes([setup[2], setup[3]]);
    let index = u16::from_le_bytes([setup[4], setup[5]]);
    let (dtype, dindex) = ((value >> 8) as u8, (value & 0xff) as u8);
    let ok = || Control::Data(Vec::new());
    match (bm, req) {
        (0x80, 0x06) => match dtype {
            1 => Control::Data(device_descriptor(kind)),
            2 => Control::Data(configuration_descriptor(kind)),
            3 => string_descriptor(kind, dindex).map(Control::Data).unwrap_or(Control::Stall),
            _ => Control::Stall, // device qualifier / other speed: a plain 2.00 device may stall these
        },
        (0x80, 0x00) | (0x81, 0x00) | (0x82, 0x00) => Control::Data(vec![0, 0]), // GET_STATUS
        (0x00, 0x09) | (0x01, 0x0b) | (0x00, 0x05) => ok(),                      // set configuration / interface / address
        (0x02, 0x01) | (0x02, 0x03) => ok(),                                     // clear/set endpoint feature
        _ => match kind {
            Kind::Mouse => match (bm, req) {
                (0x81, 0x06) => match dtype {
                    0x21 => Control::Data(hid_descriptor()),
                    0x22 => Control::Data(REPORT_DESCRIPTOR.to_vec()),
                    _ => Control::Stall,
                },
                (0x21, 0x0a) | (0x21, 0x0b) | (0x21, 0x09) => ok(), // HID set idle / protocol / report
                (0xa1, 0x03) => Control::Data(vec![1]),             // GET_PROTOCOL: report protocol
                (0xa1, 0x02) => Control::Data(vec![0]),             // GET_IDLE
                (0xa1, 0x01) => Control::Data(vec![0; 4]),          // GET_REPORT: no movement
                _ => Control::Stall,
            },
            Kind::Loopback => match (bm, req, index) {
                (0xC0 | 0xC1, MS_VENDOR_CODE, 4) => Control::Data(ms_compat_id()),
                (0xC0 | 0xC1, MS_VENDOR_CODE, 5) => Control::Data(ms_ext_properties()),
                (0xC0, 0x30, _) => Control::Data(b"NUSB".to_vec()),
                _ => Control::Stall,
            },
        },
    }
}

enum Reply {
    Submit { seq: u32, status: i32, data: Vec<u8>, actual: u32 },
    Unlink { seq: u32, status: i32 },
}

struct Shared {
    kind: Kind,
    counter: AtomicU64,
    echo: Mutex<VecDeque<u8>>,
    echo_ready: Notify,
    pending: Mutex<HashMap<u32, AbortHandle>>,
}

pub async fn run<S>(kind: Kind, stream: S, mut shutdown: watch::Receiver<bool>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut rd, mut wr) = tokio::io::split(stream);
    let (out_tx, mut out_rx) = mpsc::channel::<Reply>(256);
    let writer = tokio::spawn(async move {
        while let Some(r) = out_rx.recv().await {
            if crate::trace_on() {
                match &r {
                    Reply::Submit { seq, status, actual, .. } => {
                        eprintln!("[{}] -> RET_SUBMIT seq={seq} status={status} actual={actual}", kind.busid())
                    }
                    Reply::Unlink { seq, status } => eprintln!("[{}] -> RET_UNLINK seq={seq} status={status}", kind.busid()),
                }
            }
            let frame = match r {
                Reply::Submit { seq, status, data, actual } => {
                    let mut f = usbip::ret_submit(seq, status, actual, 0).to_vec();
                    f.extend(data);
                    f
                }
                Reply::Unlink { seq, status } => usbip::ret_unlink(seq, status).to_vec(),
            };
            if wr.write_all(&frame).await.is_err() {
                break;
            }
        }
    });

    let shared = Arc::new(Shared {
        kind,
        counter: AtomicU64::new(0),
        echo: Mutex::new(VecDeque::new()),
        echo_ready: Notify::new(),
        pending: Mutex::new(HashMap::new()),
    });

    let result = tokio::select! {
        r = read_loop(&mut rd, &out_tx, &shared) => r,
        _ = shutdown.changed() => Ok(()),
    };

    for (_, h) in shared.pending.lock().unwrap().drain() {
        h.abort();
    }
    drop(out_tx);
    let _ = writer.await;
    result
}

fn submit_reply(seq: u32, status: i32, data: Vec<u8>) -> Reply {
    Reply::Submit {
        seq,
        status,
        actual: data.len() as u32,
        data,
    }
}

async fn read_loop<R: AsyncRead + Unpin>(rd: &mut R, out: &mpsc::Sender<Reply>, shared: &Arc<Shared>) -> Result<()> {
    loop {
        let mut hdr = [0u8; usbip::HEADER_LEN];
        match rd.read_exact(&mut hdr).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        }
        let urb = usbip::parse_header(&hdr)?;
        if crate::trace_on() {
            eprintln!("[{}] <- {urb:?}", shared.kind.busid());
        }
        match urb {
            Urb::Unlink { basic, unlink_seqnum } => {
                // Whoever removes the entry first owns the answer: this, or the task that would reply.
                let handle = shared.pending.lock().unwrap().remove(&unlink_seqnum);
                let status = match handle {
                    Some(h) => {
                        h.abort();
                        usbip::ECONNRESET
                    }
                    None => 0,
                };
                let _ = out.send(Reply::Unlink { seq: basic.seqnum, status }).await;
            }
            Urb::Submit {
                basic,
                buffer_len,
                number_of_packets,
                setup,
                ..
            } => {
                let seq = basic.seqnum;
                let dir_in = basic.direction == DIR_IN;
                let len = buffer_len as usize;
                if len > MAX_URB_DATA {
                    return Err(anyhow!("URB data stage of {len} bytes exceeds the limit"));
                }
                let mut out_data = Vec::new();
                if !dir_in && len > 0 {
                    out_data = vec![0u8; len];
                    rd.read_exact(&mut out_data).await?;
                }
                if usbip::is_isochronous(number_of_packets) {
                    let mut skip = vec![0u8; (number_of_packets as usize).min(MAX_URB_DATA / ISO_DESC_LEN) * ISO_DESC_LEN];
                    rd.read_exact(&mut skip).await?;
                    let _ = out.send(submit_reply(seq, usbip::ENOSYS, Vec::new())).await;
                    continue;
                }
                if basic.ep == 0 {
                    let reply = match control(shared.kind, setup) {
                        Control::Data(mut d) if dir_in => {
                            d.truncate(len);
                            submit_reply(seq, 0, d)
                        }
                        Control::Data(_) => Reply::Submit {
                            seq,
                            status: 0,
                            actual: len as u32,
                            data: Vec::new(),
                        },
                        Control::Stall => submit_reply(seq, usbip::EPIPE, Vec::new()),
                    };
                    let _ = out.send(reply).await;
                    continue;
                }
                match (shared.kind, basic.ep, dir_in) {
                    (Kind::Mouse, 1, true) => {
                        let (out2, sh) = (out.clone(), shared.clone());
                        let task = tokio::spawn(async move {
                            tokio::time::sleep(REPORT_EVERY).await;
                            if sh.pending.lock().unwrap().remove(&seq).is_none() {
                                return; // unlinked while waiting
                            }
                            let mut r = report(sh.counter.fetch_add(1, Ordering::Relaxed)).to_vec();
                            r.truncate(len);
                            let _ = out2.send(submit_reply(seq, 0, r)).await;
                        });
                        shared.pending.lock().unwrap().insert(seq, task.abort_handle());
                    }
                    (Kind::Loopback, 1, false) => {
                        shared.echo.lock().unwrap().extend(out_data);
                        shared.echo_ready.notify_waiters();
                        let _ = out
                            .send(Reply::Submit {
                                seq,
                                status: 0,
                                actual: len as u32,
                                data: Vec::new(),
                            })
                            .await;
                    }
                    (Kind::Loopback, 1, true) => {
                        let (out2, sh) = (out.clone(), shared.clone());
                        let task = tokio::spawn(async move {
                            loop {
                                let notified = sh.echo_ready.notified();
                                tokio::pin!(notified);
                                notified.as_mut().enable();
                                // Like real hardware: asked for less than a packet while it holds more, the
                                // device still sends a whole packet and the host sees an overflow.
                                let (mut data, overflow): (Vec<u8>, bool) = {
                                    let mut q = sh.echo.lock().unwrap();
                                    let overflow = len < BULK_PACKET && q.len() > len;
                                    let n = if overflow { q.len().min(BULK_PACKET) } else { q.len().min(len) };
                                    (q.drain(..n).collect(), overflow)
                                };
                                if !data.is_empty() {
                                    if sh.pending.lock().unwrap().remove(&seq).is_none() {
                                        // Unlinked meanwhile: put the bytes back for the next reader.
                                        let mut q = sh.echo.lock().unwrap();
                                        for b in data.into_iter().rev() {
                                            q.push_front(b);
                                        }
                                        return;
                                    }
                                    if overflow {
                                        data.truncate(len);
                                    }
                                    let status = if overflow { usbip::EOVERFLOW } else { 0 };
                                    let _ = out2
                                        .send(Reply::Submit {
                                            seq,
                                            status,
                                            actual: data.len() as u32,
                                            data,
                                        })
                                        .await;
                                    return;
                                }
                                notified.await;
                            }
                        });
                        shared.pending.lock().unwrap().insert(seq, task.abort_handle());
                    }
                    _ => {
                        let _ = out.send(submit_reply(seq, usbip::EPIPE, Vec::new())).await;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use netusb_proto::usbip::{cmd_submit, cmd_unlink, DIR_OUT};
    use tokio::io::duplex;

    /// A client end driving a virtual device the way a kernel driver would.
    struct Driver {
        s: tokio::io::DuplexStream,
        seq: u32,
        _stop: watch::Sender<bool>,
        task: tokio::task::JoinHandle<Result<()>>,
    }

    impl Driver {
        fn new(kind: Kind) -> Driver {
            let (a, b) = duplex(64 * 1024);
            let (tx, rx) = watch::channel(false);
            let task = tokio::spawn(run(kind, b, rx));
            Driver {
                s: a,
                seq: 0,
                _stop: tx,
                task,
            }
        }

        async fn send_control(&mut self, setup: [u8; 8], len: u32) -> u32 {
            self.seq += 1;
            let dir = if setup[0] & 0x80 != 0 { DIR_IN } else { DIR_OUT };
            self.s
                .write_all(&cmd_submit(self.seq, 0x0063_0001, dir, 0, len, setup))
                .await
                .unwrap();
            self.seq
        }

        async fn read_ret(&mut self) -> (u32, i32, Vec<u8>) {
            let mut h = [0u8; 48];
            self.s.read_exact(&mut h).await.unwrap();
            assert_eq!(
                u32::from_be_bytes(h[0..4].try_into().unwrap()),
                usbip::RET_SUBMIT,
                "must be RET_SUBMIT"
            );
            let seq = u32::from_be_bytes(h[4..8].try_into().unwrap());
            let status = i32::from_be_bytes(h[20..24].try_into().unwrap());
            let actual = u32::from_be_bytes(h[24..28].try_into().unwrap()) as usize;
            let mut d = vec![0u8; actual];
            self.s.read_exact(&mut d).await.unwrap();
            (seq, status, d)
        }

        async fn control(&mut self, setup: [u8; 8], len: u32) -> (i32, Vec<u8>) {
            let seq = self.send_control(setup, len).await;
            let (rseq, st, d) = self.read_ret().await;
            assert_eq!(rseq, seq);
            (st, d)
        }

        /// Bulk OUT on endpoint 1; returns the reported actual length.
        async fn bulk_out(&mut self, data: &[u8]) -> u32 {
            self.seq += 1;
            self.s
                .write_all(&cmd_submit(self.seq, 0x0063_0002, DIR_OUT, 1, data.len() as u32, [0; 8]))
                .await
                .unwrap();
            self.s.write_all(data).await.unwrap();
            let mut h = [0u8; 48];
            self.s.read_exact(&mut h).await.unwrap();
            assert_eq!(i32::from_be_bytes(h[20..24].try_into().unwrap()), 0);
            u32::from_be_bytes(h[24..28].try_into().unwrap())
        }

        async fn send_bulk_in(&mut self, len: u32) -> u32 {
            self.seq += 1;
            self.s
                .write_all(&cmd_submit(self.seq, 0x0063_0002, DIR_IN, 1, len, [0; 8]))
                .await
                .unwrap();
            self.seq
        }
    }

    fn get_descriptor(bm: u8, dtype: u8, index: u8, wlen: u16) -> [u8; 8] {
        [bm, 6, index, dtype, 0, 0, (wlen & 0xff) as u8, (wlen >> 8) as u8]
    }

    #[test]
    fn descriptors_are_consistent() {
        for kind in [Kind::Mouse, Kind::Loopback] {
            let cfg = configuration_descriptor(kind);
            assert_eq!(u16::from_le_bytes([cfg[2], cfg[3]]) as usize, cfg.len(), "{kind:?} wTotalLength");
            assert_eq!(device_descriptor(kind).len(), 18);
            assert_eq!(string_descriptor(kind, 2).unwrap()[0] as usize, 2 + kind.product_name().len() * 2);
            assert!(string_descriptor(kind, 9).is_none());
            assert_eq!(
                kind.attach_info().devid() & 0xffff,
                kind.devnum(),
                "devid carries the device number"
            );
        }
        let hid = hid_descriptor();
        assert_eq!(
            hid[7] as usize,
            REPORT_DESCRIPTOR.len(),
            "HID descriptor announces the report descriptor length"
        );
        assert_eq!(
            &configuration_descriptor(Kind::Mouse)[18..27],
            &hid[..],
            "HID descriptor sits after the interface descriptor"
        );
    }

    #[test]
    fn kinds_have_distinct_identities() {
        assert_ne!(Kind::Mouse.busid(), Kind::Loopback.busid());
        assert_ne!(Kind::Mouse.product_id(), Kind::Loopback.product_id());
        assert_eq!(Kind::from_busid("99-2"), Some(Kind::Loopback));
        assert_eq!(Kind::from_busid("5-1"), None);
        assert_eq!(Kind::parse("mouse"), Some(Kind::Mouse));
        assert_eq!(Kind::parse("keyboard"), None);
    }

    #[test]
    fn microsoft_os_descriptors_are_well_formed() {
        // String descriptor 0xEE announces "MSFT100" and the vendor code.
        let s = string_descriptor(Kind::Loopback, 0xEE).unwrap();
        assert_eq!(s.len(), 18);
        assert_eq!(s[0], 18);
        assert_eq!(&s[2..16], utf16("MSFT100").as_slice());
        assert_eq!(s[16], MS_VENDOR_CODE);
        assert!(
            string_descriptor(Kind::Mouse, 0xEE).is_none(),
            "the mouse must keep using Windows' HID driver"
        );

        let c = ms_compat_id();
        assert_eq!(u32::from_le_bytes(c[0..4].try_into().unwrap()) as usize, c.len(), "dwLength");
        assert_eq!(&c[4..6], &[0x00, 0x01], "bcdVersion 1.00");
        assert_eq!(&c[6..8], &[4, 0], "wIndex 4");
        assert_eq!(&c[18..24], b"WINUSB");

        let p = ms_ext_properties();
        assert_eq!(u32::from_le_bytes(p[0..4].try_into().unwrap()) as usize, p.len(), "dwLength");
        assert_eq!(&p[6..8], &[5, 0], "wIndex 5");
        let name_len = u16::from_le_bytes(p[18..20].try_into().unwrap()) as usize;
        assert_eq!(&p[20..20 + name_len - 2], utf16("DeviceInterfaceGUIDs").as_slice());
        let data_len_at = 20 + name_len;
        let data_len = u32::from_le_bytes(p[data_len_at..data_len_at + 4].try_into().unwrap()) as usize;
        assert_eq!(
            data_len_at + 4 + data_len,
            p.len(),
            "property data fills the rest of the descriptor"
        );
        assert_eq!(&p[data_len_at + 4..data_len_at + 4 + data_len - 4], utf16(LOOPBACK_GUID).as_slice());
    }

    #[test]
    fn the_pointer_walks_a_closed_square() {
        let (mut x, mut y) = (0i32, 0i32);
        for n in 0..200 {
            let r = report(n);
            x += r[1] as i8 as i32;
            y += r[2] as i8 as i32;
        }
        assert_eq!((x, y), (0, 0), "after one lap the pointer is back where it started");
        assert_eq!(report(0), [0, 3, 0, 0]);
        assert_eq!(report(50), [0, 0, 3, 0]);
    }

    #[tokio::test]
    async fn standard_descriptors_are_served() {
        let mut d = Driver::new(Kind::Mouse);
        let (st, dev) = d.control(get_descriptor(0x80, 1, 0, 64), 64).await;
        assert_eq!((st, dev.len(), dev[0], dev[1]), (0, 18, 18, 1));
        assert_eq!(&dev[8..12], &[0x09, 0x12, 0x01, 0x00], "VID 1209 PID 0001");
        let (_, cfg) = d.control(get_descriptor(0x80, 2, 0, 255), 255).await;
        assert_eq!(cfg.len(), 34);
        let (_, name) = d.control(get_descriptor(0x80, 3, 2, 255), 255).await;
        assert_eq!(
            String::from_utf16(&name[2..].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()).unwrap(),
            "netusb virtual mouse"
        );
        let (st, rep) = d.control(get_descriptor(0x81, 0x22, 0, 4096), 4096).await;
        assert_eq!((st, rep.len()), (0, 52));
    }

    #[tokio::test]
    async fn requests_a_full_speed_device_cannot_answer_stall() {
        let mut d = Driver::new(Kind::Mouse);
        assert_eq!(
            d.control(get_descriptor(0x80, 6, 0, 10), 10).await.0,
            usbip::EPIPE,
            "device qualifier"
        );
        assert_eq!(
            d.control([0xc0, 0x77, 0, 0, 0, 0, 4, 0], 4).await.0,
            usbip::EPIPE,
            "unknown vendor request"
        );
        assert_eq!(
            d.control(get_descriptor(0x80, 3, 9, 255), 255).await.0,
            usbip::EPIPE,
            "missing string"
        );
        // The mouse does not speak Microsoft OS descriptors, so Windows must not pick WinUSB for it.
        assert_eq!(d.control(get_descriptor(0x80, 3, 0xEE, 18), 18).await.0, usbip::EPIPE);
    }

    #[tokio::test]
    async fn configuration_and_hid_setup_requests_succeed() {
        let mut d = Driver::new(Kind::Mouse);
        for setup in [
            [0x00, 9, 1, 0, 0, 0, 0, 0],
            [0x21, 0x0a, 0, 0, 0, 0, 0, 0],
            [0x21, 0x0b, 0, 0, 0, 0, 0, 0],
        ] {
            assert_eq!(d.control(setup, 0).await.0, 0, "{setup:02x?}");
        }
        assert_eq!(d.control([0xa1, 3, 0, 0, 0, 0, 1, 0], 1).await, (0, vec![1]));
    }

    #[tokio::test]
    async fn the_interrupt_endpoint_streams_movement_reports() {
        let mut d = Driver::new(Kind::Mouse);
        let t0 = std::time::Instant::now();
        let mut sum = (0i32, 0i32);
        for i in 0..20u32 {
            d.seq += 1;
            d.s.write_all(&cmd_submit(d.seq, 0x0063_0001, DIR_IN, 1, 4, [0; 8])).await.unwrap();
            let (seq, st, data) = d.read_ret().await;
            assert_eq!((seq, st, data.len()), (d.seq, 0, 4), "report {i}");
            assert_eq!(data, report(i as u64).to_vec());
            sum.0 += data[1] as i8 as i32;
            sum.1 += data[2] as i8 as i32;
        }
        assert_eq!(sum, (60, 0), "20 reports of +3 on X");
        assert!(
            t0.elapsed() >= Duration::from_millis(150),
            "reports are paced like a real device, not instantaneous"
        );
    }

    #[tokio::test]
    async fn an_unlinked_interrupt_urb_gets_no_report_and_the_unlink_says_so() {
        let mut d = Driver::new(Kind::Mouse);
        d.seq += 1;
        let target = d.seq;
        d.s.write_all(&cmd_submit(target, 0x0063_0001, DIR_IN, 1, 4, [0; 8])).await.unwrap();
        d.seq += 1;
        d.s.write_all(&cmd_unlink(d.seq, 0x0063_0001, target)).await.unwrap();
        let mut h = [0u8; 48];
        d.s.read_exact(&mut h).await.unwrap();
        assert_eq!(u32::from_be_bytes(h[0..4].try_into().unwrap()), usbip::RET_UNLINK);
        assert_eq!(
            u32::from_be_bytes(h[4..8].try_into().unwrap()),
            d.seq,
            "reply echoes the unlink's own sequence number"
        );
        assert_eq!(i32::from_be_bytes(h[20..24].try_into().unwrap()), usbip::ECONNRESET);
        // The cancelled URB never answers, so the next thing on the wire is the next URB's reply.
        let seq = d.send_control(get_descriptor(0x80, 1, 0, 18), 18).await;
        let (rseq, st, _) = d.read_ret().await;
        assert_eq!((rseq, st), (seq, 0));
    }

    #[tokio::test]
    async fn unlinking_something_already_answered_reports_status_zero() {
        let mut d = Driver::new(Kind::Mouse);
        let seq = d.send_control(get_descriptor(0x80, 1, 0, 18), 18).await;
        let _ = d.read_ret().await;
        d.seq += 1;
        d.s.write_all(&cmd_unlink(d.seq, 0x0063_0001, seq)).await.unwrap();
        let mut h = [0u8; 48];
        d.s.read_exact(&mut h).await.unwrap();
        assert_eq!(i32::from_be_bytes(h[20..24].try_into().unwrap()), 0);
    }

    #[tokio::test]
    async fn isochronous_urbs_are_refused_without_desynchronising_the_stream() {
        let mut d = Driver::new(Kind::Mouse);
        d.seq += 1;
        let mut h = cmd_submit(d.seq, 0x0063_0001, DIR_IN, 2, 0, [0; 8]);
        h[32..36].copy_from_slice(&2u32.to_be_bytes()); // two packets
        d.s.write_all(&h).await.unwrap();
        d.s.write_all(&[0u8; 2 * ISO_DESC_LEN]).await.unwrap();
        let (_, st, _) = d.read_ret().await;
        assert_eq!(st, usbip::ENOSYS);
        assert_eq!(d.control(get_descriptor(0x80, 1, 0, 18), 18).await.0, 0, "stream is still in sync");
    }

    #[tokio::test]
    async fn the_session_ends_when_the_client_disconnects() {
        let d = Driver::new(Kind::Mouse);
        let Driver { s, task, _stop, .. } = d;
        drop(s);
        let r = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("did not end")
            .unwrap();
        assert!(r.is_ok());
    }

    // ---- loopback device ----------------------------------------------------------------------

    #[tokio::test]
    async fn loopback_serves_the_microsoft_os_descriptors_over_control_transfers() {
        let mut d = Driver::new(Kind::Loopback);
        let (st, s) = d.control(get_descriptor(0x80, 3, 0xEE, 18), 18).await;
        assert_eq!((st, s.len()), (0, 18));
        assert_eq!(s[16], MS_VENDOR_CODE);
        let (st, header) = d.control([0xC0, MS_VENDOR_CODE, 0, 0, 4, 0, 16, 0], 16).await;
        assert_eq!((st, header.len()), (0, 16), "the host first reads only the 16-byte header");
        let (_, full) = d.control([0xC0, MS_VENDOR_CODE, 0, 0, 4, 0, 0x28, 0], 0x28).await;
        assert_eq!(&full[18..24], b"WINUSB");
        let (_, props) = d.control([0xC1, MS_VENDOR_CODE, 0, 0, 5, 0, 0xff, 0], 0xff).await;
        assert_eq!(props.len(), ms_ext_properties().len());
        assert_eq!(
            d.control([0xC0, 0x30, 0, 0, 0, 0, 4, 0], 4).await,
            (0, b"NUSB".to_vec()),
            "vendor request"
        );
        assert_eq!(
            d.control([0xC0, 0x55, 0, 0, 0, 0, 4, 0], 4).await.0,
            usbip::EPIPE,
            "unknown vendor request stalls"
        );
    }

    #[tokio::test]
    async fn loopback_echoes_what_is_written() {
        let mut d = Driver::new(Kind::Loopback);
        assert_eq!(
            d.bulk_out(b"hello, bulk endpoint").await,
            20,
            "OUT reports the number of bytes taken"
        );
        let seq = d.send_bulk_in(64).await;
        let (rseq, st, data) = d.read_ret().await;
        assert_eq!((rseq, st), (seq, 0));
        assert_eq!(data, b"hello, bulk endpoint");
    }

    #[tokio::test]
    async fn loopback_in_waits_for_data_and_preserves_order() {
        let mut d = Driver::new(Kind::Loopback);
        // An IN request issued first must wait until something is written.
        let seq = d.send_bulk_in(64).await;
        let early = tokio::time::timeout(Duration::from_millis(150), d.read_ret()).await;
        assert!(early.is_err(), "nothing to return yet");
        d.bulk_out(b"first").await;
        let (rseq, _, data) = d.read_ret().await;
        assert_eq!((rseq, data.as_slice()), (seq, b"first".as_slice()));
        // Packet-sized reads drain a larger queue in order, one packet at a time.
        let payload: Vec<u8> = (0..1024u32).map(|i| (i % 251) as u8).collect();
        d.bulk_out(&payload).await;
        d.send_bulk_in(512).await;
        assert_eq!(d.read_ret().await.2, &payload[..512]);
        d.send_bulk_in(512).await;
        assert_eq!(d.read_ret().await.2, &payload[512..]);
    }

    #[tokio::test]
    async fn a_read_smaller_than_a_packet_while_more_is_queued_overflows_like_hardware() {
        let mut d = Driver::new(Kind::Loopback);
        d.bulk_out(&[7u8; 128]).await;
        let seq = d.send_bulk_in(64).await;
        let (rseq, st, data) = d.read_ret().await;
        assert_eq!(
            (rseq, st, data.len()),
            (seq, usbip::EOVERFLOW, 64),
            "only the requested 64 bytes are returned, with an overflow status"
        );
        // The device sent a whole packet, so the remainder is gone from the device too.
        d.bulk_out(b"next").await;
        d.send_bulk_in(512).await;
        assert_eq!(d.read_ret().await.2, b"next");
    }

    #[test]
    fn the_loopback_device_is_a_consistent_high_speed_device() {
        assert_eq!(Kind::Loopback.attach_info().speed, 3, "high speed");
        let cfg = configuration_descriptor(Kind::Loopback);
        assert_eq!(&cfg[22..24], &[0x00, 0x02], "OUT endpoint: 512-byte packets");
        assert_eq!(&cfg[29..31], &[0x00, 0x02], "IN endpoint: 512-byte packets");
        assert_eq!(Kind::Mouse.attach_info().speed, 2, "the mouse stays a full-speed device");
    }

    #[tokio::test]
    async fn loopback_moves_large_transfers_intact() {
        let mut d = Driver::new(Kind::Loopback);
        let payload: Vec<u8> = (0..4096u32).map(|i| (i * 7 % 251) as u8).collect();
        assert_eq!(d.bulk_out(&payload).await, 4096);
        d.send_bulk_in(8192).await;
        assert_eq!(d.read_ret().await.2, payload);
    }

    #[tokio::test]
    async fn an_unlinked_loopback_read_gives_its_bytes_to_the_next_reader() {
        let mut d = Driver::new(Kind::Loopback);
        d.seq += 1;
        let target = d.seq;
        d.s.write_all(&cmd_submit(target, 0x0063_0002, DIR_IN, 1, 64, [0; 8]))
            .await
            .unwrap();
        d.seq += 1;
        d.s.write_all(&cmd_unlink(d.seq, 0x0063_0002, target)).await.unwrap();
        let mut h = [0u8; 48];
        d.s.read_exact(&mut h).await.unwrap();
        assert_eq!(i32::from_be_bytes(h[20..24].try_into().unwrap()), usbip::ECONNRESET);
        d.bulk_out(b"kept").await;
        d.send_bulk_in(64).await;
        assert_eq!(d.read_ret().await.2, b"kept", "data written after the cancelled read is not lost");
    }

    #[tokio::test]
    async fn the_mouse_has_no_bulk_endpoints_and_the_loopback_has_no_interrupt_one() {
        let mut m = Driver::new(Kind::Mouse);
        m.seq += 1;
        m.s.write_all(&cmd_submit(m.seq, 1, DIR_OUT, 1, 4, [0; 8])).await.unwrap();
        m.s.write_all(&[0u8; 4]).await.unwrap();
        assert_eq!(m.read_ret().await.1, usbip::EPIPE);
        let mut l = Driver::new(Kind::Loopback);
        l.seq += 1;
        l.s.write_all(&cmd_submit(l.seq, 1, DIR_IN, 2, 4, [0; 8])).await.unwrap();
        assert_eq!(l.read_ret().await.1, usbip::EPIPE, "endpoint 2 does not exist");
    }
}
