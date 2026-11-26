// ISC licence, like the rest of the project (LICENSE). The original CUPL design was MPL-2.0; its
// author relicensed this port.
//
// Verilog port of shift-register.pld, the example of the breakout-atf1508as board
// (teknoman117/breakout-atf1508as @ b473233, Example/; README.md says where it came from).
//
// A 64-bit shift register clocked by SCK (MOSI on IN, LSB first, as the Arduino's SPI sends it)
// and a 64-bit output register loaded from it on LATCH; both clear while rst_n is low. Each output
// pin pulls low while its register bit is 1 and is released otherwise (open collector), like the
// CUPL design's `[OUT63..0] = ![REGISTER63..0]` with every output open-collector.
// OUT[63] is pin 4 ... OUT[0] is pin 81: the CUPL pin list `[4..6, 8..12, ...] = [OUT63..0]`.
module shift_register(
  (* LOC = "1" *)  input  rst_n,   // CUPL `Pin 1 = !RST`; GCLR
  (* LOC = "2" *)  input  SCK,     // GCLK2
  (* LOC = "83" *) input  LATCH,   // GCLK1
  (* LOC = "84" *) input  IN,
  (* LOC = "81 80 79 77 76 75 74 73 71 70 69 68 67 65 64 63 62 61 60 58 57 56 55 54 52 51 50 49 48 46 45 44 41 40 39 37 36 35 34 33 31 30 29 28 27 25 24 23 22 21 20 18 17 16 15 14 12 11 10 9 8 6 5 4" *)
  output [63:0] OUT
);
  reg [63:0] shift, latched;

  always @(posedge SCK or negedge rst_n)
    if (!rst_n) shift <= 64'd0;
    else        shift <= {shift[62:0], IN};

  always @(posedge LATCH or negedge rst_n)
    if (!rst_n) latched <= 64'd0;
    else        latched <= shift;

  genvar i;
  generate
    for (i = 0; i < 64; i = i + 1) begin : oc
      assign OUT[i] = latched[i] ? 1'b0 : 1'bz;
    end
  endgenerate
endmodule
