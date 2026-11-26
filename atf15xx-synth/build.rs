//! With feature `yosys`: compiles the C++ half of the plugin's cxx bridge against the Yosys
//! headers (`yosys-config --cxxflags`). Without it: nothing.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "yosys")]
    {
        use std::process::Command;
        let out = Command::new("yosys-config")
            .arg("--cxxflags")
            .output()
            .expect("yosys-config not found: the `yosys` feature builds inside the Yosys image (tools/yosys-plugin/build.sh)");
        let flags = String::from_utf8(out.stdout).expect("yosys-config output");
        let mut b = cxx_build::bridge("src/plugin.rs");
        b.file("src/plugin/rtlil.cc");
        for f in flags.split_whitespace() {
            b.flag(f);
        }
        b.flag_if_supported("-Wno-unused-parameter");
        b.compile("atf15xx_yosys_bridge");
        for f in ["src/plugin.rs", "src/plugin/rtlil.cc", "src/plugin/rtlil.h"] {
            println!("cargo:rerun-if-changed={f}");
        }
    }
}
