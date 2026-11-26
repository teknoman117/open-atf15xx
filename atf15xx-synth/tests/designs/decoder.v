// 4-to-16 decoder with active-low enable and open-collector style active-low outputs
module decoder(input [3:0] a, input en_n, output [15:0] y_n);
  assign y_n = en_n ? 16'hffff : ~(16'd1 << a);
endmodule
