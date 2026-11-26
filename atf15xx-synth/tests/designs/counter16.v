// 16-bit up/down counter with synchronous load, clock enable and async reset
module counter16(input clk, input rst_n, input en, input up, input load, input [3:0] d, output [15:0] q);
  reg [15:0] c;
  always @(posedge clk or negedge rst_n)
    if (!rst_n) c <= 16'd0;
    else if (load) c <= {12'd0, d};
    else if (en) c <= up ? c + 16'd1 : c - 16'd1;
  assign q = c;
endmodule
