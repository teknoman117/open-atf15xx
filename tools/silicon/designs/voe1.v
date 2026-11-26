// Silicon test: P84 (OE1, bench A5) as a global output enable. P4 = 0 and P5 = d (P81, bench D4)
// are driven only while oe = 1; otherwise high-impedance (the bench detects Z with its pull-up).
module voe1((* LOC = "84" *) input oe, (* LOC = "81" *) input d,
            (* LOC = "4" *) output p4, (* LOC = "5" *) output p5);
  assign p4 = oe ? 1'b0 : 1'bz;
  assign p5 = oe ? d : 1'bz;
endmodule
