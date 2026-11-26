// Silicon test of clock-network sharing (fitter option clock_networks, default in the flow):
// a toggles on the rising, b on the falling edge of the same pin (P2 = GCLK2/OE2, bench D9).
// The vendor fitter would give b a PT clock; ours drives a second network from P2, inverted.
module vedges((* LOC = "2" *) input ck, (* LOC = "4" *) output reg a, (* LOC = "5" *) output reg b);
  always @(posedge ck) a <= ~a;
  always @(negedge ck) b <= ~b;
endmodule
