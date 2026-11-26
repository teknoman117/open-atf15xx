// Silicon test: plain 20-bit counter on GCLK2 (P2, bench D9 bursts), async reset P1 (GCLR, D8).
// P84 (OE1, bench A5) used as a plain input selects which nibble is shown on P4/P5/P80/P81
// (bench D2/D3/D5/D4): sel = 0 -> c[3:0], sel = 1 -> c[19:16].
module vcnt20((* LOC = "2" *) input ck, (* LOC = "1" *) input rst, (* LOC = "84" *) input sel,
              (* LOC = "4" *) output o0, (* LOC = "5" *) output o1,
              (* LOC = "80" *) output o2, (* LOC = "81" *) output o3);
  reg [19:0] c;
  always @(posedge ck or posedge rst)
    if (rst) c <= 0; else c <= c + 1;
  assign {o3, o2, o1, o0} = sel ? c[19:16] : c[3:0];
endmodule
