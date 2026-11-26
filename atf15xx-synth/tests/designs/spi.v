// Verilog port of the user's CUPL design ~/Projects/pld/cpld/atf1504as-spi/spi.pld
// (SPI master peripheral for an 8-bit bus, ATF1504AS PLCC44). Same pins and behaviour:
// CUPL `Pin 1 = !RST` (active-low pin) becomes the input rst_n with RST = ~rst_n, etc.
// The open-collector !INTERRUPT output is written as a tri-state that only drives low.
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
  wire RST = ~rst_n, WR = ~wr_n, RD = ~rd_n, CS = ~cs_n;

  reg WR_buf, RD_buf, RUNNING, SS_SELECT, IFLAG, IEN;
  reg [3:0] PRE, STATE;
  reg [1:0] PSEL;
  reg [8:0] DATA;

  wire CONTROL_LOAD = !RUNNING & CS & RS & WR & !WR_buf;
  wire DATA_LOAD    = !RUNNING & CS & !RS & WR & !WR_buf;
  wire DATA_READ    = !RUNNING & CS & !RS & RD & !RD_buf;
  wire P2  = RUNNING & (PSEL == 2'd0);
  wire P8  = RUNNING & (PSEL == 2'd1) & (&PRE[1:0]);
  wire P16 = RUNNING & (PSEL == 2'd2) & (&PRE[2:0]);
  wire P32 = RUNNING & (PSEL == 2'd3) & (&PRE[3:0]);
  wire COUNT = P2 | P8 | P16 | P32;
  wire DATA_SHIFT = COUNT & STATE[0];
  wire TC = COUNT & (&STATE);

  always @(posedge CLK or posedge RST)
    if (RST) begin
      WR_buf <= 0; RD_buf <= 0; RUNNING <= 0; SS_SELECT <= 0; PSEL <= 0; PRE <= 0; STATE <= 0;
      DATA <= 0; IFLAG <= 0; IEN <= 0;
    end else begin
      WR_buf <= WR;
      RD_buf <= RD;
      if (TC | DATA_LOAD) RUNNING <= DATA_LOAD;
      if (CONTROL_LOAD) begin PSEL <= D[3:2]; SS_SELECT <= D[0]; IEN <= D[6]; end
      // prescaler and state counters (CUPL .T toggle equations)
      if (RUNNING) PRE[0] <= ~PRE[0];
      if (RUNNING & PRE[0]) PRE[1] <= ~PRE[1];
      if (RUNNING & (&PRE[1:0])) PRE[2] <= ~PRE[2];
      if (RUNNING & (&PRE[2:0])) PRE[3] <= ~PRE[3];
      if (COUNT) STATE[0] <= ~STATE[0];
      if (COUNT & STATE[0]) STATE[1] <= ~STATE[1];
      if (COUNT & (&STATE[1:0])) STATE[2] <= ~STATE[2];
      if (COUNT & (&STATE[2:0])) STATE[3] <= ~STATE[3];
      if (COUNT & !STATE[0]) DATA[8] <= SPI_MISO;
      if (DATA_SHIFT | DATA_LOAD) DATA[7:0] <= DATA_LOAD ? D : DATA[8:1];
      if (TC | DATA_READ) IFLAG <= TC;
    end

  assign SPI_SCK = STATE[0];
  assign SPI_MOSI = SS_SELECT ? DATA[0] : 1'bz;
  assign SPI_SS = SS_SELECT;
  assign D = (CS & RD) ? (RS ? {IFLAG, IEN, 1'b0, RUNNING, PSEL, 1'b0, SS_SELECT} : DATA[7:0]) : 8'bz;
  assign interrupt_n = (IEN & IFLAG) ? 1'b0 : 1'bz;
endmodule
