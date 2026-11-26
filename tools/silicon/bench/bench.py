"""Host side of the Uno test instrument (bench.c). Lines: D2, D3, D4, D5, D8.

Wiring on the user's bench (2026-11-24): D2-P4, D3-P5, D4-P81 (GCLK3), D5-P80, each through 1 kOhm;
D8-P1 (GCLR, direct), D9-P2 (GCLK2/OE2; Timer1 OC1A), A5 (19)-P84 (OE1). 66.6 MHz oscillator on P83 (GCLK1).

  from bench import Bench
  b = Bench(); b.mode(8, '1'); b.read(); b.count(2, 1000)
"""
import os, time
import serial

LINES = (2, 3, 4, 5, 8, 9, 19)   # 19 = A5


class Bench:
    def __init__(self, port=None, timeout=70):
        port = port or os.environ.get('BENCH_PORT', '/dev/ttyACM0')
        self.s = serial.Serial(port, 115200, timeout=timeout)   # opening resets the Uno
        t0 = time.time()
        while time.time() - t0 < 5:
            if b'bench ready' in self.s.readline():
                break
        else:
            raise RuntimeError('bench: no banner from %s' % port)
        assert self.cmd('?').startswith('bench 3')

    def cmd(self, c):
        self.s.write((c + '\n').encode())
        r = self.s.readline().decode().strip()
        if not r or r.startswith('err'):
            raise RuntimeError('bench: %r -> %r' % (c, r))
        return r

    def mode(self, pin, m):
        """m: 'i' input, 'u' input with pull-up, '0'/'1' drive."""
        self.cmd('m %d %s' % (pin, m))

    def release(self):
        for p in LINES:
            self.mode(p, 'i')

    def read(self):
        """{pin: level}"""
        r = self.cmd('r').split()[1]
        return {p: int(r[i]) for i, p in enumerate(LINES)}

    def count(self, pin, ms):
        """rising edges on pin during ms milliseconds"""
        return int(self.cmd('c %d %d' % (pin, ms)).split()[1])

    def pulse(self, pin, n=1, us=10):
        self.cmd('p %d %d %d' % (pin, n, us))

    def freq(self, hz):
        """free-running clock on D9 (Timer1); returns the exact frequency; 0 stops it (D9 low)"""
        return int(self.cmd('f %d' % hz).split()[1])

    def burst(self, n):
        """exactly n pulses on D9 at 1 MHz (D9 must be an output, timer stopped)"""
        self.cmd('b %d' % n)

    def sample(self, n=200):
        """n back-to-back samples (~1 us apart): list of {pin: level}"""
        vals = [int(x, 16) for x in self.cmd('s 2 %d' % n).split()[1:]]
        return [{p: (v >> i) & 1 for i, p in enumerate(LINES)} for v in vals]

    def close(self):
        self.release()
        self.s.close()
