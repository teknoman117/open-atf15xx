// Latches and flip-flops with asynchronous set and reset (alone and together)
module latchsr(input clk, input le, input s, input r, input [3:0] d, output [3:0] q, output [1:0] l, output ql);
  reg [3:0] f;
  always @(posedge clk or posedge s or posedge r)
    if (r) f[0] <= 1'b0; else if (s) f[0] <= 1'b1; else f[0] <= d[0];
  always @(posedge clk or posedge r) if (r) f[1] <= 1'b0; else f[1] <= d[1];
  always @(posedge clk or posedge s) if (s) f[2] <= 1'b1; else f[2] <= d[2] ^ d[3];
  always @(negedge clk) f[3] <= d[3] & d[0];
  assign q = f;
  reg [1:0] lt;
  always @* if (le) lt = d[1:0];
  assign l = lt;
  reg lr;
  always @* if (r) lr = 1'b0; else if (le) lr = d[2];
  assign ql = lr;
endmodule
