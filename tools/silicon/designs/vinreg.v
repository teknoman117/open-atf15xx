// Silicon test of input-register packing: r samples P80 on GCLK2 (P2) and should live in P80's
// own macrocell with D straight from the pad (D/Q fast path). q = r & e on P4, nq = ~r | e on
// P5 (so r stays a buried register), e = P81.
module vinreg((* LOC = "2" *) input ck, (* LOC = "80" *) input d, (* LOC = "1" *) input rst, (* LOC = "81" *) input e,
              (* LOC = "4" *) output q, (* LOC = "5" *) output nq);
  reg r;
  always @(posedge ck or posedge rst) if (rst) r <= 1'b0; else r <= d;
  assign q = r & e;
  assign nq = ~r | e;
endmodule
