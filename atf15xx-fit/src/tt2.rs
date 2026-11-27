//! CUPL `.tt2` reader: Berkeley PLA with CUPL `#$` headers.
//!
//! ```text
//! #$ DEVICE  f1504plcc44
//! #$ PINS  3 A+:4 B+ Y+:12          name, '+'/'-' (declared polarity), optional ':pin'
//! #$ NODES 1 N+ [N+:601]             buried nodes, optional ':node-number'
//! #$ PROPERTY ATMEL <key> <value...>
//! .i 2 / .o 1 / .type f
//! .ilb A B            inputs; suffixes .Q (register feedback) .PIN (pad of an output)
//! .ob  Y Y.OE         outputs; suffixes .D .REG .T .L .LE .C .CE .AR .AP .OE, trailing '-' = active low
//! .phase 1
//! .p 1
//! 11 1                cube over inputs ('0' '1' '-') and output mask
//! .e
//! ```

#[derive(Clone, Debug, Default)]
pub struct Pla {
    pub device: String,
    pub title: String,
    /// `(name, active_low, pin)`
    pub pins: Vec<(String, bool, Option<u16>)>,
    /// `(name, active_low, node number)`
    pub nodes: Vec<(String, bool, Option<u16>)>,
    pub properties: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub phase: Vec<bool>,
    /// `(cube over inputs: Some(true)=1, Some(false)=0, None='-', outputs it feeds)`
    pub cubes: Vec<(Vec<Option<bool>>, Vec<usize>)>,
}

/// A `.tt2` syntax error: line number and message.
#[derive(Debug, thiserror::Error)]
#[error("line {0}: {1}")]
pub struct ParseError(pub usize, pub String);

fn decl(tok: &str) -> (String, bool, Option<u16>) {
    let (n, num) = match tok.split_once(':') {
        Some((a, b)) => (a, b.parse().ok()),
        None => (tok, None),
    };
    let (name, low) = if let Some(s) = n.strip_suffix('+') {
        (s, false)
    } else if let Some(s) = n.strip_suffix('-') {
        (s, true)
    } else {
        (n, false)
    };
    (name.to_string(), low, num)
}

impl Pla {
    pub fn parse(text: &str) -> Result<Pla, ParseError> {
        let mut p = Pla::default();
        let mut ni = None;
        let mut no = None;
        for (ln, raw) in text.lines().enumerate() {
            let line = raw.trim();
            let err = |m: &str| ParseError(ln + 1, m.to_string());
            if line.is_empty() {
                continue;
            }
            if let Some(h) = line.strip_prefix("#$") {
                let mut it = h.split_whitespace();
                match it.next() {
                    Some("DEVICE") => p.device = it.next().unwrap_or("").to_string(),
                    Some("TITLE") => p.title = it.collect::<Vec<_>>().join(" "),
                    Some("PINS") => {
                        it.next();
                        p.pins.extend(it.map(decl));
                    }
                    Some("NODES") => {
                        it.next();
                        p.nodes.extend(it.map(decl));
                    }
                    Some("PROPERTY") => p.properties.push(it.collect::<Vec<_>>().join(" ")),
                    _ => {}
                }
                continue;
            }
            if line.starts_with('#') {
                continue;
            }
            if let Some(d) = line.strip_prefix('.') {
                let mut it = d.split_whitespace();
                match it.next() {
                    Some("i") => ni = it.next().and_then(|s| s.parse::<usize>().ok()),
                    Some("o") => no = it.next().and_then(|s| s.parse::<usize>().ok()),
                    Some("ilb") => p.inputs = it.map(String::from).collect(),
                    Some("ob") => p.outputs = it.map(String::from).collect(),
                    Some("phase") => p.phase = it.next().unwrap_or("").chars().map(|c| c == '1').collect(),
                    Some("p") | Some("type") => {}
                    Some("e") => break,
                    _ => {}
                }
                continue;
            }
            let mut it = line.split_whitespace();
            let (ins, outs) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
            let cube = ins
                .chars()
                .map(|c| match c {
                    '1' => Ok(Some(true)),
                    '0' => Ok(Some(false)),
                    '-' | '~' | '2' => Ok(None),
                    _ => Err(err("bad cube character")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            if cube.len() != p.inputs.len() {
                return Err(err("cube width does not match .ilb"));
            }
            let o: Vec<usize> = outs.chars().enumerate().filter(|(_, c)| *c == '1' || *c == '4').map(|(i, _)| i).collect();
            if outs.len() != p.outputs.len() {
                return Err(err("output width does not match .ob"));
            }
            p.cubes.push((cube, o));
        }
        if let Some(n) = ni {
            if n != p.inputs.len() {
                return Err(ParseError(0, format!(".i {} but {} input labels", n, p.inputs.len())));
            }
        }
        if let Some(n) = no {
            if n != p.outputs.len() {
                return Err(ParseError(0, format!(".o {} but {} output labels", n, p.outputs.len())));
            }
        }
        if p.phase.is_empty() {
            p.phase = vec![true; p.outputs.len()];
        }
        Ok(p)
    }

    /// The `#$ PROPERTY ATMEL` entries as `(key, value)`.
    pub fn atmel_properties(&self) -> Vec<(String, String)> {
        self.properties
            .iter()
            .filter_map(|p| p.strip_prefix("ATMEL "))
            .map(|p| {
                let mut it = p.splitn(2, char::is_whitespace);
                (it.next().unwrap_or("").to_string(), it.next().unwrap_or("").trim().to_string())
            })
            .collect()
    }
}
