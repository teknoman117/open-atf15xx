//! For the OE-code-7 / PT5-in-sum question (docs/worklog/05-verilog-flow.md): print every
//! macrocell of the given vendor JEDs whose OE code is 7 (with OE_SCAN_ALL also every MC whose
//! PT5 is in the sum), with its PT5 routing.
//!   cargo run --release -p atf15xx-fit --example oe_scan -- a.jed b.jed ... > scan.tsv
//! Columns: file, mc, oe code, cascade_out, pt5_func, pt5_mux (1 = special function, 0 = sum), pt5_present, uses_pts.
use atf15xx_arch::decode::decode;
use atf15xx_arch::McBit;
use atf15xx_fit::jed;
fn main() {
    for p in std::env::args().skip(1) {
        let t = String::from_utf8_lossy(&std::fs::read(&p).unwrap()).to_string();
        let Ok(fz) = jed::parse(&t) else { continue };
        let d = decode(fz.family, &fz.bits);
        for m in &d.macrocells {
            let code = m.bit(McBit::OeMux0) as u8 | (m.bit(McBit::OeMux1) as u8) << 1 | (m.bit(McBit::OeMux2) as u8) << 2;
            let all = std::env::var_os("OE_SCAN_ALL").is_some();
            if code != 7 && !(all && !m.bit(McBit::Pt5Mux) && m.pts[4].is_some()) {
                continue;
            }
            println!(
                "{p}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                m.mc,
                code,
                m.cascade_out as u8,
                m.bit(McBit::Pt5Func) as u8,
                m.bit(McBit::Pt5Mux) as u8,
                m.pts[4].is_some() as u8,
                m.uses_pts() as u8
            );
        }
    }
}
