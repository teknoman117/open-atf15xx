// Registered equality and magnitude compare of two registered 20-bit words. Regression design
// for wide compares (2026-11-26, spec F9.12): one flat cover reads 3 signals per bit (60), more
// than a block's 40 UIM inputs; the word-level mapping builds it from 3- or 4-bit pieces. Before,
// it did not fit the ATF1504AS (> 400 s, then fan-in failures); now 54/64 macrocells.
module cmpwide(input clk, input [19:0] a, input [19:0] b, output reg eq, output reg lt);
  reg [19:0] ra, rb;
  always @(posedge clk) begin
    ra <= a;
    rb <= b;
    eq <= ra == rb;
    lt <= ra < rb;
  end
endmodule
