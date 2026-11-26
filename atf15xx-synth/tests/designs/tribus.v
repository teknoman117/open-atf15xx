// 8-bit bidirectional data bus with a register: write on strobe, drive the bus when rd
module tribus(input clk, input wr, input rd, input [1:0] sel, inout [7:0] data, output [7:0] led);
  reg [7:0] r0, r1;
  always @(posedge clk) if (wr && sel == 2'd0) r0 <= data;
  always @(posedge clk) if (wr && sel == 2'd1) r1 <= data;
  wire [7:0] rv = sel[0] ? r1 : r0;
  assign data = (rd && !wr) ? rv : 8'bz;
  assign led = r0 ^ r1;
endmodule
