//! The boundary register of every part, from Atmel's BSDL files in prjbureau (`bsdl/`), checked
//! against the devices. The BSDL files are read at build time only; what is generated is the cell
//! map (`crate::bsdl`).
//!
//! The formal BSDL content (cells, ports, pin map, opcodes, IDCODE, control-cell links) comes from
//! the `bsdl` crate. What the formal content does not say is which macrocell a cell belongs to
//! when the macrocell has no pin: the Atmel files state it only in a comment before each cell
//! (`--Internal, MC124`, `--Input, pin81 MC128`). Those annotations are read too, and every cell
//! is cross-checked against the device (pin ↔ macrocell, dedicated inputs), so a wrong file,
//! package or device table is reported instead of silently mislabelling pins.
//!
//! Cell numbering follows the BSDL: cell 0 is the one nearest TDO, i.e. shifted out first.

use crate::devices::Dev;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    In,
    Oe,
    Out,
}

struct Cell {
    bit: usize,
    kind: Kind,
    mc: Option<u16>,
    dedicated: Option<&'static str>,
    pin: Option<u16>,
}

pub struct Part {
    name: String,
    device: String,
    file: String,
    ir_len: usize,
    opcodes: Vec<(String, u64)>,
    idcodes: Vec<u32>,
    length: usize,
    cells: Vec<Cell>,
    notes: Vec<String>,
    jtag_pins: [u16; 4],
    max_tck_khz: Option<u32>,
}

/// Atmel's BSDL naming: `1508AS_J84.BSD` (J = PLCC, A = TQFP, Q = PQFP), any case.
fn bsdl_file(files: &[PathBuf], dev: &Dev) -> Option<PathBuf> {
    let (letters, digits): (String, String) = dev.package.chars().partition(|c| c.is_ascii_alphabetic());
    let code = match letters.as_str() {
        "PLCC" => "J",
        "TQFP" => "A",
        "PQFP" => "Q",
        _ => return None,
    };
    let want = format!("{}AS_{code}{digits}.bsd", dev.family);
    files.iter().find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&want))).cloned()
}

pub fn bsd_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e} (is the prjbureau submodule checked out?)", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("bsd")))
        .collect();
    files.sort();
    files
}

fn part(dev: &Dev, file: &Path) -> Result<Part, String> {
    let fname = file.file_name().unwrap().to_string_lossy().to_string();
    let text = std::fs::read(file).map_err(|e| format!("{fname}: {e}"))?;
    let text = String::from_utf8_lossy(&text);
    let entity = bsdl::Entity::parse(file, None).map_err(|e| format!("{fname}: {e:?}"))?;
    let annotations = annotations(&text);
    let bsr = entity.boundary_register();
    if bsr.len() != entity.boundary_length() {
        return Err(format!("{fname}: {} cells, BOUNDARY_LENGTH {}", bsr.len(), entity.boundary_length()));
    }
    let ports = entity.ports();
    let port_pin = |i: usize| -> Option<u16> { ports.get(i).and_then(|p| p.pin.parse().ok()) };
    // control cell -> its output cell
    let mut output_of: BTreeMap<usize, usize> = BTreeMap::new();
    for (i, c) in bsr.iter().enumerate() {
        if let Some(cc) = c.ccell {
            output_of.insert(cc, i);
        }
    }
    let mut cells = Vec::with_capacity(bsr.len());
    let mut pairs: BTreeMap<u16, usize> = BTreeMap::new();
    let mut problems = vec![];
    let mut notes = vec![];
    // the macrocell of a pin: an I/O pin, else an input-only pin the BSDL treats as I/O
    let mc_of_pin = |p: u16, notes: &mut Vec<String>, output: bool| -> Option<u16> {
        dev.pin_mc(p).or_else(|| {
            let mc = dev.input_only_mc(p)?;
            if output {
                let n = format!("pin {p}: the BSDL has an output cell for MC{mc} here; the device table has the pin input-only");
                if !notes.contains(&n) {
                    notes.push(n);
                }
            }
            Some(mc)
        })
    };
    for (i, c) in bsr.iter().enumerate() {
        let note = annotations.get(&i);
        let note_mc = note.and_then(|n| n.mc);
        let mut pin = c.port.and_then(port_pin);
        if pin.is_none() {
            // a control cell takes the pin of the output it enables
            pin = output_of.get(&i).and_then(|&o| bsr[o].port).and_then(port_pin);
        }
        // Where a cell has a pin, the formal pin map (and the device table) decides; a comment
        // that disagrees is a typo in the file (e.g. 1508AS_Q160: two `MC59` in a row) and noted.
        if let (Some(p), Some(np)) = (pin, note.and_then(|n| n.pin)) {
            if p != np {
                notes.push(format!("cell {i}: on pin {p}, its comment says pin {np}"));
            }
        }
        if c.ctype == bsdl::CellType::BC(4) {
            // pad input (or the input cell of a pinless macrocell)
            let dedicated = pin.and_then(|p| {
                if p == dev.gclk1 {
                    Some("Gclk1")
                } else if p == dev.gclk2_oe2 {
                    Some("Gclk2")
                } else if p == dev.gclr {
                    Some("Gclr")
                } else if p == dev.oe1 {
                    Some("Oe1")
                } else {
                    None
                }
            });
            let mc = match (dedicated, pin) {
                (Some(_), _) => None,
                (None, Some(p)) => Some(
                    mc_of_pin(p, &mut notes, false)
                        .ok_or_else(|| format!("{fname}: cell {i} on pin {p}, which is no I/O pin of {}", dev.name))?,
                ),
                (None, None) => Some(note_mc.ok_or_else(|| format!("{fname}: cell {i} has no port and no MC annotation"))?),
            };
            if let (Some(m), Some(nm)) = (mc, note_mc) {
                if m != nm {
                    notes.push(format!("cell {i}: pin {} is MC{m}, its comment says MC{nm}", pin.unwrap_or(0)));
                }
            }
            cells.push(Cell { bit: i, kind: Kind::In, mc, dedicated, pin });
        } else {
            // the (output enable, output) pair of a macrocell: control + output3 for a pin,
            // two internal cells for a pinless macrocell (enable first)
            let mc = match pin {
                Some(p) => mc_of_pin(p, &mut notes, true).ok_or_else(|| format!("{fname}: cell {i} on pin {p}, no I/O pin"))?,
                None => note_mc.ok_or_else(|| format!("{fname}: cell {i} has no port and no MC annotation"))?,
            };
            if let (Some(p), Some(nm)) = (pin, note_mc) {
                if nm != mc {
                    notes.push(format!("cell {i}: pin {p} is MC{mc}, its comment says MC{nm}"));
                }
            }
            let kind = match c.function {
                bsdl::CellFunction::Control => Kind::Oe,
                bsdl::CellFunction::Output3 | bsdl::CellFunction::Output2 | bsdl::CellFunction::Bidir => Kind::Out,
                _ => {
                    let k = pairs.entry(mc).or_insert(0);
                    *k += 1;
                    if *k == 1 {
                        Kind::Oe
                    } else {
                        Kind::Out
                    }
                }
            };
            cells.push(Cell { bit: i, kind, mc: Some(mc), dedicated: None, pin });
        }
    }
    // macrocells whose pad is a JTAG pin: the BSDL gives their cells no port (the TAP owns the
    // pad); attach the package pin for the views (they are never driven)
    for c in cells.iter_mut() {
        if let (None, Some(mc)) = (c.pin, c.mc) {
            if let Some(p) = dev.mc_pin(mc).filter(|p| dev.jtag.contains(p)) {
                c.pin = Some(p);
            }
        }
    }
    // every macrocell: one oe and one out cell, at most one input cell; pins agree with the device
    for mc in 1..=dev.macrocells() {
        for kind in [Kind::In, Kind::Oe, Kind::Out] {
            let n = cells.iter().filter(|c| c.mc == Some(mc) && c.kind == kind).count();
            if n > 1 || (n == 0 && kind != Kind::In) {
                problems.push(format!("MC{mc}: {n} {kind:?} cells"));
            }
        }
        let pins: Vec<_> = cells.iter().filter(|c| c.mc == Some(mc)).filter_map(|c| c.pin).collect();
        let want = dev.mc_pin(mc).or_else(|| pins.first().copied().filter(|&p| dev.input_only_mc(p) == Some(mc)));
        if pins.iter().any(|&p| Some(p) != want) {
            problems.push(format!("MC{mc}: BSDL pins {pins:?}, device pin {want:?}"));
        }
    }
    if !problems.is_empty() {
        return Err(format!("{fname} does not match {}:\n  {}", dev.name, problems.join("\n  ")));
    }
    let mut opcodes: Vec<(String, u64)> = entity.opcodes().iter().map(|(k, v)| (k.clone(), *v)).collect();
    opcodes.sort();
    let idcodes = entity.idcode_register().iter().flat_map(|id| expand_x(&id.to_string())).collect();
    Ok(Part {
        name: format!("ATF{}AS-{}", dev.family, dev.package),
        device: dev.name.clone(),
        file: fname,
        ir_len: entity.instruction_length(),
        opcodes,
        idcodes,
        length: bsr.len(),
        cells,
        notes,
        jtag_pins: dev.jtag,
        max_tck_khz: entity.maxtck_hz().map(|hz| (hz / 1000.0) as u32),
    })
}

#[derive(Clone, Copy, Debug, Default)]
struct Note {
    pin: Option<u16>,
    mc: Option<u16>,
}

/// The `--Input, pin81 MC128` / `--Internal, MC124` comment before each boundary cell, by cell.
/// The files are hand-written: `pin76, MC120`, `pin 73 MC115` and `MC112 TDO` all occur.
fn annotations(text: &str) -> BTreeMap<usize, Note> {
    let mut out = BTreeMap::new();
    let mut pending: Option<Note> = None;
    let mut in_register = false;
    for line in text.lines() {
        let s = line.trim();
        if s.contains("BOUNDARY_REGISTER") {
            in_register = true;
            continue;
        }
        if !in_register {
            continue;
        }
        if let Some(rest) = s.strip_prefix("--") {
            let rest = rest.trim_start();
            if let Some((kind, what)) = rest.split_once(',') {
                let note = parse_note(what);
                if note.mc.is_some() || ["Input", "Internal", "I/O"].iter().any(|k| kind.trim().starts_with(k)) {
                    pending = Some(note);
                }
            }
            continue;
        }
        if let Some(rest) = s.strip_prefix('"') {
            let num: String = rest.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
            if let (Ok(i), Some(n)) = (num.parse::<usize>(), pending) {
                out.insert(i, n);
            }
            if s.ends_with(';') {
                break;
            }
        }
    }
    out
}

fn parse_note(what: &str) -> Note {
    let mut note = Note::default();
    let w = what.replace(',', " ");
    let mut toks = w.split_whitespace().peekable();
    while let Some(t) = toks.next() {
        let t = t.to_ascii_lowercase();
        if let Some(n) = t.strip_prefix("pin") {
            note.pin = if n.is_empty() { toks.next().and_then(|x| x.parse().ok()) } else { n.parse().ok() };
        } else if let Some(n) = t.strip_prefix("mc") {
            note.mc = n.parse().ok();
        }
    }
    note
}

/// `0000000101010000100X000000111111 = 0x...` -> every value the `X` bits allow.
fn expand_x(s: &str) -> Vec<u32> {
    let bits: String = s.split_whitespace().next().unwrap_or("").chars().filter(|c| "01xX".contains(*c)).collect();
    let mut vals = vec![0u32];
    for c in bits.chars() {
        vals = match c {
            '0' => vals.iter().map(|v| v << 1).collect(),
            '1' => vals.iter().map(|v| v << 1 | 1).collect(),
            _ => vals.iter().flat_map(|v| [v << 1, v << 1 | 1]).collect(),
        };
    }
    vals
}

/// Rust source for `PARTS`, `PART_ERRORS` and `BSDL_IDCODES`.
pub fn emit(dir: &Path, devs: &[Dev]) -> String {
    let files = bsd_files(dir);
    let mut parts = vec![];
    let mut errors = vec![];
    for d in devs {
        let name = format!("ATF{}AS-{}", d.family, d.package);
        match bsdl_file(&files, d) {
            None => errors.push((name.clone(), format!("no BSDL file for {name} ({}) in prjbureau's bsdl/", d.name))),
            Some(f) => match part(d, &f) {
                Ok(p) => parts.push(p),
                Err(e) => errors.push((name, e)),
            },
        }
    }
    let mut o = String::from("pub(crate) static PARTS: &[Part] = &[\n");
    for p in &parts {
        let cells: Vec<String> = p
            .cells
            .iter()
            .map(|c| {
                format!(
                    "Cell {{ bit: {}, kind: CellKind::{:?}, mc: {:?}, dedicated: {}, pin: {:?} }}",
                    c.bit,
                    c.kind,
                    c.mc,
                    c.dedicated.map_or("None".into(), |d| format!("Some(DedicatedPin::{d})")),
                    c.pin
                )
            })
            .collect();
        o += &format!(
            "    Part {{ name: {:?}, device: {:?}, bsdl_file: {:?}, ir_len: {}, opcodes: &[{}], idcodes: &[{}], length: {}, \
             cells: &[\n        {}], notes: &[{}], jtag_pins: {:?}, max_tck_khz: {:?} }},\n",
            p.name,
            p.device,
            p.file,
            p.ir_len,
            p.opcodes.iter().map(|(k, v)| format!("({k:?}, {v:#x})")).collect::<Vec<_>>().join(", "),
            p.idcodes.iter().map(|v| format!("{v:#010x}")).collect::<Vec<_>>().join(", "),
            p.length,
            cells.join(",\n        "),
            p.notes.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", "),
            p.jtag_pins,
            p.max_tck_khz,
        );
    }
    o += "];\n\npub(crate) static PART_ERRORS: &[(&str, &str)] = &[\n";
    for (n, e) in &errors {
        o += &format!("    ({n:?}, {e:?}),\n");
    }
    o += "];\n\n/// Every BSDL file and the IDCODEs it allows (identifies a die: device and voltage, not package).\npub(crate) static BSDL_IDCODES: &[(&str, &[u32])] = &[\n";
    for f in &files {
        if let Ok(e) = bsdl::Entity::parse(f, None) {
            let ids: Vec<String> =
                e.idcode_register().iter().flat_map(|i| expand_x(&i.to_string())).map(|v| format!("{v:#010x}")).collect();
            o += &format!("    ({:?}, &[{}]),\n", f.file_name().unwrap().to_string_lossy(), ids.join(", "));
        }
    }
    o + "];\n"
}
