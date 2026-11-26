// SPI master peripheral for an 8-bit microprocessor bus: the Verilog form of the user's CUPL design
// atf1504as-spi (spi.pld, originally MPL-2.0), with the prescaler made a parameter. This port is
// relicensed ISC by its author, like the rest of the project (LICENSE).
//
// Registers (RS selects): RS = 0 data (write: load and start a transfer; read: the received byte),
// RS = 1 control/status: bit 0 slave select, bits 3:2 clock select (PSEL), bit 6 interrupt enable;
// status adds bit 4 RUNNING and bit 7 the interrupt flag (set at the end of a transfer, cleared by
// reading the data register). Writes and reads are edge-detected on CLK, so a strobe may last any
// number of clocks.
//
// SPI: mode 0 (SCK idles low, MOSI changes on the falling edge, MISO sampled on the rising edge),
// LSB first, 8 bits. SCK = CLK / 2^(TAPn + 1) for PSEL = n, TAP0 = 0 (CLK / 2). The original
// design has PRE_W = 4 and TAPs 2, 3, 4 (CLK / 8, / 16, / 32).
//
// REG_TICK = 1 registers the prescaler's tick (one CLK later, same rate): the tap selection then no
// longer sits in front of the state and data clock enables, for a faster CLK. With PSEL = 0 the
// tick is then every second CLK (SCK = CLK / 4). REG_TICK = 0 is the original design.
//
// STROBE_SYNC = 1 passes /WR and /RD through two flip-flops before the edge detection: a strobe
// asynchronous to a fast CLK is then never sampled half-way, and the data bus is latched 2-3 CLKs
// after the strobe switched, not in the switching transient (with the 66.6 MHz bench clock and
// strobes driven over JTAG, about one write in fifty latched a wrong bit without it).
module spi_master #(
  parameter PRE_W = 4,
  parameter TAP1 = 2,
  parameter TAP2 = 3,
  parameter TAP3 = 4,
  parameter REG_TICK = 0,
  parameter STROBE_SYNC = 0
) (
  input        CLK,
  input        RST,        // asynchronous, active high
  input        WR, RD, CS, RS,   // bus strobes and register select, active high
  input  [7:0] DIN,
  output [7:0] DOUT,
  output       DOE,        // drive DOUT onto the bus
  output       INT,        // interrupt request, active high
  output       SCK, MOSI, MOSI_OE, SS,
  input        MISO
);
  reg WR_buf, RD_buf, RUNNING, SS_SELECT, IFLAG, IEN;
  wire WRs, RDs;   // the strobes as the edge detection sees them
  generate
    if (STROBE_SYNC) begin : strobe_sync
      reg [1:0] wr_s, rd_s;
      always @(posedge CLK or posedge RST)
        if (RST) begin wr_s <= 2'b00; rd_s <= 2'b00; end
        else begin wr_s <= {wr_s[0], WR}; rd_s <= {rd_s[0], RD}; end
      assign WRs = wr_s[1];
      assign RDs = rd_s[1];
    end else begin : strobe_direct
      assign WRs = WR;
      assign RDs = RD;
    end
  endgenerate
  reg [PRE_W-1:0] PRE;
  reg [3:0] STATE;
  reg [1:0] PSEL;
  reg [8:0] DATA;

  wire CONTROL_LOAD = !RUNNING & CS & RS & WRs & !WR_buf;
  wire DATA_LOAD    = !RUNNING & CS & !RS & WRs & !WR_buf;
  wire DATA_READ    = !RUNNING & CS & !RS & RDs & !RD_buf;

  // prescaler: a toggle chain; carry[k] = RUNNING and PRE[k-1:0] all ones
  wire [PRE_W:0] carry;
  assign carry[0] = RUNNING;
  genvar i;
  generate
    for (i = 0; i < PRE_W; i = i + 1) begin : pre
      assign carry[i + 1] = carry[i] & PRE[i];
      always @(posedge CLK or posedge RST)
        if (RST) PRE[i] <= 1'b0;
        else if (carry[i]) PRE[i] <= ~PRE[i];
    end
  endgenerate

  wire TICK = PSEL == 2'd0 ? carry[0] : PSEL == 2'd1 ? carry[TAP1] : PSEL == 2'd2 ? carry[TAP2] : carry[TAP3];
  wire COUNT;
  generate
    if (REG_TICK) begin : reg_tick
      reg tick_q;
      always @(posedge CLK or posedge RST)
        if (RST) tick_q <= 1'b0;
        else tick_q <= TICK & !tick_q;
      assign COUNT = tick_q & RUNNING;   // no count after the transfer has ended
    end else begin : comb_tick
      assign COUNT = TICK;
    end
  endgenerate
  wire DATA_SHIFT = COUNT & STATE[0];
  wire TC = COUNT & (&STATE);

  always @(posedge CLK or posedge RST)
    if (RST) begin
      WR_buf <= 0; RD_buf <= 0; RUNNING <= 0; SS_SELECT <= 0; PSEL <= 0; STATE <= 0;
      DATA <= 0; IFLAG <= 0; IEN <= 0;
    end else begin
      WR_buf <= WRs;
      RD_buf <= RDs;
      if (TC | DATA_LOAD) RUNNING <= DATA_LOAD;
      if (CONTROL_LOAD) begin PSEL <= DIN[3:2]; SS_SELECT <= DIN[0]; IEN <= DIN[6]; end
      if (COUNT) STATE[0] <= ~STATE[0];
      if (COUNT & STATE[0]) STATE[1] <= ~STATE[1];
      if (COUNT & (&STATE[1:0])) STATE[2] <= ~STATE[2];
      if (COUNT & (&STATE[2:0])) STATE[3] <= ~STATE[3];
      if (COUNT & !STATE[0]) DATA[8] <= MISO;
      if (DATA_SHIFT | DATA_LOAD) DATA[7:0] <= DATA_LOAD ? DIN : DATA[8:1];
      if (TC | DATA_READ) IFLAG <= TC;
    end

  assign SCK = STATE[0];
  assign MOSI = DATA[0];
  assign MOSI_OE = SS_SELECT;
  assign SS = SS_SELECT;
  assign DOE = CS & RD;
  assign DOUT = RS ? {IFLAG, IEN, 1'b0, RUNNING, PSEL, 1'b0, SS_SELECT} : DATA[7:0];
  assign INT = IEN & IFLAG;
endmodule
