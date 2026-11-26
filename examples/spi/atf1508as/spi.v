// ATF1508AS-PLCC84 on the bench (breakout-atf1508as + the Uno, tools/silicon/README.md): CLK is the
// 66.6 MHz oscillator on GCLK1, and the prescaler is 17 bits wide for slow SPI clocks:
// PSEL 0..3 = SCK 16.7 MHz, 1.04 MHz, 1.02 kHz, 254 Hz (66.6 MHz / 4, / 2^6, / 2^16, / 2^18). The
// prescaler's tick is registered (REG_TICK) to meet 66.6 MHz at the -10 grade, and the bus strobes
// are synchronised to CLK (STROBE_SYNC). The slow selects are
// for running SPI through a JTAG session (host/jtag_spi.py): the pins then follow the design once
// per boundary scan (~0.3 ms), so each SCK half-period must span several scans.
// The SPI pins are the Uno's lines (SCK = pin 4 / D2, MOSI = 5 / D3, SS = 80 / D5, MISO = 81 / D4)
// and reset is GCLR (pin 1 / D8); the bus (D, RS, cs_n, wr_n, rd_n) is on pins no wire reaches, to
// be driven over JTAG (host/jtag_spi.py).
module spi(
  (* LOC = "1" *)  input  rst_n,
  (* LOC = "83" *) input  CLK,
  (* LOC = "46" *) input  wr_n,
  (* LOC = "48" *) input  rd_n,
  (* LOC = "33 34 35 36 37 39 40 41" *) inout [7:0] D,
  (* LOC = "44" *) input  RS,
  (* LOC = "45" *) input  cs_n,
  (* LOC = "49" *) output interrupt_n,
  (* LOC = "4" *)  output SPI_SCK,
  (* LOC = "5" *)  output SPI_MOSI,
  (* LOC = "81" *) input  SPI_MISO,
  (* LOC = "80" *) output SPI_SS
);
  wire [7:0] dout;
  wire doe, irq, mosi, mosi_oe;
  spi_master #(.PRE_W(17), .TAP1(5), .TAP2(15), .TAP3(17), .REG_TICK(1), .STROBE_SYNC(1)) m(
    .CLK(CLK), .RST(~rst_n), .WR(~wr_n), .RD(~rd_n), .CS(~cs_n), .RS(RS), .DIN(D), .DOUT(dout), .DOE(doe),
    .INT(irq), .SCK(SPI_SCK), .MOSI(mosi), .MOSI_OE(mosi_oe), .SS(SPI_SS), .MISO(SPI_MISO));
  assign D = doe ? dout : 8'bz;
  assign SPI_MOSI = mosi_oe ? mosi : 1'bz;
  assign interrupt_n = irq ? 1'b0 : 1'bz;
endmodule
