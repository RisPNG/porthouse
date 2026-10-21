#!/usr/bin/env python3
"""Stand-in for usbip-win2's usbip.exe that plays the kernel driver, exercising a porthouse server's
USB engine with real URBs against the loopback test device (VID 1209, PID 0002).

Command line compatible with what porthouse-client runs:
    fake-usbip.py --tcp-port P attach -r 127.0.0.1 -b TOKEN --once --terse
    fake-usbip.py detach -p N
Results go to $FAKE_OUT/results.txt, and $FAKE_OUT/done appears when the exercise has finished.

Like the real importer it exits right after the import, leaving a detached process that keeps the
socket and speaks URBs (fork on Unix, a shared socket handed to a detached child on Windows).
"""
import os, socket, struct, subprocess, sys, time

OUT = os.environ["FAKE_OUT"]
WINDOWS = os.name == "nt"


T0 = time.monotonic()


def record(name, ok, detail=""):
    with open(os.path.join(OUT, "results.txt"), "a") as f:
        f.write(f"{'PASS' if ok else 'FAIL'}  [{time.monotonic() - T0:6.2f}s] {name} {detail}\n")


def rd(s, n):
    b = b""
    while len(b) < n:
        c = s.recv(n - len(b))
        if not c:
            raise ConnectionError("connection closed")
        b += c
    return b


class Driver:
    def __init__(self, s, devid):
        self.s, self.devid, self.seq, self.dirs, self.answered = s, devid, 0, {}, set()

    def submit(self, ep, direction, length, setup=bytes(8), data=b""):
        self.seq += 1
        self.dirs[self.seq] = direction
        h = struct.pack(">IIIII", 1, self.seq, self.devid, direction, ep) + struct.pack(">IIiII", 0, length, 0, 0, 0) + setup
        self.s.sendall(h + data)
        return self.seq

    def pending(self):
        return [q for q in self.dirs if q not in self.answered]

    def unlink(self, target):
        self.seq += 1
        self.s.sendall(struct.pack(">IIIII", 2, self.seq, self.devid, 0, 0) + struct.pack(">I", target) + bytes(24))
        return self.seq

    def ret(self):
        """Read one reply: ('submit', seq, status, data) or ('unlink', seq, status)."""
        r = rd(self.s, 48)
        cmd, seq = struct.unpack(">II", r[:8])
        status, actual = struct.unpack(">iI", r[20:28])
        self.answered.add(seq)
        if cmd == 4:
            return ("unlink", seq, status)
        assert cmd == 3, f"unexpected command {cmd}"
        # Only IN transfers carry a payload; OUT replies just report the length.
        data = rd(self.s, actual) if self.dirs.get(seq) == 1 and actual else b""
        return ("submit", seq, status, data if self.dirs.get(seq) == 1 else actual)

    def control(self, setup, length, data=b""):
        d = 1 if setup[0] & 0x80 else 0
        seq = self.submit(0, d, length, setup, data)
        kind, rseq, status, payload = self.ret()
        assert (kind, rseq) == ("submit", seq), (kind, rseq, seq)
        return status, payload

    def bulk_out(self, data):
        seq = self.submit(1, 0, len(data), data=data)
        kind, rseq, status, actual = self.ret()
        assert (kind, rseq) == ("submit", seq)
        return status, actual

    def bulk_in(self, length):
        seq = self.submit(1, 1, length)
        kind, rseq, status, data = self.ret()
        assert (kind, rseq) == ("submit", seq)
        return status, data


def exercise(drv):
    try:
        st, dev = drv.control(bytes([0x80, 6, 0, 1, 0, 0, 18, 0]), 18)
        record("device descriptor read", st == 0 and len(dev) == 18 and dev[8:12] == bytes([0x09, 0x12, 0x02, 0x00]), f"status={st} {dev.hex()}")
        st, cfg = drv.control(bytes([0x80, 6, 0, 2, 0, 0, 255, 0]), 255)
        record("configuration descriptor read (32 bytes, vendor-specific interface)", st == 0 and len(cfg) == 32 and cfg[14] == 0xFF, f"status={st} len={len(cfg)}")
        st, v = drv.control(bytes([0xC0, 0x30, 0, 0, 0, 0, 4, 0]), 4)
        record("vendor control request returns NUSB", st == 0 and v == b"NUSB", f"status={st} {v!r}")
        st, _ = drv.control(bytes([0xC0, 0x55, 0, 0, 0, 0, 4, 0]), 4)
        record("unsupported control request stalls (-32)", st == -32, f"status={st}")

        msg = b"hello through the real engine"
        st, actual = drv.bulk_out(msg)
        record("bulk OUT reports the bytes written", st == 0 and actual == len(msg), f"status={st} actual={actual}")
        st, back = drv.bulk_in(512)
        record("bulk IN returns what was written", st == 0 and back == msg, f"status={st} {back!r}")

        payload = bytes((i * 7) % 251 for i in range(4096))
        st, actual = drv.bulk_out(payload)
        ok_out = st == 0 and actual == 4096
        st2, back = drv.bulk_in(8192)
        record("4096-byte bulk transfer round trip", ok_out and st2 == 0 and back == payload, f"out={st}/{actual} in={st2}/{len(back)}")

        # A read with nothing queued blocks; cancelling it must answer the unlink and not the read.
        target = drv.submit(1, 1, 512)
        time.sleep(0.5)
        ul = drv.unlink(target)
        kind, seq, status = drv.ret()[:3]
        record("cancelling a pending bulk read answers the unlink with -104", (kind, seq, status) == ("unlink", ul, -104), f"{kind} seq={seq} status={status}")
        st, actual = drv.bulk_out(b"after the cancel")
        st2, back = drv.bulk_in(512)
        record("the endpoint still works after a cancel and loses no data", st == 0 and st2 == 0 and back == b"after the cancel", f"{st}/{st2} {back!r}")

        def attempt(name, fn):
            try:
                ok, detail = fn()
                record(name, ok, detail)
            except Exception as e:  # noqa: BLE001
                record(name, False, f"{e!r}")
                raise

        def full_packet():
            drv.bulk_out(b"F" * 512)
            st, back = drv.bulk_in(512)
            return st == 0 and back == b"F" * 512, f"status={st} got {len(back)} bytes"

        attempt("a full-packet (512 of 512) read completes", full_packet)

        def second_full_packet():
            drv.bulk_out(b"G" * 512)
            st, back = drv.bulk_in(512)
            return st == 0 and back == b"G" * 512, f"status={st} got {len(back)} bytes"

        attempt("a second consecutive full-packet read completes", second_full_packet)

        def two_reads_from_1024():
            drv.bulk_out(b"H" * 1024)
            a = drv.bulk_in(512)
            b = drv.bulk_in(512)
            return a[0] == 0 and b[0] == 0 and a[1] + b[1] == b"H" * 1024, f"{len(a[1])}+{len(b[1])} bytes"

        attempt("two full-packet reads drain 1024 queued bytes", two_reads_from_1024)

        def overflow():
            # Asking for less than a packet while the device holds more cannot work on a bulk endpoint.
            # What matters is that it is reported as an overflow (-75), not returned as a clean short read.
            drv.bulk_out(b"O" * 128)
            st, back = drv.bulk_in(64)
            return st == -75, f"status={st} got {len(back)} bytes"

        attempt("a read smaller than the data on a bulk endpoint reports overflow (-75)", overflow)

        for i in range(20):
            drv.bulk_out(bytes([i]) * 10)
        want = b"".join(bytes([i]) * 10 for i in range(20))
        got = b""
        while len(got) < len(want):  # a read returns whatever is queued, up to 512 bytes
            st, chunk = drv.bulk_in(512)
            assert st == 0, st
            got += chunk
        record("20 back-to-back transfers keep their order", got == want, f"got {len(got)} bytes")
    except Exception as e:  # noqa: BLE001 - report any failure as a result, never crash silently
        record("exercise completed without error", False, f"{e!r} after {drv.seq} URBs, {len(drv.pending())} unanswered")
    open(os.path.join(OUT, "done"), "w").write("ok")


def hold_and_test(s, devid):
    s.settimeout(15)  # a reply that never comes must show up as a failure, not a silent hang
    exercise(Driver(s, devid))
    s.settimeout(None)
    try:
        while s.recv(4096):
            pass
    except OSError:
        pass


def attach(a):
    port = int(a[a.index("--tcp-port") + 1])
    token = a[a.index("-b") + 1]
    s = socket.create_connection(("127.0.0.1", port))
    s.sendall(b"\x01\x11\x80\x03\x00\x00\x00\x00" + token.encode().ljust(32, b"\0"))
    rep = rd(s, 320)
    ver, code, status = struct.unpack(">HHI", rep[:8])
    echoed = rep[8 + 256:8 + 256 + 32].split(b"\0")[0].decode()
    if (ver, code, status) != (0x0111, 3, 0) or echoed != token:
        print(f"error: USB/IP protocol violation. ver={ver:#x} code={code} status={status} busid={echoed!r}", file=sys.stderr)
        sys.exit(1)
    busnum, devnum = struct.unpack(">II", rep[8 + 256 + 32:8 + 256 + 40])
    devid = (busnum << 16) | devnum
    if WINDOWS:
        env = dict(os.environ, FAKE_DEVID=str(devid))
        child = subprocess.Popen([sys.executable, os.path.abspath(__file__), "--child"], stdin=subprocess.PIPE,
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env,
                                 creationflags=0x00000008 | 0x00000200)  # DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
        blob = s.share(child.pid)
        child.stdin.write(struct.pack(">I", len(blob)) + blob)
        child.stdin.close()
        print(9, flush=True)
        os._exit(0)
    if os.fork():
        print(9, flush=True)
        os._exit(0)
    os.setsid()
    dn = os.open(os.devnull, os.O_RDWR)
    for fd in (0, 1, 2):
        os.dup2(dn, fd)
    hold_and_test(s, devid)


def child():
    (n,) = struct.unpack(">I", sys.stdin.buffer.read(4))
    s = socket.fromshare(sys.stdin.buffer.read(n))
    hold_and_test(s, int(os.environ["FAKE_DEVID"]))


if __name__ == "__main__":
    a = sys.argv[1:]
    if "--child" in a:
        child()
    elif "detach" in a:
        open(os.path.join(OUT, "detach.txt"), "w").write(" ".join(a))
    else:
        attach(a)
