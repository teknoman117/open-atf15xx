// 32-bit shift register with parallel load of the low byte, serial in/out
module shift32(input clk, input load, input sin, input [7:0] d, output sout, output [7:0] tap);
  reg [31:0] r;
  always @(posedge clk)
    if (load) r <= {r[23:0], d};
    else r <= {r[30:0], sin};
  assign sout = r[31];
  assign tap = r[31:24];
endmodule
