// 16:1 multiplexer (16 products of 5 literals: needs cascade or foldback) and an 8-bit comparator
module mux16(input [15:0] in, input [3:0] s, input [7:0] a, input [7:0] b, output y, output eq, output lt);
  assign y = in[s];
  assign eq = a == b;
  assign lt = a < b;
endmodule
