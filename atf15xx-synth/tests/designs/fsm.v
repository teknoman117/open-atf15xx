// Moore FSM: detects the serial pattern 1011 (overlapping), binary-encoded state
module fsm(input clk, input rst, input x, output reg hit, output [2:0] state);
  localparam S0 = 3'd0, S1 = 3'd1, S10 = 3'd2, S101 = 3'd3, S1011 = 3'd4;
  reg [2:0] s, n;
  always @* begin
    case (s)
      S0:    n = x ? S1 : S0;
      S1:    n = x ? S1 : S10;
      S10:   n = x ? S101 : S0;
      S101:  n = x ? S1011 : S10;
      S1011: n = x ? S1 : S10;
      default: n = S0;
    endcase
  end
  always @(posedge clk) if (rst) s <= S0; else s <= n;
  always @(posedge clk) hit <= (n == S1011) && !rst;
  assign state = s;
endmodule
