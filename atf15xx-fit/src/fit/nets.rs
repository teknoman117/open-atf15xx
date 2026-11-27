//! Global networks (spec F4.4): GCLK sources and polarities.

use super::*;

/// Global clock networks (spec F4.5): which pin feeds which network, in which polarity.
pub(super) fn global_nets(dev: &'static Device, design: &Design, opts: &Options, placed: &Placed) -> Nets {
    let Placed { place_of, .. } = placed;
    let net_of_pin = |pin: u16| gclk_net_of_pin(dev, pin);
    let pin_of_name = |n: &str| placed_pin(place_of, n);
    // ---- global nets
    // GCLK networks: `gclk_net[n-1] = (clock pin k, positive)` -- network n takes clock pin k
    // (1 = GCLK1, 2 = GCLK2/OE2, 3 = GCLK3 I/O) through its source mux and inverts it unless
    // positive. The invert bit is per network: when cells clock on both edges of one clock pin,
    // the polarity used by more cells (cells with a clock enable count first, as they cannot use
    // a PT clock) gets the pin's own network; with `share_clock_networks` the other polarity gets
    // a free network (one whose own pin clocks nothing), otherwise those cells use a PT clock.
    let mut gclk_net: [Option<(u8, bool)>; 3] = [None; 3];
    {
        let mut votes: BTreeMap<u8, (usize, usize)> = BTreeMap::new();
        for c in &design.cells {
            if let Some((Signal::Pad(n), pos)) = c.clk.as_ref().and_then(|f| f.single_literal()) {
                if let Some(net) = pin_of_name(n).and_then(net_of_pin) {
                    let w = if c.ce.is_some() { 1000 } else { 1 };
                    let v = votes.entry(net).or_default();
                    if pos {
                        v.0 += w
                    } else {
                        v.1 += w
                    }
                }
            }
        }
        for (&k, &(p, n)) in &votes {
            gclk_net[k as usize - 1] = Some((k, p >= n));
        }
        if opts.share_clock_networks {
            for (&k, &(p, n)) in &votes {
                if p > 0 && n > 0 {
                    if let Some(free) = (0..3).find(|&i| gclk_net[i].is_none()) {
                        gclk_net[free] = Some((k, p < n));
                    }
                }
            }
        }
    }

    Nets { gclk_net, gclr_inv: None, goe: BTreeMap::new() }
}
