// C++ half of the cxx bridge: walks RTLIL into the shared structs of src/lib.rs and registers the
// `synth_atf15xx` pass, whose body is Rust (`synth_and_fit`). Everything CPLD-specific stays in
// Rust; this file only knows Yosys.
//
// read_design() mirrors backends/json/json.cc (Yosys 0.71) exactly: design->sort(), per-module
// SigMap, bit ids from 2 in the order ports -> cell connections -> wires, constants "0" "1" "x"
// "z", and write_parameter_value's encoding with -compat-int. `synth_atf15xx -check-json`
// compares the result with the JSON backend's output.
#include "atf15xx-synth/src/plugin/rtlil.h"
#include "atf15xx-synth/src/plugin.rs.h"
#include "kernel/sigtools.h"
#include <fstream>
#include <sstream>

USING_YOSYS_NAMESPACE

namespace atf15xx {

namespace {

std::string uname(Design *d, IdString id) { return d->twines().unescaped_str(id); }

Kv kv(Design *d, IdString key, const Const &v) {
    Kv r;
    r.key = uname(d, key);
    r.int_val = 0;
    if ((v.flags & RTLIL::ConstFlags::CONST_FLAG_STRING) != 0) {
        std::string str = v.decode_string();
        int state = 0;
        for (char c : str) {
            if (state == 0) {
                if (c == '0' || c == '1' || c == 'x' || c == 'z')
                    state = 0;
                else if (c == ' ')
                    state = 1;
                else
                    state = 2;
            } else if (state == 1 && c != ' ')
                state = 2;
        }
        if (state < 2) str += " ";
        r.kind = 0;
        r.text = str;
    } else if (GetSize(v) <= 32 && v.is_fully_def()) {
        if ((v.flags & RTLIL::ConstFlags::CONST_FLAG_SIGNED) != 0) {
            r.kind = 1;
            r.int_val = v.as_int();
        } else {
            r.kind = 2;
            r.int_val = (int64_t)(uint32_t)v.as_int();
        }
    } else {
        r.kind = 3;
        r.text = v.as_string();
    }
    return r;
}

rust::Vec<Kv> kvs(Design *d, const dict<IdString, Const> &m) {
    rust::Vec<Kv> out;
    for (auto &it : m) out.push_back(kv(d, it.first, it.second));
    return out;
}

struct Bits {
    SigMap sigmap;
    dict<SigBit, int64_t> ids;
    int64_t next = 2;
    rust::Vec<int64_t> get(const SigSpec &sig) {
        rust::Vec<int64_t> out;
        for (auto bit : sigmap(sig)) {
            auto it = ids.find(bit);
            if (it == ids.end()) {
                int64_t v;
                if (bit.wire == nullptr)
                    v = bit == State::S0 ? -1 : bit == State::S1 ? -2 : bit == State::Sz ? -4 : -3;
                else
                    v = next++;
                ids[bit] = v;
                out.push_back(v);
            } else
                out.push_back(it->second);
        }
        return out;
    }
};

} // namespace

rust::Vec<ModuleRec> read_design(Design &design) {
    Design *d = &design;
    d->sort();
    rust::Vec<ModuleRec> mods;
    for (auto module : d->modules()) {
        if (module->has_processes())
            log_error("Module %s contains processes (run `proc` first).\n", uname(d, module->name).c_str());
        ModuleRec m;
        m.name = uname(d, module->name);
        m.attrs = kvs(d, module->attributes);
        Bits bits;
        bits.sigmap.set(module);
        for (auto n : module->ports) {
            Wire *w = module->wire(n);
            PortRec p;
            p.name = uname(d, n);
            p.dir = w->port_input ? (w->port_output ? "inout" : "input") : "output";
            p.offset = w->start_offset;
            p.upto = w->upto;
            p.signed_ = w->is_signed;
            p.bits = bits.get(w);
            m.ports.push_back(std::move(p));
        }
        for (auto c : module->cells()) {
            if (c->type == ID($scopeinfo)) continue;
            CellRec r;
            r.name = uname(d, c->name);
            r.public_name = c->name.isPublic();
            r.ty = uname(d, c->type);
            r.params = kvs(d, c->parameters);
            r.attrs = kvs(d, c->attributes);
            r.known = c->known();
            for (auto &conn : c->connections()) {
                Conn k;
                k.port = uname(d, conn.first);
                k.dir = c->input(conn.first) ? (c->output(conn.first) ? "inout" : "input") : "output";
                k.bits = bits.get(conn.second);
                r.conns.push_back(std::move(k));
            }
            m.cells.push_back(std::move(r));
        }
        for (auto w : module->wires()) {
            WireRec r;
            r.name = uname(d, w->name);
            r.public_name = w->name.isPublic();
            r.bits = bits.get(w);
            r.offset = w->start_offset;
            r.upto = w->upto;
            r.signed_ = w->is_signed;
            r.attrs = kvs(d, w->attributes);
            m.wires.push_back(std::move(r));
        }
        mods.push_back(std::move(m));
    }
    return mods;
}

rust::String json_text(Design &design) {
    std::stringstream json;
    std::vector<std::string> args = {"json", "-compat-int"};
    Backend::backend_call(&design, &json, "<memory>", args);
    return rust::String(json.str());
}

void run_pass(Design &design, rust::Str command) { Pass::call(&design, std::string(command)); }

size_t annotate(Design &design, rust::Str module, const rust::Vec<Annot> &annots) {
    std::string want(module);
    size_t n = 0;
    for (auto m : design.modules()) {
        if (uname(&design, m->name) != want) continue;
        dict<std::string, Wire *> wires;
        for (auto w : m->wires()) wires[uname(&design, w->name)] = w;
        for (auto &a : annots) {
            auto it = wires.find(std::string(a.wire));
            if (it == wires.end()) continue;
            it->second->set_string_attribute(design.twines().add("\\" + std::string(a.key)), std::string(a.value));
            n++;
        }
    }
    return n;
}

void log_message(rust::Str text) { log("%s", std::string(text).c_str()); }

namespace {
struct SynthAtf15xxPass : public Pass {
    SynthAtf15xxPass() : Pass("synth_atf15xx", "synthesise and fit for Microchip ATF15xx CPLDs (Rust core, cxx bridge)") {}
    void help() override {
        log("\n    synth_atf15xx [-top <module>] -part <part> [-c <constraints.acf>] -o <dir>\n");
        log("                  [-fmax <MHz>] [-grade <-10>] [-no-toggles] [-option KEY=VALUE]...\n");
        log("                  [-map auto|gates|words] [-check-json] [-no-annotate]\n\n");
        log("-map: auto (default) maps the design both ways from the same RTLIL (Yosys gates, and\n");
        log("Yosys word-level cells lowered by the Rust flow), fits both and keeps the better fit.\n\n");
        log("Run the ATF15xx synthesis script, read the netlist straight from RTLIL, fit it with the\n");
        log("open ATF15xx fitter, write <top>.jed, .svf, .fit, .pin, .pins.json, .map.json and\n");
        log(".timing.rpt into <dir>, and annotate each placed wire with atf15xx_mc / atf15xx_pin.\n");
        log("-check-json also builds the netlist through Yosys's JSON backend and requires both to\n");
        log("be identical (a self-check of the bridge).\n\n");
    }
    void execute(std::vector<std::string> args, RTLIL::Design *design) override {
        FitArgs a;
        a.out_dir = "out";
        a.grade = "-10";
        a.fmax = 0.0;
        a.check_json = false;
        a.annotate = true;
        a.toggles = true;
        a.map = "auto";
        size_t i;
        for (i = 1; i < args.size(); i++) {
            auto more = [&]() { return i + 1 < args.size(); };
            if (args[i] == "-top" && more()) { a.top = args[++i]; continue; }
            if (args[i] == "-part" && more()) { a.part = args[++i]; continue; }
            if (args[i] == "-c" && more()) {
                std::ifstream f(args[++i]);
                if (!f) log_error("cannot read %s\n", args[i].c_str());
                std::stringstream s;
                s << f.rdbuf();
                a.constraints = s.str();
                continue;
            }
            if (args[i] == "-o" && more()) { a.out_dir = args[++i]; continue; }
            if (args[i] == "-fmax" && more()) { a.fmax = atof(args[++i].c_str()); continue; }
            if (args[i] == "-grade" && more()) { a.grade = args[++i]; continue; }
            if (args[i] == "-check-json") { a.check_json = true; continue; }
            if (args[i] == "-no-annotate") { a.annotate = false; continue; }
            if (args[i] == "-no-toggles") { a.toggles = false; continue; }
            if (args[i] == "-map" && more()) { a.map = args[++i]; continue; }
            if (args[i] == "-option" && more()) { a.options.push_back(rust::String(args[++i])); continue; }
            break;
        }
        extra_args(args, i, design);
        log_header(design, "Executing SYNTH_ATF15XX pass (Rust core).\n");
        log_push();
        try {
            rust::String summary = synth_and_fit(*design, a);
            log_pop();
            log("%s\n", summary.c_str());
        } catch (const rust::Error &e) {
            log_pop();
            log_error("synth_atf15xx: %s\n", e.what());
        }
    }
} SynthAtf15xxPass;
} // namespace

} // namespace atf15xx
