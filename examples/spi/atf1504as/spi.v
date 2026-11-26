// ATF1504AS-PLCC44: the original board's pinout and prescaler (CLK / 2, 8, 16, 32), as in the
// user's spi.pld. Active-low pins are named *_n; the open-collector !INTERRUPT only drives low.
module spi(
  (* LOC = "1" *)  input  rst_n,
  (* LOC = "2" *)  input  CLK,
  (* LOC = "43" *) input  wr_n,
  (* LOC = "44" *) input  rd_n,
  (* LOC = "4 5 6 7 8 9 11 12" *) inout [7:0] D,
  (* LOC = "13" *) input  RS,
  (* LOC = "14" *) input  cs_n,
  (* LOC = "16" *) output interrupt_n,
  (* LOC = "17" *) output SPI_SCK,
  (* LOC = "18" *) output SPI_MOSI,
  (* LOC = "19" *) input  SPI_MISO,
  (* LOC = "20" *) output SPI_SS
);
  wire [7:0] dout;
  wire doe, irq, mosi, mosi_oe;
  spi_master #(.PRE_W(4), .TAP1(2), .TAP2(3), .TAP3(4)) m(
    .CLK(CLK), .RST(~rst_n), .WR(~wr_n), .RD(~rd_n), .CS(~cs_n), .RS(RS), .DIN(D), .DOUT(dout), .DOE(doe),
    .INT(irq), .SCK(SPI_SCK), .MOSI(mosi), .MOSI_OE(mosi_oe), .SS(SPI_SS), .MISO(SPI_MISO));
  assign D = doe ? dout : 8'bz;
  assign SPI_MOSI = mosi_oe ? mosi : 1'bz;
  assign interrupt_n = irq ? 1'b0 : 1'bz;
endmodule
