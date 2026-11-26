// Feasibility probe for an in-Yosys ATF15xx pass: per cell, report the word-level origin.
#include "kernel/yosys.h"
#include "kernel/sigtools.h"
USING_YOSYS_NAMESPACE
PRIVATE_NAMESPACE_BEGIN
struct Atf15xxProbe : public Pass {
    Atf15xxProbe() : Pass("atf15xx_probe", "feasibility probe: count cells and FFs per source word") {}
    void execute(std::vector<std::string> args, RTLIL::Design *design) override {
        extra_args(args, 1, design);
        for (auto mod : design->selected_modules()) {
            dict<std::string, int> per_src;
            int ffs = 0;
            for (auto cell : mod->selected_cells()) {
                if (cell->is_builtin_ff()) ffs++;
                per_src[cell->get_src_attribute().empty() ? "?" : cell->get_src_attribute()]++;
            }
            log("module %s: %d cells, %d FFs, %d distinct src locations\n", mod->name.str().c_str(), GetSize(mod->cells()), ffs, GetSize(per_src));
        }
    }
} Atf15xxProbe;
PRIVATE_NAMESPACE_END
