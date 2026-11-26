//! Rewrite JEDEC files in our own layout (`jed::write`), keeping the fuses, the security fuse, the
//! `QP` pin count and the `Created` date: `cargo run --example jed_rewrite -- FILE.jed...`.
//! Each file is checked to parse back to the same fuses before it is replaced.
use atf15xx_fit::jed;
fn main() {
    let mut n = 0;
    for p in std::env::args().skip(1) {
        let t = String::from_utf8_lossy(&std::fs::read(&p).unwrap()).to_string();
        let fz = jed::parse(&t).unwrap_or_else(|e| panic!("{p}: {e}"));
        let out = jed::write(&fz, jed::created_on(&t).unwrap_or(""), jed::pin_count(&t).unwrap_or(0));
        assert_eq!(jed::parse(&out).unwrap(), fz, "{p}: fuses changed");
        std::fs::write(&p, out).unwrap();
        n += 1;
    }
    println!("rewrote {n} files");
}
