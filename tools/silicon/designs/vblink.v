// Silicon test: Verilog version of the user's CUPL blink (breakout-atf1508as, 66.6 MHz on GCLK1).
// led toggles every 1,000,000 clocks (33.3 Hz at 66.6 MHz); nled = ~led on P5.
// (A version with an extra 21-bit free-running counter did not fit: UIM routing of block A at 40
// inputs, the known placer weakness -- docs/HANDOFF.md next steps.)
module vblink((* LOC = "83" *) input clk, (* LOC = "1" *) input rst,
              (* LOC = "4" *) output reg led, (* LOC = "5" *) output nled);
  reg [19:0] c;
  always @(posedge clk or posedge rst)
    if (rst) begin c <= 0; led <= 0; end
    else if (c == 20'd999999) begin c <= 0; led <= ~led; end
    else c <= c + 1;
  assign nled = ~led;
endmodule
