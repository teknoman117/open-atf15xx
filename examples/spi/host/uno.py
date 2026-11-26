"""Host side of the SPI example's Uno firmware (examples/spi/uno/spi_slave.c)."""
import os, time
import serial


class Uno:
    def __init__(self, port=None, timeout=10):
        port = port or os.environ.get('BENCH_PORT', '/dev/ttyACM0')
        self.s = serial.Serial(port, 115200, timeout=timeout)   # opening resets the Uno
        t0 = time.time()
        while time.time() - t0 < 5:
            if b'spi-slave ready' in self.s.readline():
                break
        else:
            raise RuntimeError('no spi-slave firmware on %s: make -C examples/spi/uno flash' % port)
        self.edges = None

    def cmd(self, c):
        self.s.write((c + '\n').encode())
        r = self.s.readline().decode().strip()
        if not r or r.startswith('err'):
            raise RuntimeError('uno: %r -> %r' % (c, r))
        return r

    def mode(self, pin, m):
        """pin 8 (rst_n), 9 (GCLK2/OE2) or 19 (A5 = OE1); m: 'i' input, '0'/'1' drive."""
        self.cmd('m %d %s' % (pin, m))

    def spi_slave(self, first_reply):
        """Slave on: replies `first_reply`, then each byte it received before (echo)."""
        self.cmd('x %02x' % first_reply)

    def spi_off(self):
        self.cmd('x -')

    def spi_received(self):
        """The bytes received since the last call (self.edges: SCK (rising, falling) edges)."""
        w = self.cmd('y').split()
        self.edges = (int(w[-2]), int(w[-1]))
        return [int(x, 16) for x in w[2:2 + int(w[1])]]

    def close(self):
        for p in (8, 9, 19):
            self.mode(p, 'i')
        self.spi_off()
        self.s.close()
