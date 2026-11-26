//! `fit1504`: open replacement for Atmel's `fit1504.exe` (ATF1504AS), CUPL `.tt2` -> JEDEC
//! (see the crate README).
fn main() {
    std::process::exit(atf15xx_fit::cli::main(atf15xx_arch::Family::F1504));
}
