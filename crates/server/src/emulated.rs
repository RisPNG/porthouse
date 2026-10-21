//! A software USB device for testing: a boot-protocol HID mouse that draws a small square.
//!
//! It lets the whole attach path (kernel driver, importer, relay, TLS, server) be exercised on a
//! machine with no spare hardware, including real interrupt data flow. Enabled with
//! `netusb-server run --test-device mouse`. While attached it moves the pointer of the machine it
//! is attached to, so it is strictly opt-in.
//!
//! It speaks the same URB protocol as the real engine, so the client cannot tell the difference.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use netusb_proto::usbip::{self, Urb, DIR_IN, ISO_DESC_LEN, MAX_URB_DATA};
use netusb_proto::AttachInfo;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, watch};
use tokio::task::AbortHandle;

pub const BUSID: &str = "99-1";
pub const VENDOR_ID: u16 = 0x1209; // pid.codes
pub const PRODUCT_ID: u16 = 0x0001; // pid.codes "test" product id
/// Report interval of the interrupt endpoint, like a real full-speed mouse's bInterval.
const REPORT_EVERY: Duration = Duration::from_millis(10);

pub fn attach_info() -> AttachInfo {
    AttachInfo {
        busid: BUSID.into(),
        busnum: 99,
        devnum: 1,
        speed: 2, // full speed
        vendor_id: VENDOR_ID,
        product_id: PRODUCT_ID,
        bcd_device: 0x0100,
        class: 0,
        subclass: 0,
        protocol: 0,
        configuration_value: 1,
        num_configurations: 1,
        num_interfaces: 1,
    }
}

/// Standard boot mouse report descriptor: 3 buttons, relative X, Y and wheel.
const REPORT_DESCRIPTOR: [u8; 52] = [
    0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x09, 0x01, 0xA1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03,
    0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x05, 0x81, 0x03, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x09, 0x38, 0x15, 0x81, 0x25, 0x7F,
    0x75, 0x08, 0x95, 0x03, 0x81, 0x06, 0xC0, 0xC0,
];

fn device_descriptor() -> Vec<u8> {
    vec![
        18,
        1,
        0x00,
        0x02, // bcdUSB 2.00
        0,
        0,
        0,
        64, // class/subclass/protocol in the interface; ep0 max packet 64
        (VENDOR_ID & 0xff) as u8,
        (VENDOR_ID >> 8) as u8,
        (PRODUCT_ID & 0xff) as u8,
        (PRODUCT_ID >> 8) as u8,
        0x00,
        0x01, // bcdDevice 1.00
        1,
        2,
        3, // manufacturer, product, serial string indexes
        1, // one configuration
    ]
}

fn hid_descriptor() -> Vec<u8> {
    vec![9, 0x21, 0x11, 0x01, 0, 1, 0x22, REPORT_DESCRIPTOR.len() as u8, 0]
}

fn configuration_descriptor() -> Vec<u8> {
    let mut c = vec![
        9, 2, 34, 0, 1, 1, 0, 0xA0, 50, // configuration: 34 bytes total, 1 interface, bus powered
        9, 4, 0, 0, 1, 3, 1, 2, 0, // interface 0: HID, boot subclass, mouse protocol, 1 endpoint
    ];
    c.extend(hid_descriptor());
    c.extend([7, 5, 0x81, 3, 4, 0, 10]); // endpoint 1 IN, interrupt, 4 bytes, every 10 ms
    debug_assert_eq!(c.len(), 34);
    c
}

fn string_descriptor(index: u8) -> Option<Vec<u8>> {
    let text = match index {
        0 => return Some(vec![4, 3, 0x09, 0x04]), // supported languages: US English
        1 => "netusb",
        2 => "netusb virtual mouse",
        3 => "0001",
        _ => return None,
    };
    let utf16: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let mut d = vec![(2 + utf16.len()) as u8, 3];
    d.extend(utf16);
    Some(d)
}

/// The n-th movement report: the pointer walks the sides of a square, 50 reports per side.
pub fn report(n: u64) -> [u8; 4] {
    let (dx, dy): (i8, i8) = match (n / 50) % 4 {
        0 => (3, 0),
        1 => (0, 3),
        2 => (-3, 0),
        _ => (0, -3),
    };
    [0, dx as u8, dy as u8, 0]
}

/// Outcome of a control request.
enum Control {
    Data(Vec<u8>),
    Stall,
}

fn control(setup: [u8; 8]) -> Control {
    let (bm, req) = (setup[0], setup[1]);
    let value = u16::from_le_bytes([setup[2], setup[3]]);
    let (kind, index) = ((value >> 8) as u8, (value & 0xff) as u8);
    let ok = || Control::Data(Vec::new());
    match (bm, req) {
        (0x80, 0x06) => match kind {
            1 => Control::Data(device_descriptor()),
            2 => Control::Data(configuration_descriptor()),
            3 => string_descriptor(index).map(Control::Data).unwrap_or(Control::Stall),
            _ => Control::Stall, // device qualifier / other speed: full-speed devices stall these
        },
        (0x81, 0x06) => match kind {
            0x21 => Control::Data(hid_descriptor()),
            0x22 => Control::Data(REPORT_DESCRIPTOR.to_vec()),
            _ => Control::Stall,
        },
        (0x80, 0x00) | (0x81, 0x00) | (0x82, 0x00) => Control::Data(vec![0, 0]), // GET_STATUS
        (0x00, 0x09) | (0x01, 0x0b) | (0x00, 0x05) => ok(),                      // set configuration / interface / address
        (0x02, 0x01) | (0x02, 0x03) => ok(),                                     // clear/set endpoint feature
        (0x21, 0x0a) | (0x21, 0x0b) | (0x21, 0x09) => ok(),                      // HID set idle / protocol / report
        (0xa1, 0x03) => Control::Data(vec![1]),                                  // GET_PROTOCOL: report protocol
        (0xa1, 0x02) => Control::Data(vec![0]),                                  // GET_IDLE
        (0xa1, 0x01) => Control::Data(vec![0; 4]),                               // GET_REPORT: no movement
        _ => Control::Stall,
    }
}

enum Reply {
    Submit { seq: u32, status: i32, data: Vec<u8>, actual: u32 },
    Unlink { seq: u32, status: i32 },
}

/// Serve one attachment of the virtual mouse until the client leaves or `shutdown` fires.
pub async fn run<S>(stream: S, mut shutdown: watch::Receiver<bool>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut rd, mut wr) = tokio::io::split(stream);
    let (out_tx, mut out_rx) = mpsc::channel::<Reply>(256);
    let writer = tokio::spawn(async move {
        while let Some(r) = out_rx.recv().await {
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

    let pending: Arc<Mutex<HashMap<u32, AbortHandle>>> = Arc::default();
    let counter = Arc::new(AtomicU64::new(0));

    let result = tokio::select! {
        r = read_loop(&mut rd, &out_tx, &pending, &counter) => r,
        _ = shutdown.changed() => Ok(()),
    };

    for (_, h) in pending.lock().unwrap().drain() {
        h.abort();
    }
    drop(out_tx);
    let _ = writer.await;
    result
}

async fn read_loop<R: AsyncRead + Unpin>(
    rd: &mut R,
    out: &mpsc::Sender<Reply>,
    pending: &Arc<Mutex<HashMap<u32, AbortHandle>>>,
    counter: &Arc<AtomicU64>,
) -> Result<()> {
    loop {
        let mut hdr = [0u8; usbip::HEADER_LEN];
        match rd.read_exact(&mut hdr).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        }
        match usbip::parse_header(&hdr)? {
            Urb::Unlink { basic, unlink_seqnum } => {
                // Whoever removes the entry first owns the answer: this, or the report timer.
                let handle = pending.lock().unwrap().remove(&unlink_seqnum);
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
                if !dir_in && len > 0 {
                    let mut data = vec![0u8; len];
                    rd.read_exact(&mut data).await?;
                }
                if usbip::is_isochronous(number_of_packets) {
                    let mut skip = vec![0u8; (number_of_packets as usize).min(MAX_URB_DATA / ISO_DESC_LEN) * ISO_DESC_LEN];
                    rd.read_exact(&mut skip).await?;
                    let _ = out
                        .send(Reply::Submit {
                            seq,
                            status: usbip::ENOSYS,
                            data: Vec::new(),
                            actual: 0,
                        })
                        .await;
                    continue;
                }
                if basic.ep == 0 {
                    let reply = match control(setup) {
                        Control::Data(mut d) if dir_in => {
                            d.truncate(len);
                            Reply::Submit {
                                seq,
                                status: 0,
                                actual: d.len() as u32,
                                data: d,
                            }
                        }
                        Control::Data(_) => Reply::Submit {
                            seq,
                            status: 0,
                            actual: len as u32,
                            data: Vec::new(),
                        },
                        Control::Stall => Reply::Submit {
                            seq,
                            status: usbip::EPIPE,
                            actual: 0,
                            data: Vec::new(),
                        },
                    };
                    let _ = out.send(reply).await;
                } else if basic.ep == 1 && dir_in {
                    // Interrupt IN: answer with the next movement report after one report interval.
                    let (out2, pending2, counter2) = (out.clone(), pending.clone(), counter.clone());
                    let task = tokio::spawn(async move {
                        tokio::time::sleep(REPORT_EVERY).await;
                        if pending2.lock().unwrap().remove(&seq).is_none() {
                            return; // unlinked while waiting
                        }
                        let mut r = report(counter2.fetch_add(1, Ordering::Relaxed)).to_vec();
                        r.truncate(len.max(0));
                        let _ = out2
                            .send(Reply::Submit {
                                seq,
                                status: 0,
                                actual: r.len() as u32,
                                data: r,
                            })
                            .await;
                    });
                    pending.lock().unwrap().insert(seq, task.abort_handle());
                } else {
                    let _ = out
                        .send(Reply::Submit {
                            seq,
                            status: usbip::EPIPE,
                            data: Vec::new(),
                            actual: 0,
                        })
                        .await;
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

    /// A client end driving the virtual device the way a kernel driver would.
    struct Driver {
        s: tokio::io::DuplexStream,
        seq: u32,
        _stop: watch::Sender<bool>,
        task: tokio::task::JoinHandle<Result<()>>,
    }

    impl Driver {
        fn new() -> Driver {
            let (a, b) = duplex(64 * 1024);
            let (tx, rx) = watch::channel(false);
            let task = tokio::spawn(run(b, rx));
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
    }

    fn get_descriptor(bm: u8, kind: u8, index: u8, wlen: u16) -> [u8; 8] {
        [bm, 6, index, kind, 0, 0, (wlen & 0xff) as u8, (wlen >> 8) as u8]
    }

    #[test]
    fn descriptors_are_consistent() {
        let cfg = configuration_descriptor();
        assert_eq!(u16::from_le_bytes([cfg[2], cfg[3]]) as usize, cfg.len(), "wTotalLength");
        let hid = hid_descriptor();
        assert_eq!(
            hid[7] as usize,
            REPORT_DESCRIPTOR.len(),
            "HID descriptor announces the report descriptor length"
        );
        assert_eq!(&cfg[18..27], &hid[..], "HID descriptor sits after the interface descriptor");
        assert_eq!(device_descriptor().len(), 18);
        assert_eq!(string_descriptor(2).unwrap()[0] as usize, 2 + "netusb virtual mouse".len() * 2);
        assert!(string_descriptor(9).is_none());
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
        let mut d = Driver::new();
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
        let mut d = Driver::new();
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
    }

    #[tokio::test]
    async fn configuration_and_hid_setup_requests_succeed() {
        let mut d = Driver::new();
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
        let mut d = Driver::new();
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
        let mut d = Driver::new();
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
        let mut d = Driver::new();
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
        let mut d = Driver::new();
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
        let d = Driver::new();
        let Driver { s, task, _stop, .. } = d;
        drop(s);
        let r = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("did not end")
            .unwrap();
        assert!(r.is_ok());
    }
}
