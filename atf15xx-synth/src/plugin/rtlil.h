// C++ half of the cxx bridge (atf15xx-synth/src/plugin.rs): RTLIL access for the Rust flow.
#pragma once
#include "kernel/yosys.h"
#include "rust/cxx.h"

namespace atf15xx {
using Design = Yosys::RTLIL::Design;
struct ModuleRec;
struct Annot;

// The design's modules in the Yosys JSON backend's form (same bit numbering and value encoding as
// `write_json -compat-int`), after `design->sort()`.
rust::Vec<ModuleRec> read_design(Design &design);
// `write_json -compat-int` of the design, in memory (the bridge's self-check).
rust::String json_text(Design &design);
// Run a Yosys command (`Pass::call`).
void run_pass(Design &design, rust::Str command);
// Set string attributes on wires of `module`; returns how many were set.
size_t annotate(Design &design, rust::Str module, const rust::Vec<Annot> &annots);
void log_message(rust::Str text);
} // namespace atf15xx
