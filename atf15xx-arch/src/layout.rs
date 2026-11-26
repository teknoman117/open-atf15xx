// Macrocell-configuration layout tables shared by the library and its build script
// (`build/main.rs` includes this file), so the generator and the accessors use one source.

/// Field pairs whose macrocell slot runs forward (slot = local index) on 1504/1508; reversed on 1502.
pub const FORWARD_PAIRS: [(u8, u8); 6] = [(0, 1), (4, 23), (6, 9), (7, 19), (12, 16), (18, 20)];

/// For every macrocell bit, in `McBit::ALL` order: its name, the S-label pair (lower, higher) of
/// its 32-fuse field, `k` (which of the macrocell's two bits of that field, after undoing the
/// per-column parity swap) and the prjbureau option that holds it.
pub const MC_BIT_LAYOUT: [(&str, (u8, u8), bool, &str); 24] = [
    ("OeMux0", (0, 1), false, "oe_mux"),
    ("OeMux1", (0, 1), true, "oe_mux"),
    ("Pt5Func", (2, 17), false, "pt5_func"),
    ("OeMux2", (2, 17), true, "oe_mux"),
    ("OMux", (3, 15), false, "dfast_mux"),
    ("GclrMux", (3, 15), true, "gclr_mux"),
    ("Storage", (4, 23), false, "storage"),
    ("DMux", (4, 23), true, "d_mux"),
    ("Slew", (5, 22), false, "slew_rate"),
    ("Pt4Func", (5, 22), true, "pt4_func"),
    ("Pt1Mux", (6, 9), false, "pt1_mux"),
    ("CasMux", (6, 9), true, "cas_mux"),
    ("Gclk0", (7, 19), false, "gclk_mux"),
    ("Gclk1", (7, 19), true, "gclk_mux"),
    ("LowPower", (8, 21), false, "low_power"),
    ("PtPowerOff", (8, 21), true, "pt_power"),
    ("Pt2Mux", (10, 13), false, "pt2_mux"),
    ("XorB", (10, 13), true, "xor_b_mux"),
    ("Pt5Mux", (11, 14), false, "pt5_mux"),
    ("Invert", (11, 14), true, "reset"),
    ("Pt3Mux", (12, 16), false, "pt3_mux"),
    ("Pt4Mux", (12, 16), true, "pt4_mux"),
    ("OpenDrain", (18, 20), false, "output_driver"),
    ("FbMux", (18, 20), true, "fb_mux"),
];
