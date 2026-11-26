//! In-circuit debugging of ATF15xx CPLDs over JTAG boundary scan (formerly the `atf15xx-dbg` crate).
//!
//! * [`part`]: the boundary register of a part, from the Atmel BSDL files (shipped with the
//!   prjbureau submodule; read at run time, never copied) and `atf15xx-arch`'s pin map;
//! * [`jtag`]: a transport trait, OpenOCD through its Tcl RPC server, and the boundary-scan
//!   operations (IDCODE, SAMPLE, EXTEST, BYPASS);
//! * [`design`]: naming a snapshot with the fitter's `<top>.map.json`;
//! * [`snapshot`]: decoding, printing, change detection and VCD capture;
//! * [`session`]: scripted EXTEST sessions (driving a design's inputs step by step).
//!
//! What SAMPLE shows on an ATF15xx (spec: docs/architecture/debug/01-boundary-scan.md): every
//! pad, and for every macrocell (with or without a pin) its output-enable and output-mux values,
//! i.e. the register Q of a registered output, while the device keeps running. There is no
//! INTEST: registers cannot be written, only pins driven (EXTEST).

pub mod design;
pub mod jtag;
pub mod part;
pub mod session;
pub mod snapshot;

pub use design::{DesignMap, Signal};
pub use jtag::{Jtag, OpenOcd, OpenOcdConfig, Scanner};
pub use part::{Cell, CellKind, Part};
