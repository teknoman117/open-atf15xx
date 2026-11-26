// Registered equality and magnitude compare of two registered 6-bit words. Regression design
// (2026-11-26): on the ATF1502AS the fitter used a macrocell for an input register *and* the
// cascade helper of its neighbour, wrote the helper's product terms there, but did not route
// their inputs into the block (panic in the PT writer).
module cmpreg(input clk, input [5:0] a, input [5:0] b, output reg eq, output reg lt);
  reg [5:0] ra, rb;
  always @(posedge clk) begin
    ra <= a;
    rb <= b;
    eq <= ra == rb;
    lt <= ra < rb;
  end
endmodule
