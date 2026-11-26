// Plain behavioural 64-bit counter with enable, synchronous clear and asynchronous reset.
// The carry of the top bits depends on 64 signals: more than a block's 40 UIM inputs, so the
// flow must keep carry nodes (regression for wide-support decomposition).
module counter64(input clk, input rst_n, input clr, input en, output [63:0] q);
  reg [63:0] c;
  always @(posedge clk or negedge rst_n)
    if (!rst_n) c <= 64'd0;
    else if (clr) c <= 64'd0;
    else if (en) c <= c + 64'd1;
  assign q = c;
endmodule
