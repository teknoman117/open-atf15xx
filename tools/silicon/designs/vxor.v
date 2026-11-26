// Silicon test of XOR synthesis (spec F4.6b): y = 4-input parity (8 PTs as a plain cover, 5 with
// the macrocell XOR: g = 3-input parity, PT2 = one input); z = registered, inverted 3-input
// parity on GCLK2 (active-low cover through the XOR: !f = !g ^ p).
module vxor((* LOC = "80" *) input d, (* LOC = "81" *) input e, (* LOC = "84" *) input a, (* LOC = "1" *) input b,
            (* LOC = "2" *) input ck, (* LOC = "4" *) output y, (* LOC = "5" *) output reg z);
  assign y = d ^ e ^ a ^ b;
  always @(posedge ck) z <= ~(d ^ e ^ a);
endmodule
