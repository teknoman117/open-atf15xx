# The story so far

*A running narrative of the open-atf150x project, kept as raw material for a blog post. Newest
chapters at the bottom. Each chapter: what we set out to do, what happened, what surprised us,
what went wrong, and the numbers. The technical record lives in `docs/worklog/`; this file is the
human version.*

---

## Prologue: old chips, dead tools

The Microchip (formerly Atmel) ATF1502/1504/1508 are 5 V–tolerant CPLDs that are still in
production and still useful, especially for retro-computing glue logic. Their toolchain is not:
WinCUPL from the early 2000s, a set of command-line "fitters" (`fit1502.exe`, `fit1504.exe`,
`fit1508.exe`) and **ATMISP**, a Windows GUI that turns a fuse file (`.jed`) into an SVF file that a
JTAG programmer can play. Atmel once had a Verilog compiler for these parts; today it is effectively
impossible to obtain.

The goal: **Verilog in, programming file out, with open-source tools only**, using
**Yosys → our own fitter → `.jed` → SVF**. The ancient vendor tools stay around as *oracles*:
ground truth to test against, never part of the flow.

## Chapter 1: ATMISP, the GUI that wouldn't talk (2026-11-19)

**The ask:** "This program is GUI only. Can you define a specification for all the conversions it
does, and then build a command-line version?"

**The approach:** treat ATMISP as a black box. Everything it does would be learned from what it
writes for inputs we make up, so that anyone with the program could repeat the whole process.

**Second surprise:** ATMISP has a headless mode (`ATMISP.exe file.chn -1`), but it only drives the
USB cable. SVF output exists *only* behind the GUI's "Write SVF file" checkbox.

**The trick:** a tiny Win32 program (`drive.c`, cross-compiled with MinGW) that starts ATMISP under
wine, finds its dialog controls by resource ID (1019 = SVF path, 1020 = checkbox, 1022 = revision,
1007 = Run), fills them in, presses Run, dismisses message boxes and scrapes the transcript list box.
Wrapped in a Python runner inside a wine virtual desktop, the GUI became a 10-second-per-case test
oracle. Over the project it ran a few hundred cases.

**The best trick of the chapter: let the program map itself.** ATMISP rearranges JEDEC fuses into
the chip's programming rows, differently for every family. Instead of guessing the shuffle, we
handed it JEDEC files in which every fuse spells out its own index in binary, one bit of the index
per file (seventeen files per family, plus all-zero and all-one files to spot constant cells).
Reading the rows back out of the SVF gave the exact fuse → (row, column) map for every family.
Those maps, compressed into a few formula tables, became the spec. A test *parses the spec's own
Markdown tables* and checks them against the code.

**Archaeology finds** (all reproduced by our implementation, because byte compatibility was the
target):
- Rev C SVF delays are 1000× too short: ATMISP divides microseconds by `TCK × 1000`.
- Every wait gets a stray `+1 µs` (50 ms prints as `50021E-6`).
- One 1508BE scan prints a **stray byte**, a backtick inside a hex
  number: `SDR 9 TDI (\`00)`. That is invalid SVF, and it is the same on every run.
- JEDEC fields of 200 bytes or more crash the program or corrupt its output; fuse digits 2–9 are accepted and
  produce garbage; the JEDEC checksum is never checked.
- Line 2 of a chain file is always thrown away, whatever it contains.
- Device names with "SE" in them crash the program.

**Mistakes worth admitting:** my first JEDEC regex skipped every other fuse field. My first guesses
about the 200-byte limit and over-long fields were both wrong; the oracle corrected them. A
path-handling bug later made one of my own checks compare ATMISP's output with *itself*. It passed,
of course.

**Result:** an 11-chapter spec, and `atmisp`, a Rust CLI byte-identical to ATMISP on 139
recorded cases and all 16 Microchip sample designs.

## Chapter 2: The fitters (2026-11-23)

**The ask:** work out what `fit1502/1504/1508` do, build open fitters, and document how pins,
nodes and buried macrocells map into the `.jed`.

A background agent ran thousands of synthetic `.tt2` designs through the vendor fitters under wine,
varying one feature at a time and diffing the fuse files. The interconnect (UIM) tables agreed
with 27,000 routing observations without a single mismatch. Every fuse of all three families got
a documented meaning, each marked as oracle-verified, labelled by the vendor's own output,
external, or inferred.

**A familiar bug:** the oracle's JEDEC reader skipped every other fuse field, the same mistake as
in Chapter 1. Everything was re-derived after the fix.

**Result:** Rust fitter crates that turn `.tt2` into `.jed`, byte-identical to the vendor 96.8 %
(1502), 98.0 % (1504) and 91.2 % (1508) of the time on synthetic designs. 7 of the user's 8 real
designs fit and are functionally equivalent.

## Chapter 3: prjbureau, and what it isn't (2026-11-23)

whitequark's **prjbureau** had documented ATF15xx fuses years earlier. The assumption was that it
connected to nextpnr, the open place-and-route tool. Reading its source rather than relying on
memory showed otherwise: prjbureau is a **fuse database plus docs and JED↔SVF tools**. It has
**no fitter**, and nextpnr appears only in its acknowledgements. Its Verilog flow hands a
hand-written netlist to the *vendor* fitter via EDIF and needs a library file (`aprim.lib`) that the
user's WinCUPL install doesn't have.

Cross-checking paid off both ways. prjbureau agreed with our maps on every fuse it covers. It also
showed that ATMISP's own fuse annotations number product terms **backwards for half the
macrocells**, and a fitter experiment confirmed prjbureau's numbering is the functional one. In
return we added BE-series programming to prjbureau (kept as local patches), which upstream lists as
"not investigated".

The user's note: prjbureau was said to work with the ATF1502, untested with the 1504 and 1508,
"and ironically, I have samples of the latter two but not the first."

## Chapter 4: Proving it, not just fitting it (2026-11-24)

"Verilog simulation is a solved problem, so we need to ensure the resulting design matches the
Verilog." So a simulator agent built a `.jed` → chip-model decoder, a `.jed` → Verilog emitter,
and an equivalence checker: formal proofs with Yosys/ABC, plus co-simulation.

**The vendor fitter has bugs, and the checker proved them:**
1. On outputs with an output enable, it feeds the register back from the *pin*, so the logic breaks
   whenever the output is disabled.
2. It silently truncates the `-open_collector` list after 43 entries. **The user's real
   `shift-register` board has 20 outputs that are push-pull rather than open collector**, a
   genuine hardware finding from software archaeology.
3. When its first placement fails, its second, cascade-based pass inverts register polarity; seen on
   the user's `blink.v`.

Our fitter produced no wrong result on any design it fitted.

## Chapter 5: Verilog, all the way down (2026-11-24)

The open flow: **Verilog → Yosys (pinned v0.71) → Rust importer → our fitter → `.jed` →
`atmisp` → SVF**, one command: `atf15xx-synth build design.v -d ATF1504AS-PLCC44 -o out/`.

**Surprise:** Yosys removed `abc -sop`, the sum-of-products mapping the prototype relied on, in
v0.69. The importer now builds the sum-of-products equations itself from Yosys's gates, and packs
shared logic better (51 cells instead of 91 on one SPI design).

The equivalence checker found two real bugs in *our* fitter (a wrong clock edge, an output that
never drove). Both were fixed. Then the user's own Xilinx designs, `blink.v` and
`spi_controller_mode0.v`, retargeted to ATF15xx parts, were **formally proven equivalent to their
Verilog**. The vendor fitter gets both of them wrong.

## Chapter 6: The heat death of the universe (2026-11-24)

The proof engine stalled on `blink.v`: a 28-bit counter reached only 40 steps of depth in 300
seconds. The user: "if you made a 64-bit counter we'll be sitting here until the heat death of the
universe."

The fix was to stop counting. The fitter now writes a map from each Verilog register to its
macrocell. With it, the formal engine proves "matching states lead to matching next states", a
one-step induction proof, and the Verilator simulator periodically forces both models into the same
random state, so the top bits of a 64-bit counter get exercised without waiting 2⁶⁴ cycles. Every
check now has a hard time budget and ends with an honest guarantee line: PROVEN, BOUNDED, SIMULATED
or FAIL.

| | before | after |
|---|---|---|
| `blink.v` (28-bit counter) | 40 steps in 300 s, not proven | proven in ~1–2 s |
| 64-bit counter | "heat death" | proven in 2.6 s |
| broken 64-bit counter (wrong only near 2⁶² cycles) | — | caught in ~30 s |

Along the way the 64-bit counter crashed our fitter: any function with more than 64 inputs panicked.
The real fix was to stop collapsing logic past the 40 inputs a logic block can take, so carry chains
stay as intermediate nodes. A plain behavioural `count <= count + 1` now fits in 87 of 128
macrocells and is proven equivalent.

## Chapter 7: Timing charts hiding in plain sight (2026-11-24)

**The ask:** a better fitter needs to know how fast things are. "The documentation directory should
contain some timing data."

The datasheets have the classic Atmel timing model: a pin goes through the input buffer (tIN), the
interconnect (tUIM), the product-term array (tLAD), maybe a cascade (tPEXP) or a foldback (tSEXP),
the register (tRD) and the output buffer (tOD1/2/3). One surprise came early: added up, these
internal numbers reproduce the datasheet's headline pin-to-pin figures *exactly* for the -7, -10
and -15 grades (tPD1 at -7: 0.5 + 1 + 3 + 1 + 2 = 7.5 ns). For the slow -20 and -25 parts the
headline figures quietly include 2–5 ns of extra margin.

**The find:** the fitters' help text lists two options nobody had used:
`-strategy Verilog_sim sdf` and `-strategy tPD = N`. With them, the 20-year-old fitter writes a
gate-level Verilog netlist plus an SDF timing file. Every delay in it turned out to be a sum of
datasheet parameters, truncated to whole nanoseconds (7.5 ns became 7, 0.5 ns became 0).

Matching the delays against the datasheets gave each parameter its value and each cell its
recipe. A checker confirmed the recipes on 260 vendor-written SDF files: 52 designs at all five grades, 6,895 delays, every one explained. The
checker also showed what the vendor's timing model ignores. Slow slew is the vendor's *default*,
yet outputs are always annotated with the fast-slew delay, 3 ns optimistic at -7. The 10–15 ns
reduced-power adder is ignored too. The vendor SDF is a simulation aid, not sign-off timing.

The user's other question had a short answer. The fuses that route the global interconnect onto
each block's 40 inputs were already fully mapped in Chapter 2, with zero mismatches across 27,000
routings.

## Chapter 8: First silicon, and not bricking it (2026-11-24)

**The ask:** "Follow my notes … and you should be able to use openocd to program the attached
atf1508as cpld. Ensure you've traced the jtag enable / disable bits so you're not left unable to
program without me putting 12v on the erase pin."

The ATF15xx can give its four JTAG pins back to the design as ordinary I/O. Once that's programmed,
the only way back in is 12 V on the OE1 pin, which the breakout board has a jumper and a boost
regulator for. The first thing the notes turned up was a trap: the board's own example project is
built with `-strategy jtag off`. Programming it, the most natural first test, would have locked the
chip.

So the bit was traced three ways before anything was written:
1. The vendor fitter fitted the same design with JTAG on and with JTAG off. The fuse files differ
   in one special-purpose fuse (74115 on the 1508) plus the configuration of the four JTAG pins'
   macrocells.
2. All ten of the user's vendor-built JEDs agree with their command lines: fuse 74115 is 1 for
   `-jtag on` and 0 for `-jtag off`.
3. Flipping only that fuse and regenerating the SVF changes exactly one thing: a 4-bit write to the
   chip's special register goes from `f` to `b`.

A small guard script now refuses to program anything that would clear that bit. It was tested on
three images that must be refused, including the board's own example.

Then came first silicon. A read-only scan found the chip (IDCODE `0x0150803F`). The user's own
`blink` design for this board was fitted by *our* fitter and checked against the original over 33
million simulated cycles. `atmisp` turned it into SVF, and OpenOCD on a $15 FT232H board played
it: *"svf file programmed successfully for 3569 commands with 0 errors."* A separate readback
pass matched. To make sure that readback really checks something, a deliberate negative control
verified the chip against a *different* JED, and it failed. JTAG still answered afterwards.

## Chapter 9: An Arduino as a lab assistant (2026-11-24)

"So that you don't need me confirming things," the user wired an Arduino Uno to the CPLD: four
I/O pins through resistors, the global clear, the second global clock (on a timer-capable pin)
and OE1. A 2 KB firmware turned the Uno into a test instrument, and the chip could finally answer
for itself.

Its first answer was a lesson in humility. The LED pin "toggled" at exactly 60 Hz, which is
mains frequency: hum on a pin that nothing was driving. A first conclusion, that our fitter's
image didn't drive the pin, was wrong. The chip simply hadn't been responding while the bench was
being wired. Reprogrammed, our image blinked at exactly 33.5 Hz, just like the vendor's.

Then came a run of questions that had been marked "inferred" for days:

- **Reset or preset, when both are asserted?** Reset wins, both for the global clear and for a
  product-term reset, whichever arrives first.
- **What's in a register at start-up?** 0, whatever the polarity fuse says (after programming at
  least; a cold power-up still needs someone to flip the power switch).
- **Is the foldback line really the complement?** Yes. An exhaustive truth table that would have
  caught the opposite polarity came back 64/64.

Then came a proper mystery. The Verilog version of blink didn't blink at all at 66.6 MHz. On a slow
bench clock it blinked, but *late*: sometimes 21 clocks after 1,000,000, sometimes 267, different
every run. A synchronous circuit on a clean, slow clock has no business being random. Every
suspect was eliminated in turn:
- the bench clock: a 4-bit counter counted 36 random bursts of up to 200,000 pulses exactly;
- the carry chain: a 20-bit counter was exact up to a million;
- the foldback: a build without it behaved the same.

The culprit was two dedicated input pins that nothing was driving. With the bus keeper switched
off, they floated around their switching threshold in the mains hum, and that was enough to upset
logic elsewhere on the chip. Once both pins were driven, the slow blink was exact every time.

The 66.6 MHz failure was real, though, and the timing tables from Chapter 7 explained it to the
nanosecond. The user's CUPL blink computes every counter bit in one pass through the logic array.
The vendor's fit takes two passes, and ours took four. At 15 ns per clock, four passes don't fit at
any speed grade. The user's hint, "most wide counter examples have Xn.T = Xn-1 & Xn-2 & …", is
the fix: a wide counter is one product term per bit, not a carry chain.

The fix turned out to be a single number. Our importer could already turn counters into T
flip-flops, but it first expanded each bit's full next-state function, with a budget of 64
product terms. For a counter that wraps at 999,999, that expansion contains the complement of the
compare and blows past 64, even though the toggle function at the end is one or two terms.
Raising the budget to 1024 made 19 of the 20 bits single-term T flip-flops, and the build got
*faster*. Spilling the last bit over a three-macrocell cascade made the design single-level. The
Verilog blink then ran at 66.6 MHz: 33.40 Hz on the LED, on silicon.

What the episode showed is that the hard decisions in a CPLD flow, such as T or D, XOR or
cascade, which block a cell goes in, all need knowledge Yosys no longer has. Upstream Yosys
dropped its last CPLD flows. A quick probe showed that our own Yosys plugin builds and loads fine,
and that Yosys's coarse netlist still sees the counter as six word-level cells before it gets
ground into gates. A placement experiment found that our "fill block A first" placer leaves a
third of the interconnect headroom unused. Those numbers became the roadmap (spec F9).

Why had Yosys dropped them? The git history gives a precise answer. CPLD support lasted ten
years and was deleted in four days, 17 to 20 August 2026, just three months before this
project started. The PR descriptions explain the change in one word: "Yeet." The deletions were a
side effect of moving all of Yosys onto `abc9`. CoolRunner-II was "the one flow using `abc -sop`,
which has no XAIGER equivalent", and its author had agreed: "the flow as currently implemented
does not actually work all that well. As long as these architectures don't get _completely_
conceptually forgotten, I can accept needing to reimplement them to a higher standard." Nobody
complained. The place-and-route tools that consumed that output had been quiet since 2020. The
building blocks are still in Yosys 0.71, and sixteen lines of script rebuild `abc -sop` from
them, proven correct on a counter. The first attempt was silently wrong (a port-naming quirk),
and only the equivalence proof caught it (research note `docs/research/yosys-cpld-removal.md`).

## Chapter 10: The chip settles the arguments (2026-11-24)

With the bench wired up, questions stopped being matters of opinion. The user suspected that clock
polarity could be chosen per macrocell. It can, but not through any per-macrocell bit. Each of the
three global clock networks has its own source selector and inverter, so one pin can feed two
networks with opposite polarity. Fuse-patched JEDs showed it working on silicon. The vendor fitter
never does this; it spends a product term on a clock instead.

"You should definitely figure out how to make foldback terms work." A foldback is a NAND gate that
stays inside the block, so an OR of any number of literals is a single foldback product term (De
Morgan). Generalising the fitter's foldback factoring to any common factor, and trying both
polarities of every function, made `motioncontroller` fit. It was the last of the user's designs
that hadn't. A foldback whose product term reads other foldbacks (NAND of NANDs) passed its
silicon truth table too, and the vendor fitter turned out to use exactly that trick itself.

Then a real design: the user's P80C550 self-programming board, "iirc it barely fit". It did: 64
of 64 macrocells and 47 foldbacks. Checking it found three problems on our side:
- a harness bug that dropped address pins declared as `[15:10]`;
- a formal engine that let a bus write and its clock change in the same instant;
- a wrong guess in both of our simulators about what a macrocell sees when it lends its OR gate
  to a neighbour.

That last one is the best story of the chapter. Our models said the vendor's JED was wrong. The
user said it worked in hardware. Two one-fuse experiments on the bench settled it in the user's
favour: a lending macrocell's XOR sees a constant 1. The same bench also *confirmed* a vendor bug
claimed earlier. Programmed on the chip, the vendor's fit of the user's Verilog blink starts its
LED at 0 where the Verilog says 1, while our fit starts at 1.

With a partitioning placer (sharing interconnect inputs evenly instead of filling block A first)
and the new foldback factoring, our fitter now fits the self-programming board too, and the result
matches both the vendor's working JED and the CUPL source. The Verilog port of the same design is
the next hill: four interconnect inputs too many in one block.

Meanwhile an agent dug through Yosys's history. Upstream removed all CPLD support in four days in
August 2026, as part of a move to a single ABC9 mapping path, with the original authors' blessing:
"as long as these architectures don't get completely conceptually forgotten". This project is,
in a way, the not-forgetting.

## Chapter 11: Reading the vendor's homework (2026-11-24)

"You should try building the selfprogramboard with the vendor tools and reading its fit report …
maybe it's lowering to a primitive you don't yet use." It was. The vendor's report lists every
equation after mapping, and one name stood out: `XXL_375`. It is the SPI clock enable, an OR of
four products, built as a foldback that reads four other foldbacks, and then **copied into every
one of the four blocks**. Foldbacks never leave their block, so the vendor spends one product term
per block on each copy instead of a macrocell and an interconnect input. Twenty of its 47
foldbacks are that one signal.

Teaching our fitter the same trick took a string of small, honest fixes:
- foldback products that read other nodes;
- a collapse that leaves those nodes alone;
- a rule that a foldback line has only one polarity, so no rewrite may invert it;
- letting a macrocell host the very foldback it reads.

Then the Verilog port of the board needed 64 cells, exactly like the vendor. One wall remains: in
the first block, the interconnect multiplexers can't reach some of the signals, because *where* a
signal lives decides which multiplexers can select it. Placement has to choose positions, not just
blocks.

Along the way:
- **The flip-flop became stealable as an input register**, with D straight from the pin, verified
  on silicon.
- **The fitter now runs inside Yosys** as a plugin, writing byte-identical JEDs.
- **A crate audit replaced two hand-rolled random number generators.** A JEDEC crate written for
  the CoolRunner-II tools reads all 134 of our JEDs correctly. It also caught a checksum bug in
  our own fuse-patching tool on its first run.

## Chapter 12: Looking inside a running chip (2026-11-24)

"Figure out how to use openocd to read and possibly write input, output, and register state over
jtag." Every JTAG chip carries a boundary-scan register: a shift register with a cell next to every
pin, which an IEEE 1149.1 instruction (SAMPLE) photographs without disturbing the logic. A
chip's BSDL file says which bit is which. Microchip's website turned away the automated download,
and the datasheet folder had BSDLs for the 1502 and 1504 but not the 1508. A forum attachment
turned out to hold an Atmel-generated BSDL for a different 1508 package.

That raised a question: would the other package's map fit this chip? A first measurement of the
register said 300 cells, and the BSDL said 352. The 300 was a bug in the measurement: the marker
pattern was shorter than the register. Fixed, the chip measured 352 exactly, because the chain is
per macrocell and the package only changes the labels.

Every cell checked out on the bench: inputs, outputs, and a register's state. Writing worked too,
with EXTEST driving pins that the Arduino read back. The ATF15xx's scan cell sits on each
macrocell's *output multiplexer*, so a buried register is visible only if that multiplexer
selects it. For a macrocell with no output that choice is free, so the flow now always makes it.
Every register in a design built by our flow can now be read over JTAG by its Verilog name, while
the design runs.

## Chapter 13: The design that barely fit, from Verilog (2026-11-24)

The user's SELFPROGRAMBOARD (an 80C550 self-programming board, in CUPL) "barely fit", and the
vendor's fit used all 64 macrocells. Our fitter had matched that from the vendor's own
intermediate file for a while. From a Verilog port, it kept failing on one block, which wanted 37
of its 40 inputs and could not get them.

The reason is a detail of the interconnect. Each of a block's 40 input multiplexers can pick only
a few signals, and *which* ones depends on the macrocell a signal lives in: on the 1504, a
macrocell's feedback is reachable from exactly three of the forty. Assigning cells to the right
*blocks* is not enough; inside a block, the right *seats* matter too. A small local search now
shuffles buried cells between seats, scoring each arrangement by a bipartite matching. It took the
failing block from 4 unroutable inputs to 0, and the design fits in 60 macrocells: four fewer
than the vendor needed, from Verilog. It is formally equivalent, for as many steps as the
time budget allows, to both the Verilog and the vendor's JED, which the user confirmed works on
hardware.

Two smaller lessons came with it. The first version of the search was correct but slow. It
quietly cost an unrelated 64-bit counter its fit, because the flow ran out of time before
reaching the strategy that worked. The fix was to make the search incremental and to keep its
result only when it fully succeeds. The second lesson was a naming bug: `input [15:10] A` had
been numbered from zero, so its bit "A0" collided with the board's real `A0` line. As the user
pointed out, the CUPL original declares `A[15:10, 0]` as one field, which is why the port has
both.

## Chapter 14: Two jobs for one macrocell (2026-11-24)

The user asked a question that CUPL had never let them answer: can a pin's fast input register,
the flip-flop fed straight from the pad, share its macrocell with logic that does something else?

The fuse map gives a precise answer. The register's "D from the pad" setting reuses the fuse that
otherwise picks what drives the pin. With D from the pad, the pin is fixed to the register output.
A macrocell has only two exits, the pin and the feedback into the interconnect. If the register's
value goes out through the feedback, the combinational output has nowhere to go. Everything
*before* that output is still free, though: five product terms and an OR gate whose result can
be handed to the next macrocell through the cascade chain.

So the register costs nothing, and the macrocell is still a full product-term donor. The bench
confirmed it: one macrocell loaded P80 on every GCLK2 edge while also supplying five of the eight
product terms of a 4-input parity next door. All 32 combinations were correct, on the pins and in
the boundary scan. The fitter now packs that way by default.

## Chapter 15: Six truth tables settle an XOR (2026-11-24)

Every macrocell ends in an XOR gate, and three fuses decide what reaches its two inputs. The
vendor's fitter only ever uses three of the eight combinations: we scanned all 17,193 vendor JEDs
from our experiments to be sure. Our model had been fitted to those three. prjbureau's notes
made a different claim for one of the unused ones. XOR synthesis would need the others, so it was
time to ask the chip.

A new helper (`mcbits.py`) patches a macrocell's bits by name. One base design and six variants,
each programmed and swept through all 16 inputs, gave six truth tables. Two of them contradicted
our model. They all fit one rule: the cascade fuse doesn't just divert the OR gate, it *swaps*
the XOR's inputs. The "invert" fuse is really an enable on the second input. The old model's
prediction for "PT2 to the XOR, invert off" was an XNOR. The chip produced plain PT1: product
term 2 simply drops out. The vendor never writes that combination, which is why nothing had
caught it.

Both simulators and the oracle now follow the silicon rule, and each variant is proven equal to
what the chip did. The rule has a practical consequence: a macrocell that lends its OR gate to its
neighbour still has one product term of its own, which it can drive, inverted or not, to its pin.

With the rule settled, XOR synthesis followed. The vendor only ever does one thing with the XOR:
when asked (`-XOR_Synthesis on`), it turns `A·!B + !A·B` into PT1 = A, PT2 = B. Our fitter now
does exactly that, and 94 of the vendor's 95 XOR test fits match ours bit for bit. Before, none
did. Our flow goes further. Any function may become `g ⊕ p`, with one product moved to the XOR,
whenever that saves product terms. A 3-input parity drops from four product terms to three. The
chip agreed on all 16 input combinations.

## Chapter 16: A timing analyser that agrees with the chip (2026-11-24)

The bench taught the timing lesson early. A Verilog blink counter that looked fine in
simulation would not run at all from the 66.6 MHz oscillator. The fix then was found by hand: a
larger product-term budget gave a single-level design. What was missing was a tool that could
see the problem coming.

`atf15xx-sim timing` now reads any fuse map, ours or the vendor's. It follows each path through
the interconnect, product terms, cascades, foldbacks and registers, using the vendor's own
speed-grade tables. For the counter that failed, it reports 32 MHz, three trips through the
interconnect deep. For the one that ran, it reports 100 MHz, one trip. With `--fmax 66.6`, the
flow keeps trying strategies until it finds a fit that meets the target. For the blink counter
it found one at 66.7 MHz, in fewer macrocells than the default. On the chip it blinked at exactly
66.6 MHz / 2,000,000. The analyser agrees with the silicon on both sides of the line.

## Chapter 17: Talking to Yosys directly (2026-11-24)

Until now our Yosys plugin was a courier. The C++ side asked Yosys to write the netlist out as
JSON text, and the Rust side parsed the text back. Rust could read the design but never touch it.
The user suggested the `cxx` crate, which lets Rust and C++ share types safely, so Rust could
work on Yosys's own data structures.

The bridge has three lanes. Rust *drives* Yosys, running each line of the synthesis script as a
Yosys command. Rust *reads* the result straight from Yosys's in-memory netlist. And Rust *writes
back*: after the fit, every wire that landed in a macrocell carries an `atf15xx_mc` attribute,
so ordinary Yosys commands can ask where a signal went.

The bridge has to be exact: our importer was written against the JSON text, so the bridge copies
the JSON writer's quirks, from its bit numbering to its habit of adding a space after strings
that look like binary numbers. A built-in self-check runs both paths in one process and demands
identical netlists. It passed on all 35 test designs, and every resulting fuse map matched the
command-line flow bit for bit. One check briefly disagreed, which uncovered a reproducibility
wart. Yosys's internal wire names contain the source file's path. Read the same file by a
different path, and the fitter can place the design differently.

## Chapter 18: A new name (2026-11-24)

The project began as `open-atmisp7`: a byte-exact replacement for one Windows programmer, ATMISP
7.3. It has long since outgrown that. It now has a fitter, a simulator, a timing analyser, a Yosys
plugin and a silicon bench. So it is now **open-atf150x**, after the chips rather than one vendor
tool. The ATMISP replacement, which once lived at the repository root, moved into its own crate,
`atmisp`. The repository became a plain multi-crate Rust workspace of the kind used upstream: a
virtual root manifest, shared package metadata and dependency versions in one place, and every
crate under `crates/`. The regression suite showed the move changed nothing: the same fuse maps,
the same SVFs, the same proofs.

## Chapter 19: Counting without lookahead (2026-11-25)

The last roadmap item asked for counters without the carry-lookahead trees that Yosys builds for
adders. A 16-bit counter should take 16 macrocells; ours took 20 on two of the three chips.

It turned out to be a question of method rather than structure. To make each counter bit a
toggle flip-flop, the flow first wrote the bit's next state out as a sum of products. For the top
bits of an up/down counter that form is astronomically large, even though the answer, "toggle
when everything below is ones (or zeros)", is three terms. A binary decision diagram, a compact
canonical form for Boolean functions, holds the same function in a few hundred nodes, and
the three terms read straight off it. The crate for that, `biodivine-lib-bdd`, was already on
crates.io.

A 64-bit counter adds one more problem: bit 63 toggles on 63 signals, and a block of the chip
sees only 40. Yosys's answer was a lookahead tree of 6 to 23 helper cells. The new answer is a
single shared "carry" cell that ANDs the first 30 bits, which every higher bit then reads. The
64-bit counter dropped from 87 macrocells to 65 and doubled its speed. On the bench, a 40-bit
counter was loaded just below its wrap and counted across it, and boundary scan read back all
40 bits correctly. That closed the roadmap that had been drafted the day before.

## Chapter 20: Our fitter against theirs (2026-11-25)

The user wanted the scoreboard for the board that started all this: SELFPROGRAMBOARD, the
design that "barely fit". There are four ways to build it: WinCUPL or our Verilog flow for
synthesis, and the vendor's fitter or ours for fitting. All four went through the same
measuring tape: resources decoded from the fuse maps, timing from our analyser, and an
equivalence proof against the vendor's hardware-proven JED or against the Verilog.

On the vendor's own netlist, our fitter now wins outright: 60 macrocells instead of 64, 180
product terms instead of 191, 55.6 MHz instead of 50, and a 10 ns instead of 14 ns worst
pin-to-pin path. Most of the difference is foldbacks. The vendor replicates its foldback
networks in every block that reads them (47 in all), and each costs a foldback delay; we use
one.

Our own synthesis is a different story. From Verilog the flow reaches the same size, but runs
at 33 MHz, because the gate-level netlist arrives with twice CUPL's product terms. The fitter
recovers the area but not the logic depth. And the vendor fitter cannot fit our netlist at all.

The same day brought the tighter Yosys integration the user had asked about. Yosys now stops at
word-level cells, and Rust lowers them: comparisons share their XORs, and XOR trees come in
4-input groups, each one macrocell with XOR synthesis. Inside the plugin, Rust maps the design
both ways from the same RTLIL and keeps the better fit. The flow also started to listen to the
fitter: when a fit fails for lack of macrocells, the next strategies are the ones that save
macrocells. That alone made SELFPROGRAMBOARD fit with default options. And timing became a
goal you can state, `clock clk 66.6` in the constraints file. A fit that meets the target beats
a smaller one that does not.

## Chapter 21: Checking our homework against the textbooks (2026-11-25)

Much of the fitter is our own invention: a two-level minimiser, a complement routine, an
interconnect matcher. The user asked to lean on existing libraries wherever they could replace
unproven code, and a crates.io survey found good candidates. The first use for them was as
examiners.

Berkeley Espresso, the reference two-level minimiser, checked our minimiser on a thousand
random functions with exhaustive truth tables. It was always correct, and within one product
term of Espresso. Our complement routine was always correct, but larger than Espresso's in
almost two cases out of five. A graph library's maximum matching checked our interconnect router
on 1200 random signal sets. It always agreed.

Adoption went less well, and taught more. Espresso inside the fitter never finished the first
design: an ordinary counter bit, 15 inputs, took it nearly a minute, growing eightfold with each
input. Berkeley's own program, compiled from source, behaved the same. On that function our
routine is instant, and its answer is minimal. A second idea, reading complements off a decision
diagram and keeping whichever cover is smaller, was exact and fast, and changed nothing at all
across 70 real fits. So it came back out. The code that stayed is the same as before. What changed
is that its correctness now has a proof attached.

## Chapter 22: An exam for the placer (2026-11-25)

The placer, the part that decides which block of the chip each piece of logic lives in, had
never been graded. Its job looks simple: no block may need more than 40 input signals. It
was written as a greedy pass plus a local search, and nobody knew how far it was from the best
possible answer.

An integer-programming solver, HiGHS, can find that best answer exactly for designs this size.
Fed the same problem as our placer, it ran across 70 builds, the dense SELFPROGRAMBOARD and the
user's own designs. Wherever the chip was tight, our placer was already optimal:
SELFPROGRAMBOARD needed 38 inputs in its fullest block, and so did the solver's best. The user's
densest design, motioncontroller, matched as well. On easy designs our placer leaves blocks
lopsided, up to 20 inputs above the best balance, because it stops at the first assignment that
works. That costs nothing until a block is nearly full, and there the placer already balances.

The same round closed an old reproducibility bug. Yosys puts the source file's path into its
internal names, and the alphabetical order of those names against Yosys's own internal names
depends on the path's first character, so the same design could fit differently depending on
how its path was spelled. The fix is to run Yosys from the source directory, so the names never
contain a path.

## Chapter 23: Faster than the vendor, and faster to get there (2026-11-25)

Two requests closed the loop on the board that started everything. The first was to make
SELFPROGRAMBOARD fast from Verilog: above 50 MHz, as WinCUPL manages. The fit was spending its
spare logic on foldback networks, each costing a delay, where CUPL's netlist kept shared
clock-enables as ordinary macrocells and paid for them by packing register pairs into shared
macrocells. Our registers could have been packed too, except that toggle recovery had turned
them into toggle flip-flops, which don't fit the shared slot. A strategy that keeps plain D
flip-flops and no foldback signals now fits in 60 macrocells at 52.6 MHz, and the flow picks it
by itself, because on dense designs it now tries every space-saving strategy and keeps the
fastest of the smallest. WinCUPL and the vendor fitter reach 50 MHz.

The second request was speed of the tools themselves, with an instruction to profile before
optimising. The profile overturned an assumption: the fitter was not slow at fitting, it was
slow at bookkeeping. Most of its time went into hashing signal names while the block placer
tried its 160,000 moves. Numbering the signals once cut the dense board's fit from 26 seconds
to 9. Running the strategies in parallel looked like an obvious win and was measured to be a
loss: the first version ran 16 strategies at once and then waited for the slowest, mostly
strategies the search would never have needed. Limited to the strategies known to be needed, it
helped. Typed failure reasons let the fitter skip retries that cannot succeed, and a one-line
counting argument now rejects an impossible fit in milliseconds instead of a minute. The dense
board fits in 6.3 seconds, every fuse identical to before.

One more round, of caching, finished the job. Remembering covers already computed helped a
little. The big win was a check the placer runs for every candidate seat, "would this block
still need at most 40 inputs?", which had been rebuilding sets of signal names from scratch each
time. Answered from remembered, numbered inputs instead, the dense board fits in 2.3 seconds, a
little over a tenth of where it started, and the whole test corpus builds in 47 seconds instead
of 396.

Measured end to end, the whole trip from the Verilog source to a programmable SVF file,
including both Yosys syntheses, both fits and the JED, takes 2.3 seconds of wall time for
SELFPROGRAMBOARD, or 3.0 seconds counting the Docker container's start-up. That is 5.4 seconds
of CPU (5.25 s user, 0.14 s system) spread over about two and a half cores, because the two
mappings and the space-saving strategies run side by side. Yosys accounts for 0.12 seconds of
each synthesis; nearly all the rest is the fitter. For comparison, the same build took 62 seconds
earlier in the day.

## Chapter 24: Our own handwriting (2026-11-25)

Byte-for-byte compatibility had been the project's yardstick from the first day, and for JEDEC
files it had quietly turned into copying. The fitter's JED writer reproduced the vendor fitter's
layout down to its annotation notes and their trailing spaces, and the ATMISP replacement wrote its
`Load` files from six templates cut out of ATMISP's own output, header line "By Atmel ISP Version
7.3" included. That made a perfect test, and it put the vendor's text in a repository meant to
become public.

The fix was to change what "identical" means. A JEDEC file is a container for a fuse array, a
security bit and a checksum; the comments and line breaks around them are presentation. Both
writers now use a layout of our own: a one-line header naming the chip and our tool, then one
`L` field per item of the fuse map, each with a short note of what it is (`MC7 PT2`, `block C UIM
mux 13`, `JTAG special bits`), every fuse written out. The one hard constraint came from our own
spec of ATMISP's reader: every field must stay under 200 bytes, or ATMISP reads past its buffer.
Every test that used to compare bytes now compares fuses and checksum. The 95 fitter vectors and
nine ATMISP `Load` vectors were rewritten in the new layout, each checked to hold exactly the
fuses of the vendor's file before it was replaced, and the templates were deleted.

The match counts did not move: 1591, 2146 and 1777 identical designs on the feature corpus, every
pin-role design on the 1502 and 1504, 8 of 8 user designs, 34 of 35 corpus designs proven
against their Verilog. The JTAG lockout guard reads the new files just as well and still refuses
a `jtag off` image. One piece of vendor text remains: the SVF writer still copies ATMISP's
header comment, because the SVF is still compared byte for byte.

## Chapter 25: A debugger made of a test standard (2026-11-25)

Boundary scan exists so that factories can test solder joints: every pin of a chip sits on a long
shift register that JTAG can capture or drive. On these CPLDs it does more than that. Every
macrocell, whether or not it has a pin, has its output and output enable on that register, so one
capture shows the state of every register of a running design. A debugger for free, if something
could read it.

`atf15xx-dbg` is that something. It builds the register's map from Atmel's own description files
(the BSDL files that prjbureau carries), checks every cell against what the project had already
measured, and talks to the chip through OpenOCD. The files are formal enough for a parser, except
for one thing: which macrocell a pinless cell belongs to is written only in a comment, by hand, in
four different styles and with one typo. The tool reads the comments and trusts them only where
nothing better exists. The checks found one real disagreement: a pin on the 160-pin package that
the description calls an I/O and the vendor fitter would only ever use as an input.

On the bench the first version took twelve snapshots a second. The chip was never the bottleneck:
each command waited 40 milliseconds for a network acknowledgement, because the command and its
terminator went out as two packets. Sent as one, the same tool takes over three thousand
snapshots a second. The test that counts: a 40-bit counter on the chip, clocked a random number of
times by the Arduino, read back by name through boundary scan, eight times out of eight correct,
including the moment every bit rolls over.

One question about it came straight from a real use: could it test a design with an 8-bit
8080-style bus, driving the address, data and strobes from JTAG? That needed two things: a session
that stays in EXTEST across many steps, and proof that the CPLD's logic keeps working while the
test standard holds its pins. The counter from the carry-chain test made the proof: its load and
enable inputs, normally driven by the Arduino, were driven over JTAG instead, while the Arduino only
clocked it. The counter loaded, counted across all 40 bits and held, read back by name every step,
18 checks out of 18. The one rule that falls out: a strobe to be driven this way cannot sit on a
dedicated input pin, because those have no driver cell; on the 1508 the GCLK3 pin is an ordinary
I/O pin that still feeds the global clock.

## Chapter 26: The tables, measured (2026-11-25)

The fitter's routing tables had been checked against 27,000 observed routings, but not yet
measured fuse by fuse. This round measured them, by the method of the project that started it all, prjbureau: run its own fuzzers, which ask the vendor
fitter thousands of small questions and record the fuse answers. Its fuzzers needed a fitter build
this bench does not have, so they first learned to speak to the one it has, through the netlist
format CUPL writes. On the two smaller chips the fuzzers then reproduced prjbureau's published
database exactly, value for value. On the larger 1508, the chip on the bench, they had never run
at all; a handful of bugs that only that chip reaches later, and with fourteen fitters running in
parallel, they mapped its 8,640 routing fuses in under an hour, where one fitter at a time would
have needed six. Where they found an answer, it matched the tables we had; where they did not, the
connection turned out to be one that no design in the corpus ever uses. The architecture file is
now generated from those measurements, and every regression came out the same.

The same day brought some tidying that a new reader would otherwise trip over. The crates moved out
of their `crates/` directory to the top of the project; the three little fitter programs became
executables of the fitter crate instead of crates of their own; and the ATMISP replacement and the
debugger, which had been two programs speaking to the same chips from two sides, became one:
`atf15xx-tool svf` writes the programming file, `atf15xx-tool sample` looks inside the running
chip. The prjbureau work moved to the user's own fork, ready to be proposed upstream branch by
branch.

The last tidy was of the question-askers. Every program that learns about the chips by asking a
black box thousands of small questions (the fitter experiments, the timing corpus, the ATMISP case
generators, the harness around prjbureau's fuzzers) now lives under `tools/fuzzers/`, and each
comes with a `CLAUDE.md`: the instructions an AI agent needs to supervise a run. They cover what to
start, which log lines show progress, when the cache must be thrown away, why two fitters must
never share a directory, and what counts as a correct result. Lessons that had lived only in
one long session's memory are now written down next to the code they concern.

Then the user asked for the whole learning process to be repeatable by anyone: one workflow per
fact, each run against the vendor programs from outside. A new fuzzer handed ATMISP JEDEC files in which every fuse spells out its own number in
binary across seventeen files, and read the map back from the SVF: our tool's output matched
ATMISP's in all 127 files, for every family. The timing tables were rebuilt from the public
datasheets and the fitters' own SDF output, explaining every one of 6,895 delays; the parameters
no output of the fitter ever shows are the datasheet's. Every workflow now sits in its own
directory with a README for readers and a CLAUDE.md for the agent that runs it, so anyone with the
vendor programs can watch the whole thing being learned from scratch.

One more rule about borrowed knowledge followed. prjbureau's database and Atmel's BSDL files had
been read in two ways: a Python script turned the database into a committed JSON file that our
crate parsed every time it started, and the debugger opened the BSDL files from disk when it ran.
Now the architecture crate's build script reads both when the code is compiled and writes them
out as Rust: the fuse map, the device tables and the boundary-scan map of every part become
ordinary constants in the binary. No prjbureau file is stored in our repository, and nothing is
parsed at run time. The build-time version matched what it replaced in every field, and its
boundary map for the bench chip matched the one checked on the chip in all 352 cells.

The user's oldest CPLD design came back as an example. The SPI master for an 8-bit bus, first
written in CUPL for an ATF1504AS, became a parameterised Verilog module with two variants: one
proven identical to the original, and one for the ATF1508AS on the bench, whose 66.6 MHz oscillator
needed a 17-bit prescaler to bring SCK down to a kilohertz. On the bench the JTAG debugger played
the microprocessor and the Arduino played the SPI slave, echoing every byte back. It failed at
first in two instructive ways. Running SPI through boundary scan, with the pins following the
design once per scan, is too slow and too jittery for a kilohertz clock, so the session now hands
the pins back to the design for each transfer, letting the hardware clock SCK, with the bus keepers holding the bus
meanwhile. And about one write in fifty loaded a wrong bit, at every JTAG speed: the original design
latched the data bus on the first clock after the write strobe, in the middle of the strobe's own
switching noise. A two-flop synchroniser fixed it; 64 of 64 bytes then crossed correctly in both
directions.

The breakout board's own demo followed: a 64-bit shift register fed by an Arduino over SPI, latched
onto 64 open-collector outputs, 128 flip-flops on a chip with exactly 128 macrocells. In Verilog it
fit in all 128, and the proof against the original CUPL design passed on every pin. The vendor's own
build of it never did: its fitter silently stops reading the open-collector list after 43 names, so
twenty of the board's outputs had always been push-pull. With that, the project's first release,
0.1.0.

## Where it stands

- ✅ `.jed` → SVF, byte-identical to ATMISP
- ✅ JEDEC files in our own layout, conformance checked fuse for fuse; no vendor-written JEDEC text in the repository
- ✅ `.tt2` → `.jed`, 8/8 real designs, plus the dense SELFPROGRAMBOARD (vendor: 64/64 MCs)
- ✅ Verilog → `.jed` → SVF, proven against the Verilog
- ✅ time-bounded proof and simulation
- ✅ **a real chip**: an ATF1508AS-10JU84 on an Arduino bench, programmed from our JEDs over OpenOCD; dozens of fuse-level facts settled on silicon
- ✅ timing data: vendor speed-grade tables for every family and grade, verified against 260 vendor SDFs
- ✅ an interconnect-aware placer (block partitioning + UIM-aware seats); the user's densest design fits from Verilog
- ✅ JTAG boundary scan: read every pin and register of a running design by name
- ✅ XOR synthesis (vendor-identical on its own tests) and a timing analyser that agrees with the chip
- ✅ a Yosys plugin in Rust that talks to Yosys's netlist directly (`cxx`), fuse-identical to the CLI
- ✅ input registers that leave their macrocell's logic free for neighbours (silicon-verified)
- ✅ counters mapped exactly (BDDs) into toggle flip-flops with carry chains; roadmap R1–R6 closed
- ✅ word-level mapping in Rust, both mappings compared in-process, timing targets
- ✅ the placer graded against an exact solver: optimal where the chip is tight; results independent of the source path
- ✅ SELFPROGRAMBOARD from Verilog at 52.6 MHz (WinCUPL + vendor: 50) in 2.3 s; the corpus in 47 s
- ✅ an in-circuit debugger over boundary scan: snapshots by Verilog name, watch, VCD capture, guarded pin driving
- ⬜ a typed netlist IR; the mapping–placement loop in the plugin

## Notes for the blog

- **Anecdote:** a GUI-only program turned into a test oracle by a robot hand (Chapter 1).
- **Possible titles:** "Interrogating a 2003 GUI with a robot hand", "The fitter that lied",
  "Proving a CPLD before burning it".
- **Themes:** vendor tools as oracles, not dependencies; letting a black box map itself (fuses that
  spell their own index); byte-for-byte compatibility, bugs included; formal methods catching real bugs in a
  20-year-old tool and in our own; honesty about guarantee levels.
- **Good figures to make:** the GUI-driver loop diagram; the fuse-map tables; the before/after
  timing table above; the vendor-bug waveform (pin feedback while OE is off).
- **Numbers:** 6,895/6,895 vendor SDF delays explained; 139 SVF vectors; 16/16 Microchip samples; 27,000 routing observations; 96.8 / 98.0 /
  91.2 % byte-identity; 12/12 bounded checks under 60 s; 64-bit counter proven in 2.6 s.
- **Process note:** most of the work was done by several AI agents running in parallel (fitter,
  prjbureau, simulator, Verilog flow), coordinated from one session, with every result
  independently re-checked before being reported.
