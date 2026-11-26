//! The Yosys netlist as typed structs (spec F9.7 item 1): the schema of `write_json`, filled
//! either by `serde` from a JSON file ([`Netlist::from_json`]) or directly from RTLIL by the Yosys
//! plugin (`crate::plugin`). The importer (`crate::yosys`) reads only these types.
//!
//! Maps are `BTreeMap`s, so iteration is in name order, as with `serde_json`'s default map: the
//! importer's choices (names, cell order) are the same whichever producer filled the netlist.

use serde::Deserialize;
use std::collections::BTreeMap;

/// A net bit: integer id or a constant (`'0'`, `'1'`, `'x'`, `'z'`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "RawBit")]
pub enum Bit {
    Net(u64),
    Const(char),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawBit {
    Net(u64),
    Const(String),
}

impl TryFrom<RawBit> for Bit {
    type Error = String;
    fn try_from(r: RawBit) -> Result<Bit, String> {
        Ok(match r {
            RawBit::Net(n) => Bit::Net(n),
            RawBit::Const(s) => Bit::Const(s.chars().next().unwrap_or('x')),
        })
    }
}

/// A parameter or attribute value: Yosys writes integers as numbers, everything else as strings
/// (bit vectors as MSB-first `0`/`1` strings).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Const {
    Int(i64),
    Str(String),
    /// Anything else (never written by Yosys; kept so that a file with one still loads).
    Other(serde::de::IgnoredAny),
}

impl Const {
    /// As an unsigned integer; a string is read as a bit vector.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Const::Int(i) => u64::try_from(*i).ok(),
            Const::Str(s) if !s.is_empty() && s.chars().all(|c| c == '0' || c == '1') => {
                s.chars().try_fold(0u64, |acc, c| acc.checked_mul(2).map(|a| a + u64::from(c == '1')))
            }
            _ => None,
        }
    }

    /// Little-endian bits of an unsigned parameter (a number gives 64 bits).
    pub fn bits(&self) -> Result<Vec<bool>, String> {
        match self {
            Const::Int(i) => {
                let x = u64::try_from(*i).map_err(|_| "negative parameter".to_string())?;
                Ok((0..64).map(|k| (x >> k) & 1 == 1).collect())
            }
            Const::Str(s) => Ok(s.chars().rev().map(|c| c == '1').collect()),
            Const::Other(_) => Err("bad parameter".into()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Netlist {
    #[serde(default)]
    pub modules: BTreeMap<String, Module>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Module {
    #[serde(default)]
    pub attributes: BTreeMap<String, Const>,
    /// `None` when the file has no `ports` object (an error for the importer).
    pub ports: Option<BTreeMap<String, Port>>,
    #[serde(default)]
    pub cells: BTreeMap<String, Cell>,
    #[serde(default)]
    pub netnames: BTreeMap<String, NetName>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Port {
    #[serde(default = "input")]
    pub direction: String,
    pub bits: Vec<Bit>,
}

fn input() -> String {
    "input".into()
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Cell {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, Const>,
    #[serde(default)]
    pub attributes: BTreeMap<String, Const>,
    /// `None` for cells of unknown type (an error for the importer).
    pub port_directions: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub connections: BTreeMap<String, Vec<Bit>>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct NetName {
    #[serde(default)]
    pub hide_name: u64,
    pub bits: Vec<Bit>,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub upto: i64,
    #[serde(default)]
    pub attributes: BTreeMap<String, Const>,
}

impl Netlist {
    pub fn from_json(text: &str) -> Result<Netlist, String> {
        serde_json::from_str(text).map_err(|e| format!("JSON: {e}"))
    }
}

impl Cell {
    /// The bits of port `p` (empty when unconnected).
    pub fn port(&self, p: &str) -> &[Bit] {
        self.connections.get(p).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_yosys_schema() {
        let n = Netlist::from_json(
            r#"{"modules": {"top": {"attributes": {"top": "00000000000000000000000000000001"},
                "ports": {"a": {"direction": "input", "bits": [2, "0"]}},
                "cells": {"$1": {"type": "$sop", "parameters": {"WIDTH": 2, "TABLE": "0110"},
                    "port_directions": {"A": "input", "Y": "output"},
                    "connections": {"A": [2, 3], "Y": [4]}}},
                "netnames": {"a": {"hide_name": 0, "bits": [2, "0"], "offset": 3,
                    "attributes": {"init": "01", "src": "t.v:1"}}}}}}"#,
        )
        .unwrap();
        let m = &n.modules["top"];
        assert_eq!(m.attributes["top"].as_u64(), Some(1));
        assert_eq!(m.ports.as_ref().unwrap()["a"].bits, vec![Bit::Net(2), Bit::Const('0')]);
        let c = &m.cells["$1"];
        assert_eq!(c.port("A"), &[Bit::Net(2), Bit::Net(3)]);
        assert!(c.port("B").is_empty());
        assert_eq!(c.parameters["WIDTH"].bits().unwrap()[1], true);
        assert_eq!(c.parameters["TABLE"].bits().unwrap(), vec![false, true, true, false]);
        assert_eq!(m.netnames["a"].offset, 3);
        assert_eq!(m.netnames["a"].attributes["init"], Const::Str("01".into()));
    }
}
