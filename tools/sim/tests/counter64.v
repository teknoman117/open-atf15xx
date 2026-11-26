// 64-bit counter: the "heat death" test for the time-bounded comparison (tools/sim/vcompare.py).
// Equivalence from power-up cannot be reached by simulation (2^64 cycles); it needs a register
// correspondence (state injection, correspondence induction).
//
// Behaviour: a 64-bit up counter with asynchronous reset; `top` = c[63:60], `carry` = &c[62:0].
// An ATF15xx function block has 40 UIM inputs, so a flat top bit (63 inputs) cannot be fitted.
// Registered carry-lookahead flags keep every function at <= 33 inputs while staying
// cycle-exact: f16 = (c[15:0] == all ones), f32 = (c[31:0] == all ones),
// f48 = (c[47:0] == all ones), g16 = (c[15:0] == 16'hFFFE), each computed one cycle ahead.
module counter64(input clk, input rst, output [3:0] top, output carry);
  reg [63:0] c = 64'd0;
  reg f16 = 1'b0, f32 = 1'b0, f48 = 1'b0, g16 = 1'b0;
  always @(posedge clk or posedge rst)
    if (rst) begin
      c <= 64'd0; f16 <= 1'b0; f32 <= 1'b0; f48 <= 1'b0; g16 <= 1'b0;
    end else begin
      c[15:0] <= c[15:0] + 16'd1;
      if (f16) c[31:16] <= c[31:16] + 16'd1;
      if (f32) c[47:32] <= c[47:32] + 16'd1;
      if (f48) c[63:48] <= c[63:48] + 16'd1;
      g16 <= (c[15:0] == 16'hFFFD);
      f16 <= (c[15:0] == 16'hFFFE);
      f32 <= (c[31:16] == 16'hFFFF) & (c[15:0] == 16'hFFFE);
      f48 <= (c[47:32] == 16'hFFFF) & (c[31:16] == 16'hFFFF) & g16;
    end
  assign top = c[63:60];
  assign carry = f48 & (&c[62:48]);
endmodule
