"""Measure the blink design: P4 (D2) must be driven (pull-up changes nothing) and toggle at
f_osc / 2e6 (33.3 Hz at 66.6 MHz); RST = P1 (D8) high must stop it with P4 low."""
import sys
from bench import Bench
b = Bench()
b.mode(8, '0')
fl = b.count(2, 2000) / 2
b.mode(2, 'u'); pu = b.count(2, 2000) / 2; b.mode(2, 'i')
b.mode(8, '1'); held = b.count(2, 1000); lvl_rst = b.read()[2]
b.mode(2, 'u'); lvl_rst_pu = b.read()[2]; b.mode(2, 'i')
b.mode(8, '0'); after = b.count(2, 1000)
b.close()
print('P4 edges/s: input %.1f, pull-up %.1f; in reset: %d edges, level %d (pull-up: %d); after reset: %d/s'
      % (fl, pu, held, lvl_rst, lvl_rst_pu, after))
ok = 32 <= pu <= 35 and 32 <= fl <= 35 and held == 0 and lvl_rst_pu == 0 and 32 <= after <= 35
print('BLINK', 'PASS' if ok else 'FAIL')
sys.exit(0 if ok else 1)
