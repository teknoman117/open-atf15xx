// Silicon test of the global clock networks: a toggles on GCLK2 (P2, bench D9), b on GCLK3 (P81,
// bench D4). Variants of the JED re-source the GCLK3 network from the GCLK2 pin, with and without
// its invert bit (tools/silicon/bench/exp_gclk_polarity.py).
module vpol((* LOC = "2" *) input ck2, (* LOC = "81" *) input ck3,
            (* LOC = "4" *) output reg a, (* LOC = "5" *) output reg b);
  always @(posedge ck2) a <= ~a;
  always @(posedge ck3) b <= ~b;
endmodule
