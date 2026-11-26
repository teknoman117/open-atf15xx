//! Fuse addresses for test tooling (tools/sim/tests/make_broken.py, power-up experiments):
//!   mcbit_addr 1508 29 Invert   -> address of a macrocell configuration bit (McBit name)
//!   mcbit_addr 1508 29 PT       -> start addresses of the macrocell's five 96-fuse PT rows
use atf15xx_arch::{Family, McBit};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 4 {
        eprintln!("usage: mcbit_addr 1502|1504|1508 MC (McBit name | PT)");
        std::process::exit(2);
    }
    let fam = Family::ALL.into_iter().find(|f| f.to_string().contains(&a[1])).expect("family");
    let mc: usize = a[2].parse().expect("macrocell number");
    if a[3] == "PT" {
        let rows: Vec<String> = (1..=5).map(|k| fam.pt_addr(mc, k).to_string()).collect();
        println!("{}", rows.join(" "));
        return;
    }
    let bit = McBit::ALL.iter().copied().find(|b| format!("{b:?}") == a[3]).expect("bit name");
    println!("{}", fam.mc_bit_addr(mc, bit));
}
