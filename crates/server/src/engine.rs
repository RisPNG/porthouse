//! Per-attachment USB engine.
//!
//! Once a client attaches, the connection carries raw USB/IP URB traffic. This module reads
//! `CMD_SUBMIT` / `CMD_UNLINK` from the stream and executes them against a real device through
//! `nusb`, writing `RET_SUBMIT` / `RET_UNLINK` back.
//!
//! Layout:
//! * a reader loop parses URBs and dispatches them;
//! * one worker task per bulk/interrupt endpoint owns that `nusb::Endpoint` and keeps a FIFO of
//!   in-flight transfers (the kernel completes them in submission order);
//! * control transfers run as short-lived tasks, with the few standard requests that the host OS
//!   owns (set configuration / interface, clear halt, set address) intercepted;
//! * a single writer task serialises replies.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use anyhow::{anyhow, Result};
use netusb_proto::usbip::{self, Urb, DIR_IN, ISO_DESC_LEN, MAX_URB_DATA};
use nusb::descriptors::TransferType;
use nusb::transfer::{
    Buffer, Bulk, BulkOrInterrupt, ControlIn, ControlOut, ControlType, Direction, EndpointDirection, In, Interrupt, Out, Recipient,
    TransferError,
};
use nusb::{Device, Endpoint, Interface};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, watch};

const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

enum Reply {
    /// `actual` is the transferred length; `data` is the payload, present only for IN transfers.
    Submit { seq: u32, status: i32, actual: u32, data: Vec<u8> },
    /// `seq` is the CMD_UNLINK's own sequence number (what the reply must echo); `target` is the
    /// URB that was unlinked, so its in-flight entry can be dropped.
    Unlink { seq: u32, status: i32, target: Option<u32> },
}

enum EpCmd {
    Submit { seq: u32, len: usize, data: Vec<u8> },
    Cancel { target: u32, unlink_seq: u32 },
    ClearHalt { done: oneshot::Sender<Result<(), TransferError>> },
}

struct EpHandle {
    tx: mpsc::Sender<EpCmd>,
    interface: u8,
}

/// seq -> endpoint address, for bulk/interrupt URBs that have not been answered yet.
type Inflight = Arc<Mutex<HashMap<u32, u8>>>;
type EpTable = Arc<Mutex<HashMap<u8, EpHandle>>>;

fn errno(e: &TransferError) -> i32 {
    match e {
        TransferError::Cancelled => usbip::ECONNRESET,
        TransferError::Stall => usbip::EPIPE,
        TransferError::Disconnected => usbip::ENODEV,
        TransferError::InvalidArgument => usbip::EINVAL,
        TransferError::Fault => usbip::EPROTO,
        _ => usbip::EIO,
    }
}

fn round_up(n: usize, to: usize) -> usize {
    n.div_ceil(to).max(1) * to
}

struct Pending {
    seq: u32,
    req_len: usize,
    data: Vec<u8>,
    /// Sequence number of the CMD_UNLINK that asked to cancel this transfer.
    unlink_seq: Option<u32>,
}

async fn worker<T: BulkOrInterrupt, D: EndpointDirection>(mut ep: Endpoint<T, D>, mut rx: mpsc::Receiver<EpCmd>, out: mpsc::Sender<Reply>) {
    let is_in = D::DIR == Direction::In;
    let mps = ep.max_packet_size().max(1);
    let mut queue: VecDeque<Pending> = VecDeque::new();

    fn submit<T: BulkOrInterrupt, D: EndpointDirection>(ep: &mut Endpoint<T, D>, is_in: bool, mps: usize, p: &Pending) {
        let buf = if is_in {
            Buffer::new(round_up(p.req_len, mps))
        } else {
            let mut b = Buffer::new(p.data.len());
            b.extend_from_slice(&p.data);
            b
        };
        ep.submit(buf);
    }

    enum Ev {
        Cmd(Option<EpCmd>),
        Done(nusb::transfer::Completion),
    }

    loop {
        let ev = std::future::poll_fn(|cx| {
            if let Poll::Ready(c) = rx.poll_recv(cx) {
                return Poll::Ready(Ev::Cmd(c));
            }
            if ep.pending() > 0 {
                if let Poll::Ready(c) = ep.poll_next_complete(cx) {
                    return Poll::Ready(Ev::Done(c));
                }
            }
            Poll::Pending
        })
        .await;

        match ev {
            Ev::Cmd(None) => break,
            Ev::Cmd(Some(EpCmd::Submit { seq, len, data })) => {
                let p = Pending {
                    seq,
                    req_len: len,
                    data,
                    unlink_seq: None,
                };
                submit(&mut ep, is_in, mps, &p);
                queue.push_back(p);
            }
            Ev::Cmd(Some(EpCmd::Cancel { target, unlink_seq })) => {
                if let Some(p) = queue.iter_mut().find(|p| p.seq == target) {
                    p.unlink_seq = Some(unlink_seq);
                    ep.cancel_all();
                } else {
                    // Already answered: nothing left to cancel.
                    let _ = out
                        .send(Reply::Unlink {
                            seq: unlink_seq,
                            status: 0,
                            target: Some(target),
                        })
                        .await;
                }
            }
            Ev::Cmd(Some(EpCmd::ClearHalt { done })) => {
                let r = ep.clear_halt().await.map_err(|_| TransferError::Fault);
                let _ = done.send(r);
            }
            Ev::Done(c) => {
                let Some(p) = queue.pop_front() else { continue };
                if let (Some(unlink_seq), Err(TransferError::Cancelled)) = (p.unlink_seq, &c.status) {
                    // Cancelled on request before it finished: answer the unlink only.
                    let _ = out
                        .send(Reply::Unlink {
                            seq: unlink_seq,
                            status: usbip::ECONNRESET,
                            target: Some(p.seq),
                        })
                        .await;
                    continue;
                }
                match c.status {
                    Ok(()) => {
                        let reply = if is_in {
                            let mut data = c.buffer.into_vec();
                            // The read buffer is rounded up to a whole number of packets, so a device can
                            // hand back more than the host asked for. Those bytes cannot be returned to the
                            // device, and a real USB stack reports this case as an overflow rather than
                            // pretending the transfer was clean, so do the same instead of dropping them silently.
                            let overflow = c.actual_len > p.req_len;
                            data.truncate(c.actual_len.min(p.req_len));
                            Reply::Submit {
                                seq: p.seq,
                                status: if overflow { usbip::EOVERFLOW } else { 0 },
                                actual: data.len() as u32,
                                data,
                            }
                        } else {
                            Reply::Submit {
                                seq: p.seq,
                                status: 0,
                                actual: c.actual_len as u32,
                                data: Vec::new(),
                            }
                        };
                        let _ = out.send(reply).await;
                        if let Some(unlink_seq) = p.unlink_seq {
                            let _ = out
                                .send(Reply::Unlink {
                                    seq: unlink_seq,
                                    status: 0,
                                    target: None,
                                })
                                .await;
                        }
                    }
                    Err(TransferError::Cancelled) => {
                        // Collateral of cancel_all() for someone else's unlink: run it again.
                        submit(&mut ep, is_in, mps, &p);
                        queue.push_back(p);
                    }
                    Err(e) => {
                        let _ = out
                            .send(Reply::Submit {
                                seq: p.seq,
                                status: errno(&e),
                                actual: 0,
                                data: Vec::new(),
                            })
                            .await;
                        if let Some(unlink_seq) = p.unlink_seq {
                            let _ = out
                                .send(Reply::Unlink {
                                    seq: unlink_seq,
                                    status: 0,
                                    target: None,
                                })
                                .await;
                        }
                    }
                }
            }
        }
    }

    ep.cancel_all();
    while ep.pending() > 0 {
        let _ = ep.next_complete().await;
    }
}

fn control_type(bm: u8) -> ControlType {
    match (bm >> 5) & 3 {
        0 => ControlType::Standard,
        1 => ControlType::Class,
        _ => ControlType::Vendor,
    }
}

fn recipient(bm: u8) -> Recipient {
    match bm & 0x1f {
        0 => Recipient::Device,
        1 => Recipient::Interface,
        2 => Recipient::Endpoint,
        _ => Recipient::Other,
    }
}

struct Session {
    device: Device,
    interfaces: Vec<Interface>,
    eps: EpTable,
    inflight: Inflight,
    out: mpsc::Sender<Reply>,
    worker_tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl Session {
    /// (Re)create workers for every endpoint of `interface` in its current alternate setting.
    fn open_endpoints(&self, interface: &Interface) -> Result<()> {
        let Some(desc) = interface.descriptor() else { return Ok(()) };
        let number = interface.interface_number();
        for e in desc.endpoints() {
            let addr = e.address();
            let is_in = addr & 0x80 != 0;
            let (tx, rx) = mpsc::channel(64);
            let out = self.out.clone();
            let task = match (e.transfer_type(), is_in) {
                (TransferType::Bulk, true) => tokio::spawn(worker(interface.endpoint::<Bulk, In>(addr)?, rx, out)),
                (TransferType::Bulk, false) => tokio::spawn(worker(interface.endpoint::<Bulk, Out>(addr)?, rx, out)),
                (TransferType::Interrupt, true) => tokio::spawn(worker(interface.endpoint::<Interrupt, In>(addr)?, rx, out)),
                (TransferType::Interrupt, false) => tokio::spawn(worker(interface.endpoint::<Interrupt, Out>(addr)?, rx, out)),
                // Isochronous (audio/video) is not carried yet; URBs to it are refused.
                _ => continue,
            };
            self.eps.lock().unwrap().insert(addr, EpHandle { tx, interface: number });
            self.worker_tasks.lock().unwrap().push(task);
        }
        Ok(())
    }

    async fn close_endpoints_of(&self, interface: u8) {
        let gone: Vec<EpHandle> = {
            let mut t = self.eps.lock().unwrap();
            let addrs: Vec<u8> = t.iter().filter(|(_, h)| h.interface == interface).map(|(a, _)| *a).collect();
            addrs.into_iter().filter_map(|a| t.remove(&a)).collect()
        };
        drop(gone); // closing the senders ends the workers
        tokio::task::yield_now().await;
    }

    fn interface(&self, number: u8) -> Option<&Interface> {
        self.interfaces.iter().find(|i| i.interface_number() == number)
    }

    /// Execute one control transfer; `Err` carries a negative errno for the URB status.
    async fn control(&self, setup: [u8; 8], dir_in: bool, data: Vec<u8>) -> Result<Vec<u8>, i32> {
        let (bm, req) = (setup[0], setup[1]);
        let value = u16::from_le_bytes([setup[2], setup[3]]);
        let index = u16::from_le_bytes([setup[4], setup[5]]);
        let length = u16::from_le_bytes([setup[6], setup[7]]);

        // Requests the host OS owns on a claimed device. They must not reach the wire as raw
        // control transfers, because the kernel tracks configuration and endpoint state itself.
        match (bm, req) {
            (0x00, 0x05) => return Ok(Vec::new()), // SET_ADDRESS: the virtual bus already did this
            (0x00, 0x09) => {
                // SET_CONFIGURATION
                let active = self.device.active_configuration().map(|c| c.configuration_value()).ok();
                return if active == Some(value as u8) {
                    Ok(Vec::new())
                } else {
                    Err(usbip::ENOSYS)
                };
            }
            (0x01, 0x0b) => {
                // SET_INTERFACE
                let number = (index & 0xff) as u8;
                let Some(iface) = self.interface(number) else {
                    return Err(usbip::EINVAL);
                };
                if iface.get_alt_setting() as u16 != value {
                    self.close_endpoints_of(number).await;
                    iface.set_alt_setting(value as u8).await.map_err(|_| usbip::EPIPE)?;
                    self.open_endpoints(iface).map_err(|_| usbip::EIO)?;
                }
                return Ok(Vec::new());
            }
            (0x02, 0x01) if value == 0 => {
                // CLEAR_FEATURE(ENDPOINT_HALT)
                let addr = (index & 0xff) as u8;
                let tx = self.eps.lock().unwrap().get(&addr).map(|h| h.tx.clone());
                let Some(tx) = tx else { return Err(usbip::EPIPE) };
                let (done, wait) = oneshot::channel();
                tx.send(EpCmd::ClearHalt { done }).await.map_err(|_| usbip::ENODEV)?;
                return match wait.await {
                    Ok(Ok(())) => Ok(Vec::new()),
                    _ => Err(usbip::EPIPE),
                };
            }
            _ => {}
        }

        // Use the interface the request is addressed to. WinUSB insists on it (an interface-directed
        // request must go through that interface's handle); Linux accepts any claimed interface.
        let target = match bm & 0x1f {
            1 => Some((index & 0xff) as u8),
            2 => self.eps.lock().unwrap().get(&((index & 0xff) as u8)).map(|h| h.interface),
            _ => None,
        };
        let ctl = target
            .and_then(|n| self.interface(n))
            .or_else(|| self.interfaces.first())
            .ok_or(usbip::ENODEV)?;
        if dir_in {
            ctl.control_in(
                ControlIn {
                    control_type: control_type(bm),
                    recipient: recipient(bm),
                    request: req,
                    value,
                    index,
                    length,
                },
                CONTROL_TIMEOUT,
            )
            .await
            .map_err(|e| errno(&e))
        } else {
            ctl.control_out(
                ControlOut {
                    control_type: control_type(bm),
                    recipient: recipient(bm),
                    request: req,
                    value,
                    index,
                    data: &data,
                },
                CONTROL_TIMEOUT,
            )
            .await
            .map(|()| Vec::new())
            .map_err(|e| errno(&e))
        }
    }
}

/// Journal of devices whose kernel drivers this process detached. If the process dies without
/// cleaning up, the next start restores them so a hard kill cannot leave an input device dead.
fn journal_path() -> std::path::PathBuf {
    // The state directory persists across restarts (and, for a service, across its private /tmp).
    netusb_common::paths::state_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("claimed-devices")
}

fn journal_add(busid: &str, interfaces: &[u8]) {
    let line = format!(
        "{busid} {}\n",
        interfaces.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
    );
    let mut all = std::fs::read_to_string(journal_path()).unwrap_or_default();
    all.push_str(&line);
    let _ = std::fs::write(journal_path(), all);
}

fn journal_remove(busid: &str) {
    let all = std::fs::read_to_string(journal_path()).unwrap_or_default();
    let keep: String = all
        .lines()
        .filter(|l| l.split(' ').next() != Some(busid))
        .map(|l| format!("{l}\n"))
        .collect();
    if keep.is_empty() {
        let _ = std::fs::remove_file(journal_path());
    } else {
        let _ = std::fs::write(journal_path(), keep);
    }
}

/// Give back any device a previous crashed instance left detached. `busid_of` maps nusb info to ids.
pub async fn restore_stale(busid_of: impl Fn(&nusb::DeviceInfo) -> String) {
    let Ok(text) = std::fs::read_to_string(journal_path()) else {
        return;
    };
    let Ok(devs) = nusb::list_devices().await else { return };
    let devs: Vec<_> = devs.collect();
    for line in text.lines() {
        let mut it = line.split(' ');
        let (Some(busid), Some(ifs)) = (it.next(), it.next()) else {
            continue;
        };
        let Some(info) = devs.iter().find(|d| busid_of(d) == busid) else {
            continue;
        };
        if let Ok(dev) = info.open().await {
            for n in ifs.split(',').filter_map(|n| n.parse::<u8>().ok()) {
                let _ = dev.attach_kernel_driver(n);
            }
            eprintln!("restored {busid} left claimed by a previous run");
        }
    }
    let _ = std::fs::remove_file(journal_path());
}

/// Why a device could not be opened, with what to do about it on this operating system.
fn open_failure(e: &impl std::fmt::Display) -> anyhow::Error {
    if cfg!(windows) {
        anyhow!(
            "cannot open the device ({e}). On Windows a device can only be shared if it uses the WinUSB driver; \
             bind WinUSB to it first (for example with Zadig; note that this replaces the driver Windows normally uses for it)"
        )
    } else {
        anyhow!("cannot open the device ({e}). Run the server as root, or allow access to the device with a udev rule")
    }
}

/// A device that is open, claimed from the host and ready to serve. Created before the client is
/// told the attach succeeded, so that a device that cannot be taken is refused with a clear error
/// instead of being reported as attached and then silently dropped.
pub struct Prepared {
    device: Device,
    numbers: Vec<u8>,
    busid: String,
    session: Arc<Session>,
    out_tx: mpsc::Sender<Reply>,
    out_rx: mpsc::Receiver<Reply>,
    inflight: Inflight,
}

/// Open the device, take its interfaces away from the host driver and start its endpoint workers.
/// On failure everything done so far is undone and the host driver is given back.
pub async fn prepare(info: nusb::DeviceInfo, busid: String) -> Result<Prepared> {
    let device = info.open().await.map_err(|e| open_failure(&e))?;
    let numbers: Vec<u8> = {
        let config = device
            .active_configuration()
            .map_err(|e| anyhow!("the device has no active configuration ({e})"))?;
        config.interfaces().map(|i| i.interface_number()).collect()
    };

    // Journal before detaching so a crash between here and teardown is recoverable.
    journal_add(&busid, &numbers);
    match claim_all(&device, &numbers).await {
        Ok((session, out_tx, out_rx, inflight)) => Ok(Prepared {
            device,
            numbers,
            busid,
            session,
            out_tx,
            out_rx,
            inflight,
        }),
        Err(e) => {
            for n in &numbers {
                let _ = device.attach_kernel_driver(*n);
            }
            journal_remove(&busid);
            Err(e)
        }
    }
}

type Claimed = (Arc<Session>, mpsc::Sender<Reply>, mpsc::Receiver<Reply>, Inflight);

async fn claim_all(device: &Device, numbers: &[u8]) -> Result<Claimed> {
    let mut interfaces = Vec::new();
    for n in numbers {
        let iface = device.detach_and_claim_interface(*n).await.map_err(|e| {
            let hint = if cfg!(windows) {
                "; composite devices (several interfaces) are not supported on Windows yet"
            } else {
                ""
            };
            anyhow!("cannot claim interface {n} of the device ({e}){hint}")
        })?;
        interfaces.push(iface);
    }

    let (out_tx, out_rx) = mpsc::channel::<Reply>(256);
    let inflight: Inflight = Arc::default();
    let session = Arc::new(Session {
        device: device.clone(),
        interfaces,
        eps: Arc::default(),
        inflight: inflight.clone(),
        out: out_tx.clone(),
        worker_tasks: Mutex::new(Vec::new()),
    });
    for iface in &session.interfaces {
        session.open_endpoints(iface)?;
    }
    Ok((session, out_tx, out_rx, inflight))
}

/// Serve one prepared attachment until the client goes away or `shutdown` fires. Gives the device
/// back to the host on exit.
pub async fn serve<S>(prepared: Prepared, stream: S, mut shutdown: watch::Receiver<bool>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let Prepared {
        device,
        numbers,
        busid,
        session,
        out_tx,
        mut out_rx,
        inflight,
    } = prepared;
    let (mut rd, mut wr) = tokio::io::split(stream);

    let writer_inflight = inflight.clone();
    let writer = tokio::spawn(async move {
        while let Some(r) = out_rx.recv().await {
            if crate::trace_on() {
                match &r {
                    Reply::Submit { seq, status, actual, .. } => eprintln!("-> RET_SUBMIT seq={seq} status={status} actual={actual}"),
                    Reply::Unlink { seq, status, .. } => eprintln!("-> RET_UNLINK seq={seq} status={status}"),
                }
            }
            let mut frame;
            match r {
                Reply::Submit { seq, status, actual, data } => {
                    writer_inflight.lock().unwrap().remove(&seq);
                    frame = usbip::ret_submit(seq, status, actual, 0).to_vec();
                    frame.extend_from_slice(&data);
                }
                Reply::Unlink { seq, status, target } => {
                    if let Some(t) = target {
                        writer_inflight.lock().unwrap().remove(&t);
                    }
                    frame = usbip::ret_unlink(seq, status).to_vec();
                }
            }
            if wr.write_all(&frame).await.is_err() {
                break;
            }
        }
    });

    let result = tokio::select! {
        r = read_loop(&mut rd, session.clone(), out_tx.clone()) => r,
        _ = shutdown.changed() => Ok(()),
    };

    // Teardown: stop workers (they cancel and drain), release the interfaces, hand the device back.
    let handles: Vec<EpHandle> = session.eps.lock().unwrap().drain().map(|(_, h)| h).collect();
    drop(handles);
    let tasks: Vec<_> = session.worker_tasks.lock().unwrap().drain(..).collect();
    for t in tasks {
        let _ = t.await;
    }
    // Dropping our senders lets the writer drain and finish (control tasks hold clones briefly).
    drop(out_tx);
    drop(session);
    let _ = writer.await;
    for n in &numbers {
        let _ = device.attach_kernel_driver(*n);
    }
    journal_remove(&busid);
    result
}

async fn read_loop<R: AsyncRead + Unpin>(rd: &mut R, session: Arc<Session>, out: mpsc::Sender<Reply>) -> Result<()> {
    loop {
        let mut hdr = [0u8; usbip::HEADER_LEN];
        match rd.read_exact(&mut hdr).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        }
        let urb = usbip::parse_header(&hdr)?;
        if crate::trace_on() {
            eprintln!("<- {urb:?}");
        }
        match urb {
            Urb::Unlink { basic, unlink_seqnum } => {
                let ep = session.inflight.lock().unwrap().get(&unlink_seqnum).copied();
                let tx = ep.and_then(|a| session.eps.lock().unwrap().get(&a).map(|h| h.tx.clone()));
                match tx {
                    Some(tx) => {
                        let _ = tx
                            .send(EpCmd::Cancel {
                                target: unlink_seqnum,
                                unlink_seq: basic.seqnum,
                            })
                            .await;
                    }
                    // Already answered, or a control transfer that cannot be aborted.
                    None => {
                        let _ = out
                            .send(Reply::Unlink {
                                seq: basic.seqnum,
                                status: 0,
                                target: None,
                            })
                            .await;
                    }
                }
            }
            Urb::Submit {
                basic,
                buffer_len,
                number_of_packets,
                setup,
                ..
            } => {
                let dir_in = basic.direction == DIR_IN;
                let len = buffer_len as usize;
                if len > MAX_URB_DATA {
                    return Err(anyhow!("URB data stage of {len} bytes exceeds the limit"));
                }
                let mut data = Vec::new();
                if !dir_in && len > 0 {
                    data = vec![0u8; len];
                    rd.read_exact(&mut data).await?;
                }
                if usbip::is_isochronous(number_of_packets) {
                    // Consume the per-packet descriptors to stay in sync, then refuse.
                    let n = (number_of_packets as usize).min(MAX_URB_DATA / ISO_DESC_LEN);
                    let mut skip = vec![0u8; n * ISO_DESC_LEN];
                    rd.read_exact(&mut skip).await?;
                    let _ = out
                        .send(Reply::Submit {
                            seq: basic.seqnum,
                            status: usbip::ENOSYS,
                            actual: 0,
                            data: Vec::new(),
                        })
                        .await;
                    continue;
                }
                let seq = basic.seqnum;
                if basic.ep == 0 {
                    let session = session.clone();
                    let out = out.clone();
                    tokio::spawn(async move {
                        let reply = match session.control(setup, dir_in, data).await {
                            Ok(mut d) if dir_in => {
                                d.truncate(len);
                                Reply::Submit {
                                    seq,
                                    status: 0,
                                    actual: d.len() as u32,
                                    data: d,
                                }
                            }
                            Ok(_) => Reply::Submit {
                                seq,
                                status: 0,
                                actual: len as u32,
                                data: Vec::new(),
                            },
                            Err(status) => Reply::Submit {
                                seq,
                                status,
                                actual: 0,
                                data: Vec::new(),
                            },
                        };
                        let _ = out.send(reply).await;
                    });
                    continue;
                }
                let addr = (basic.ep as u8 & 0x0f) | if dir_in { 0x80 } else { 0 };
                let tx = session.eps.lock().unwrap().get(&addr).map(|h| h.tx.clone());
                match tx {
                    Some(tx) => {
                        session.inflight.lock().unwrap().insert(seq, addr);
                        if tx.send(EpCmd::Submit { seq, len, data }).await.is_err() {
                            session.inflight.lock().unwrap().remove(&seq);
                            let _ = out
                                .send(Reply::Submit {
                                    seq,
                                    status: usbip::ENODEV,
                                    actual: 0,
                                    data: Vec::new(),
                                })
                                .await;
                        }
                    }
                    None => {
                        let _ = out
                            .send(Reply::Submit {
                                seq,
                                status: usbip::EPROTO,
                                actual: 0,
                                data: Vec::new(),
                            })
                            .await;
                    }
                }
            }
        }
    }
}
