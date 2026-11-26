// Silicon test of the carry-chain mapping (R4, spec F9.5): a 40-bit counter is wider than a
// block's UIM inputs, so its toggle functions need a shared carry node (en & q0..q30). A load to
// 40'hFF_FFFF_FFFA (ld = P81) puts the count just below the wrap, so a few GCLK2 pulses carry
// through every bit; all 40 registers are read back over boundary scan.
module vcnt40((* LOC = "2" *) input clk, (* LOC = "81" *) input ld, (* LOC = "80" *) input en,
              (* LOC = "4" *) output top, (* LOC = "5" *) output mid);
  reg [39:0] c;
  always @(posedge clk)
    if (ld) c <= 40'hFF_FFFF_FFFA;
    else if (en) c <= c + 40'd1;
  assign top = c[39];
  assign mid = c[31];
endmodule
