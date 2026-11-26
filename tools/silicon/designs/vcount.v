// Silicon test: 4-bit counter clocked by the bench (P81 = GCLK3), enable P80, async reset P1 (GCLR).
// Outputs q[1] on P4 and q[3] on P5 (the bench can read two lines).
module vcount((* LOC = "81" *) input ck, (* LOC = "80" *) input en, (* LOC = "1" *) input rst,
              (* LOC = "4" *) output q1, (* LOC = "5" *) output q3);
  reg [3:0] q;
  always @(posedge ck or posedge rst)
    if (rst) q <= 0; else if (en) q <= q + 1;
  assign q1 = q[1];
  assign q3 = q[3];
endmodule
