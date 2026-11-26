// Wide XOR: 16-bit parity, plus a registered running parity
module parity(input clk, input [15:0] a, output p, output reg pr);
  assign p = ^a;
  always @(posedge clk) pr <= pr ^ (^a[7:0]);
endmodule
