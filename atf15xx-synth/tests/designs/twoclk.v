// Two clock domains (one on a negative edge), each with its own counter, and a cross-domain flag
module twoclk(input clka, input clkb, input rst_n, output [3:0] qa, output [3:0] qb, output flag);
  reg [3:0] a, b;
  always @(posedge clka or negedge rst_n) if (!rst_n) a <= 0; else a <= a + 1;
  always @(negedge clkb or negedge rst_n) if (!rst_n) b <= 0; else b <= b + 3;
  reg f;
  always @(posedge clkb) f <= a[3];
  assign qa = a; assign qb = b; assign flag = f;
endmodule
