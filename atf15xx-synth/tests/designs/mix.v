// latches, async preset, shared logic, bidirectional bus (prjbureau prototype)
module mix(input clk, input set, input le, input [3:0] a, input dir, inout [1:0] bus, output [1:0] lat, output y);
  // transparent latch
  reg [1:0] l;
  always @* if (le) l = a[1:0];
  assign lat = l;
  // flip-flop with asynchronous preset, shared logic
  wire shared = (a[0] & a[1]) | (a[2] ^ a[3]);
  reg r;
  always @(posedge clk or posedge set)
    if (set) r <= 1'b1; else r <= shared ^ bus[0];
  assign y = r & shared;
  // bidirectional bus: drive {r, shared} when dir, otherwise read
  assign bus = dir ? {r, shared} : 2'bzz;
endmodule
