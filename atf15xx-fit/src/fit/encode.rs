//! Fuse assembly (spec F2): macrocell configuration, product terms, UIM and global fuses.

use super::*;

/// Fuse assembly (spec F5): UIM and GOE selections, global configuration, every macrocell.
#[allow(clippy::too_many_arguments)]
pub(super) fn assemble(
    dev: &'static Device,
    design: Design,
    opts: &Options,
    placed: Placed,
    nets: &Nets,
    plans: &[PtPlan],
    folds: &Folds,
    routing: Routing,
) -> Result<FitResult, FitError> {
    let f = dev.family;
    let nmc = f.macrocells();
    let Placed { uses, place_of, .. } = placed;
    let Nets { gclk_net, gclr_inv, goe } = nets;
    let Folds { fold_mc, fold_at } = folds;
    let Routing { routing, col_of, sel_of } = routing;
    let source_of = |s: &Signal| signal_source(&design, &place_of, s);
    // ---- assemble
    let mut fz = Fuses::new(f);
    fz.security = opts.security;
    for a in f.mc_filler_fields() {
        fz.fill(a, 16, true);
    }
    for b in 0..f.blocks() {
        for m in 0..40 {
            fz.fill(f.uim_addr(b, m), f.uim_width(), true);
        }
        for (mux, src) in sel_of.iter().filter(|((bb, _), _)| *bb == b).map(|((_, m), s)| (*m, *s)) {
            let i = f.uim_choices(mux).iter().position(|&x| x == src).unwrap();
            fz.set(f.uim_addr(b, mux) + i, false);
        }
    }
    for n in 1..=6u8 {
        fz.fill(f.goe_addr(n as usize), f.uim_width(), true);
        if let Some((src, _)) = goe.get(&n) {
            let i = f.goe_choices(n as usize).iter().position(|x| x == src).unwrap();
            fz.set(f.goe_addr(n as usize) + i, false);
        }
    }
    // global configuration
    let cfg = f.config_base();
    fz.fill(cfg, 32, true);
    fz.set(f.config_addr(ConfigBit::Arming), false);
    fz.set(f.config_addr(ConfigBit::Termination), !opts.pin_keep);
    fz.set(f.config_addr(ConfigBit::Pd1), !opts.pd1);
    fz.set(f.config_addr(ConfigBit::Pd2), !opts.pd2);
    for n in 1..=6u8 {
        fz.set(f.config_addr(ConfigBit::GoeInvert(n)), !goe.get(&n).map_or(false, |x| x.1));
    }
    fz.set(f.config_addr(ConfigBit::GclrInvert), !gclr_inv.unwrap_or(false));
    // clock networks: source mux and invert (unused networks: the vendor default, own pin, not
    // inverted)
    for n in 1..=3u8 {
        let (k, pos) = gclk_net[n as usize - 1].unwrap_or((n, true));
        let (b0, b1) = atf15xx_arch::decode::gclk_select_for(k);
        fz.set(f.config_addr(ConfigBit::GclkInvert(n)), !pos);
        fz.set(f.config_addr(ConfigBit::GclkMux(n, 0)), b0);
        fz.set(f.config_addr(ConfigBit::GclkMux(n, 1)), b1);
    }
    for n in 1..=3u8 {
        fz.set(f.config_addr(ConfigBit::Itd(n)), !opts.itd[n as usize - 1]);
    }
    fz.set(f.config_addr(ConfigBit::TmsPullup), !opts.tms_pullup);
    fz.set(f.config_addr(ConfigBit::TdiPullup), !opts.tdi_pullup);
    fz.set(f.config_addr(ConfigBit::PowerReset), !opts.power_reset);
    fz.fill(f.special_base(), 4, true);
    fz.set(f.config_addr(ConfigBit::Jtag), opts.jtag);
    fz.fill(f.ues_base(), 16, true);

    // macrocells
    for m in 1..=nmc {
        let u = &uses[m];
        let mut bits: BTreeMap<McBit, bool> = BTreeMap::new();
        let mut set = |b: McBit, v: bool| {
            bits.insert(b, v);
        };
        let mc_name = u.cell.map(|i| design.cells[i].name.clone()).unwrap_or_default();
        let low_power = opts.mc_power.contains(&mc_name) || opts.mc_power == Select::All;
        let fast = opts.output_fast == Select::All || u.cell.map_or(false, |i| opts.output_fast.contains(&design.cells[i].name));
        if let Some(i) = u.cell {
            let c = &design.cells[i];
            let pl = &plans[i];
            encode_logic(c, pl, u.pad_used, dev.mc_pin(m as u16).is_some(), &mut set);
            if opts.observable_registers && c.registered() && !c.is_output && pl.hosted.is_none() && !pl.fast_in {
                set(McBit::OMux, false);
            }
            if let Some(t) = u.helper_for {
                // input register + cascade helper in one macrocell: the register takes D from the
                // pad (D/Q fast path), the PTs all go to the sum, the sum to the cascade
                set(McBit::CasMux, true);
                set(McBit::PtPowerOff, false);
                for k in 1..=5 {
                    set(pt_mux_bit(k), false);
                }
                for (k, p) in helper_pts_at(&plans[uses[t as usize].cell.unwrap()], t, m as u16) {
                    write_pt(&mut fz, f, m, k, p, b_of(m), &col_of, fold_mc, &source_of)?;
                }
            }
            set(McBit::Slew, !fast);
            set(McBit::OpenDrain, opts.open_collector.contains(&c.name));
            for (&k, p) in &pl.pts {
                write_pt(&mut fz, f, m, k, p, b_of(m), &col_of, fold_mc, &source_of)?;
            }
            if let Some(p) = fold_at.get(&(m as u16)) {
                set(McBit::Pt1Mux, true);
                write_pt(&mut fz, f, m, 1, p, b_of(m), &col_of, fold_mc, &source_of)?;
            }
            if let (Some(h), Some(ri)) = (&pl.hosted, u.hosted) {
                // vendor encoding of a doubled macrocell (observed in spi / shift-register)
                let r = &design.cells[ri];
                set(McBit::DMux, false);
                set(McBit::Pt2Mux, false);
                set(McBit::FbMux, false);
                set(McBit::XorB, false);
                set(McBit::Storage, r.storage != Storage::Latch);
                let (g0, g1) = match h.clk {
                    ClkUse::Global(1) => (true, true),
                    ClkUse::Global(3) => (false, true),
                    _ => (false, false),
                };
                set(McBit::Gclk0, g0);
                set(McBit::Gclk1, g1);
                set(McBit::GclrMux, !h.ar_global);
                set(McBit::Pt4Func, false);
                set(McBit::Pt4Mux, h.ce_pt);
                set(McBit::Pt3Mux, h.ar_pt || !pl.in_sum.contains(&3));
            }
        } else if let Some(t) = u.helper_for {
            // cascade helper: all PTs into its sum, sum to cascade output
            let pl = &plans[uses[t as usize].cell.unwrap()];
            encode_unused(u.pad_used, &mut set);
            set(McBit::CasMux, true);
            set(McBit::PtPowerOff, false);
            for k in 1..=5 {
                set(pt_mux_bit(k), false);
            }
            for (k, p) in helper_pts_at(pl, t, m as u16) {
                write_pt(&mut fz, f, m, k, p, b_of(m), &col_of, fold_mc, &source_of)?;
            }
        } else if let Some(p) = fold_at.get(&(m as u16)) {
            encode_unused(u.pad_used, &mut set);
            set(McBit::Pt1Mux, true);
            set(McBit::PtPowerOff, false);
            write_pt(&mut fz, f, m, 1, p, b_of(m), &col_of, fold_mc, &source_of)?;
        } else {
            encode_unused(u.pad_used, &mut set);
        }
        set(McBit::LowPower, low_power);
        bits.insert(McBit::Slew, !fast);
        for (b, v) in bits {
            fz.set(f.mc_bit_addr(m, b), v);
        }
    }

    // the placement in name order (the reports' order)
    let placement: Vec<(String, Place)> = place_of.into_iter().collect();
    Ok(FitResult { device: dev, fuses: fz, placement, routing, design })
}

pub(super) fn pt_mux_bit(k: usize) -> McBit {
    match k {
        1 => McBit::Pt1Mux,
        2 => McBit::Pt2Mux,
        3 => McBit::Pt3Mux,
        4 => McBit::Pt4Mux,
        _ => McBit::Pt5Mux,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn write_pt(
    fz: &mut Fuses,
    f: Family,
    mc: usize,
    k: usize,
    p: &[(Signal, bool)],
    b: usize,
    col_of: &BTreeMap<(usize, Source), usize>,
    fold_mc: &BTreeMap<(String, usize), u16>,
    source_of: &dyn Fn(&Signal) -> Result<Source, FitError>,
) -> Result<(), FitError> {
    let a = f.pt_addr(mc, k);
    fz.fill(a, 96, true);
    for (s, pos) in p {
        if let Signal::Fold(k) = s {
            if !pos {
                return err(FitErrorKind::Other, format!("internal: inverted foldback literal {k} in a product term"));
            }
            let Some(&m) = fold_mc.get(&(k.clone(), b)) else {
                return err(
                    FitErrorKind::Other,
                    format!("internal: MC{mc} reads foldback {k}, which has no expander in block {}", atf15xx_arch::block_letter(b)),
                );
            };
            fz.set(a + f.pt_foldback_column((m as usize - 1) % 16), false);
            continue;
        }
        let src = source_of(s)?;
        let Some(&mux) = col_of.get(&(b, src)) else {
            return err(
                FitErrorKind::Other,
                format!("internal: PT{k} of MC{mc} reads {s}, which is not routed into block {}", atf15xx_arch::block_letter(b)),
            );
        };
        fz.set(a + f.pt_input_column(mux, !pos), false);
    }
    Ok(())
}

/// Unused macrocell, or one whose pad is only read as an input.
pub(super) fn encode_unused(pad_used: bool, set: &mut dyn FnMut(McBit, bool)) {
    use McBit::*;
    // observed vendor patterns (see docs/architecture/fitter/04-fuse-map.md)
    let unused = [
        (OeMux0, false),
        (OeMux1, false),
        (Pt5Func, true),
        (OeMux2, false),
        (OMux, true),
        (GclrMux, true),
        (Storage, false),
        (DMux, true),
        (Slew, true),
        (Pt4Func, true),
        (Pt1Mux, false),
        (CasMux, false),
        (Gclk0, false),
        (Gclk1, false),
        (PtPowerOff, true),
        (Pt2Mux, false),
        (XorB, false),
        (Pt5Mux, false),
        (Invert, false),
        (Pt3Mux, false),
        (Pt4Mux, false),
        (OpenDrain, false),
        (FbMux, true),
    ];
    for (b, v) in unused {
        set(b, v);
    }
    if pad_used {
        for (b, v) in [(Pt5Func, false), (Storage, true), (CasMux, true), (Pt5Mux, true), (Pt3Mux, true), (Pt4Mux, true)] {
            set(b, v);
        }
    }
}

pub(super) fn encode_logic(c: &Cell, pl: &PtPlan, pad_used: bool, _bonded: bool, set: &mut dyn FnMut(McBit, bool)) {
    use McBit::*;
    let reg = c.registered();
    // output enable
    let oe_code: u8 = match pl.oe {
        OeUse::Always | OeUse::Pt => 7,
        OeUse::Global(n) => [4, 2, 6, 1, 5, 3][n as usize - 1],
        OeUse::Off => {
            // the vendor's buried-cell encoding is code 7 with `Pt5Func` = 0, i.e. "OE = PT5's
            // special line"; with PT5 steered into the sum that line is (inferred) 0, but
            // decode/simulator readings differ (F2.8 rule 13), so use the unambiguous code 0 then
            if pad_used || pl.in_sum.contains(&5) {
                0
            } else {
                7
            }
        }
    };
    set(OeMux0, oe_code & 1 != 0);
    set(OeMux1, oe_code & 2 != 0);
    set(OeMux2, oe_code & 4 != 0);
    let pt5_func = match pl.oe {
        OeUse::Always => true,
        OeUse::Pt | OeUse::Global(_) | OeUse::Off => pl.ap_pt,
    };
    set(Pt5Func, pt5_func);
    set(OMux, !reg || !c.is_output);
    set(GclrMux, !pl.ar_global);
    set(McBit::Storage, c.storage != crate::netlist::Storage::Latch);
    set(DMux, true);
    let pt4_special = pl.clk == ClkUse::Pt || pl.ce_pt || (!reg && !pl.in_sum.contains(&4));
    set(Pt4Func, if pl.ce_pt { false } else { pl.clk == ClkUse::Pt || !reg });
    set(Pt4Mux, pt4_special);
    set(Pt1Mux, false);
    set(CasMux, pl.constant == Some(true));
    let (g0, g1) = match pl.clk {
        ClkUse::Global(1) => (true, true),
        ClkUse::Global(3) => (false, true),
        _ => (false, false),
    };
    set(Gclk0, g0);
    set(Gclk1, g1);
    // power the PTs down only if none is used: a constant output can still have a PT clock,
    // reset or output enable (e.g. an open-drain `assign irq_n = irq ? 1'b0 : 1'bz`)
    set(PtPowerOff, pl.constant.is_some() && pl.pts.is_empty() && pl.helper_pts.is_empty());
    set(Pt2Mux, pl.xor);
    set(XorB, c.storage == crate::netlist::Storage::Tff);
    set(Pt5Mux, !pl.in_sum.contains(&5));
    set(Invert, pl.invert);
    set(Pt3Mux, !pl.in_sum.contains(&3));
    set(FbMux, !reg);
    if pl.fast_in {
        // D/Q group: d_mux fast + dfast/o_mux pad: FF.D = pad, IO.A = Q (the pin is an input,
        // its OE stays off), feedback = Q
        set(DMux, false);
        set(OMux, false);
    }
}
