// 4-bit counter: clock enable, async reset, tri-state outputs, carry (prjbureau prototype)
module counter4(input clk, input rst, input en, input oe, output [3:0] q, output carry);
  reg [3:0] c;
  always @(posedge clk or posedge rst)
    if (rst) c <= 4'd0;
    else if (en) c <= c + 4'd1;
  assign q = oe ? c : 4'bz;
  assign carry = &c & en;
endmodule
