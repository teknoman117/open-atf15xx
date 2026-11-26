#!/usr/bin/env python3
"""Drive the SPI example (ATF1508AS-PLCC84 on the bench) from the JTAG debugger, with the Uno as
the SPI slave.

The JTAG session plays the microprocessor: it drives the 8-bit bus (D, RS, cs_n, wr_n, rd_n) by
EXTEST through `atf15xx-tool seq` for the bus cycles, and hands the pins back to the design
(`release`) for the transfers, so the design drives SCK / MOSI / SS itself from the 66.6 MHz
oscillator at any clock select; SAMPLE (non-intrusive) watches for the end of a transfer. The Uno runs the
example's own firmware (examples/spi/uno, `make -C examples/spi/uno flash`) and is the slave:
mode 0, LSB first, it answers a chosen first byte and then echoes each byte it received. So every exchange checks both directions.

  python3 examples/spi/host/jtag_spi.py --test [--psel 2|3] [--program]    # bench self-test
  python3 examples/spi/host/jtag_spi.py 0x12 0x34 ...                        # just transfer

Build first (from the repository root):
  docker/yosys/run.sh cargo run --release -p atf15xx-synth -- build examples/spi/rtl/spi_master.v \\
      examples/spi/atf1508as/spi.v --top spi -c examples/spi/atf1508as/spi.acf -o .scratch/examples/spi-atf1508as
  cargo build --release -p atf15xx-tool
"""
import argparse, os, random, subprocess, sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..'))
OUT = os.path.join(ROOT, '.scratch', 'examples', 'spi-atf1508as')
TOOL = os.path.join(ROOT, 'target', 'release', 'atf15xx-tool')
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

DATA, CTRL = 0, 1           # RS: data register, control/status register
RUNNING, IFLAG = 0x10, 0x80  # status bits


class SpiBus:
    """The design's bus, driven over JTAG (one `atf15xx-tool seq` session, one line per command)."""

    def __init__(self, map_path=os.path.join(OUT, 'spi.map.json'), speed_khz=4000):
        self.p = subprocess.Popen([TOOL, '--speed', str(speed_khz), 'seq', '--yes-drive-pins', '--allow-inputs',
                                   '--map', map_path, '-'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
        self.idle()

    def cmd(self, line):
        self.p.stdin.write(line + '\n')
        self.p.stdin.flush()
        r = self.p.stdout.readline().strip()
        if not r or r.startswith(('error', 'MISMATCH')):
            raise RuntimeError('%s -> %r' % (line, r))
        return r

    def idle(self):
        self.cmd('drive cs_n=1 wr_n=1 rd_n=1 RS=0')
        self.cmd('float D')

    def write(self, rs, value):
        """One write cycle (the design detects the /WR edge on its clock)."""
        self.cmd('drive RS=%d D=0x%02x' % (rs, value))
        self.cmd('drive cs_n=0')                # select, then strobe: one pin switches at a time
        self.cmd('drive wr_n=0')
        self.cmd('drive wr_n=1')
        self.cmd('drive cs_n=1')
        self.cmd('float D')

    def read(self, rs):
        """One read cycle: what the design drives onto D (its captured output cells)."""
        self.cmd('drive RS=%d cs_n=0' % rs)
        self.cmd('drive rd_n=0')
        v = int(self.cmd('print D').split('=')[1], 16)
        self.cmd('drive rd_n=1')
        self.cmd('drive cs_n=1')
        return v

    def configure(self, psel, select=True, irq=False):
        self.write(CTRL, (psel << 2) | (0x40 if irq else 0) | (1 if select else 0))

    def transfer(self, byte, timeout_samples=20000):
        """Start a transfer with a bus write, hand the pins back to the design for the transfer
        itself (it drives SCK / MOSI / SS at its own speed), then end the write cycle and read the
        byte received. While the pins are released, the bus keepers hold D, RS, cs_n = 0 and
        wr_n = 0; the design loads on the /WR edge only, so the held strobe does nothing more."""
        self.cmd('drive RS=%d D=0x%02x cs_n=0' % (DATA, byte))
        self.cmd('drive wr_n=0')                # the design sees the edge: the transfer starts
        self.cmd('release')
        for _ in range(timeout_samples):
            if self.cmd('sample m.RUNNING').split('=')[1] == '0':
                break
        else:
            raise RuntimeError('transfer did not finish')
        self.cmd('drive wr_n=1')                # takes the pins again, ends the write cycle
        self.cmd('drive cs_n=1')
        self.cmd('float D')
        return self.read(DATA)

    def close(self):
        self.idle()
        self.p.stdin.close()
        self.p.wait(30)


def program():
    """Erase, program and verify the bench chip with the example's JED (`atf15xx-tool program`)."""
    r = subprocess.run([TOOL, '--speed', '4000', 'program', os.path.join(OUT, 'spi.jed')], capture_output=True, text=True)
    if r.returncode:
        raise SystemExit('programming failed:\n' + r.stdout[-800:] + r.stderr[-800:])


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('bytes', nargs='*', help='bytes to send (without --test)')
    ap.add_argument('--test', action='store_true', help='bench self-test with the Uno as the echoing slave')
    ap.add_argument('--psel', type=int, default=2, help='clock select: 0 = 16.7 MHz, 1 = 1.04 MHz, 2 = 1.02 kHz, 3 = 254 Hz SCK (default 2)')
    ap.add_argument('--count', type=int, default=16, help='bytes in the self-test')
    ap.add_argument('--program', action='store_true', help='program the bench chip first (JTAG-guarded)')
    a = ap.parse_args()
    from uno import Uno
    b = Uno()                                # opening resets the Uno: set its lines before programming
    b.mode(9, '0'); b.mode(19, '0')          # unused dedicated inputs GCLK2/OE2 and OE1 tied low
    b.mode(8, '0')                           # rst_n (GCLR) low: reset
    first = 0xA5
    b.spi_slave(first)
    if a.program:
        program()
    bus = SpiBus()
    try:
        b.mode(8, '1')                       # release reset with the bus idle
        bus.configure(a.psel)
        if a.test:
            rnd = random.Random(7)
            sent = [rnd.randrange(256) for _ in range(a.count)]
        else:
            sent = [int(x, 0) for x in a.bytes]
        got, slave = [], []
        for x in sent:                       # (the Uno keeps at most 32 bytes: collect as we go)
            got.append(bus.transfer(x))
            slave += b.spi_received()
        bus.configure(a.psel, select=False)
        slave += b.spi_received()
        for i, (s, g) in enumerate(zip(sent, got)):
            print('sent 0x%02x  received 0x%02x' % (s, g))
        if a.test:
            want_cpld = [first] + sent[:-1]
            ok = got == want_cpld and slave == sent
            print('CPLD -> Uno: %d/%d bytes as sent; Uno -> CPLD: %d/%d echoes as expected -> %s' % (
                sum(x == y for x, y in zip(slave, sent)), len(sent), sum(x == y for x, y in zip(got, want_cpld)), len(sent),
                'PASS' if ok else 'FAIL'))
            sys.exit(0 if ok else 1)
    finally:
        bus.close()
        b.close()


if __name__ == '__main__':
    main()
