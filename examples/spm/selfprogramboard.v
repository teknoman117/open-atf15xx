// Verilog port of SELFPROGRAMBOARD.pld (teknoman117/P80C550-SPM-CPLD @ 28f54f3, originally MPL-2.0,
// Nathaniel R Lewis) for the open ATF15xx flow; relicensed ISC by its author, like the rest of the project (LICENSE). Line-by-line translation of the CUPL equations;
// checked for equivalence against the vendor's fit of the original (see README.md).
// Target: ATF1504AS-PLCC44, JTAG pins used as I/O (jtag off), bus keeper off.
module selfprogramboard (
    (* LOC = "12 11 9 8 7 6 5 4" *) inout [7:0] D,      // D0..D7
    (* LOC = "13" *) output CODE_CS,
    (* LOC = "14" *) input CODE_RD,
    (* LOC = "34" *) input A0,
    (* LOC = "36 37 38 39 40 41" *) input [15:10] A,     // A10..A15
    (* LOC = "44" *) input RD,
    (* LOC = "43" *) input WR,
    (* LOC = "16 17 18 19 20 21" *) output [5:0] MA,
    (* LOC = "24" *) output MEM_RD,
    (* LOC = "25" *) output MEM_WR,
    (* LOC = "33" *) output SPI_INT,
    (* LOC = "29" *) output SPI_CLK,
    (* LOC = "31" *) output SPI_OUT,
    (* LOC = "32" *) input SPI_IN,
    (* LOC = "26 27 28" *) output [2:0] SPI_SS,
    (* LOC = "2" *) input CLK,
    (* LOC = "1" *) input RST
);
  // ---- ~RD edge detector
  reg RD_edge;
  always @(posedge CLK or negedge RST) if (!RST) RD_edge <= 1'b0; else RD_edge <= RD;
  wire RD_rising = RD & ~RD_edge;

  // ---- address decoding (only A15..A10 and A0 are wired)
  wire Configure_Register   = A == 6'b100000;                 // 8000..83FF
  wire SPI_Data_Register    = A == 6'b100001 & ~A0;
  wire SPI_Control_Register = A == 6'b100001 &  A0;
  wire Memory_Window        = A[15:13] == 3'b101;              // A000..BFFF

  // ---- configure register (clocked by WR)
  reg Offboard_Select;
  reg [5:0] WA;
  always @(posedge WR or negedge RST)
    if (!RST) begin Offboard_Select <= 1'b0; WA <= 6'd0; end
    else if (Configure_Register) begin Offboard_Select <= D[6]; WA <= D[5:0]; end

  // three code reads before switching to SPM memory
  reg [2:0] Delay;
  always @(posedge CODE_RD or negedge RST)
    if (!RST) Delay <= 3'd0; else Delay <= {Delay[1:0], Offboard_Select};
  assign CODE_CS = Delay[2];

  // ---- SPM memory logic
  assign MEM_RD = ~((Memory_Window & ~RD) | (CODE_CS & ~CODE_RD));
  assign MEM_WR = ~(Memory_Window & ~WR);
  wire code_fetch = CODE_CS & ~CODE_RD;
  assign MA = code_fetch ? {3'b000, A[15:13]} : WA;

  // ---- SPI
  reg SPI_Running, SPI_Interrupt_Enable, SPI_Interrupt_Flag;
  reg [1:0] SPI_Prescaler, SS;
  wire SPI_Data_Load    = ~WR & SPI_Data_Register;
  wire SPI_Control_Load = ~WR & SPI_Control_Register;

  always @(posedge WR or negedge RST)
    if (!RST) SPI_Prescaler <= 2'd0;
    else if (SPI_Control_Register & ~SPI_Running) SPI_Prescaler <= D[1:0];

  // prescaler: bits held in reset unless the selected rate needs them
  reg [5:0] Prescaler;
  wire ps_rst01 = ~RST | SPI_Prescaler == 2'd0;
  wire ps_rst23 = ps_rst01 | SPI_Prescaler == 2'd1;
  wire ps_rst45 = ps_rst23 | SPI_Prescaler == 2'd2;
  always @(posedge CLK or posedge ps_rst01)
    if (ps_rst01) Prescaler[1:0] <= 2'd0;
    else begin
      if (SPI_Running) Prescaler[0] <= ~Prescaler[0];
      if (SPI_Running & Prescaler[0]) Prescaler[1] <= ~Prescaler[1];
    end
  always @(posedge CLK or posedge ps_rst23)
    if (ps_rst23) Prescaler[3:2] <= 2'd0;
    else begin
      if (SPI_Running & &Prescaler[1:0]) Prescaler[2] <= ~Prescaler[2];
      if (SPI_Running & &Prescaler[2:0]) Prescaler[3] <= ~Prescaler[3];
    end
  always @(posedge CLK or posedge ps_rst45)
    if (ps_rst45) Prescaler[5:4] <= 2'd0;
    else begin
      if (SPI_Running & &Prescaler[3:0]) Prescaler[4] <= ~Prescaler[4];
      if (SPI_Running & &Prescaler[4:0]) Prescaler[5] <= ~Prescaler[5];
    end

  wire SPI_Count_Enable = (SPI_Prescaler == 2'd0 & SPI_Running)
                        | (SPI_Prescaler == 2'd1 & SPI_Running & &Prescaler[1:0])
                        | (SPI_Prescaler == 2'd2 & SPI_Running & &Prescaler[3:0])
                        | (SPI_Prescaler == 2'd3 & SPI_Running & &Prescaler[5:0]);

  reg [3:0] CNT;
  always @(posedge CLK or negedge RST)
    if (!RST) CNT <= 4'd0;
    else if (SPI_Count_Enable) CNT <= CNT + 4'd1;     // CNTn.T = enable & CNT[n-1:0] all ones
  assign SPI_CLK = CNT[0];
  wire SPI_Last_Edge = SPI_Count_Enable & &CNT;

  // ---- SPI data registers
  // MOSI: shift left, parallel load through the asynchronous set/reset while SPI_Data_Load
  reg [7:0] MOSI;
  genvar i;
  generate
    for (i = 0; i < 8; i = i + 1) begin : mosi_bit
      wire ar = ~RST | (SPI_Data_Load & ~D[i]);
      wire ap = SPI_Data_Load & D[i];
      always @(posedge CLK or posedge ar or posedge ap)
        if (ar) MOSI[i] <= 1'b0;
        else if (ap) MOSI[i] <= 1'b1;
        else if (SPI_Count_Enable & CNT[0]) MOSI[i] <= (i == 0) ? 1'b0 : MOSI[i - 1];
    end
  endgenerate
  assign SPI_OUT = MOSI[7];

  reg [7:0] MISO;
  always @(posedge CLK or negedge RST)
    if (!RST) MISO <= 8'd0;
    else if (SPI_Count_Enable & ~CNT[0]) MISO <= {MISO[6:0], SPI_IN};

  // ---- SPI control register
  always @(posedge WR or negedge RST)
    if (!RST) SPI_Interrupt_Enable <= 1'b0;
    else if (SPI_Control_Register) SPI_Interrupt_Enable <= D[6];

  wire if_ar = (SPI_Control_Load & D[7]) | (SPI_Data_Register & RD_rising) | SPI_Data_Load | ~RST;
  always @(posedge CLK or posedge if_ar)
    if (if_ar) SPI_Interrupt_Flag <= 1'b0;
    else if (SPI_Last_Edge) SPI_Interrupt_Flag <= 1'b1;

  assign SPI_INT = (SPI_Interrupt_Enable & SPI_Interrupt_Flag) ? 1'b0 : 1'bz;

  wire run_ar = ~RST | SPI_Interrupt_Flag;
  always @(posedge WR or posedge run_ar)
    if (run_ar) SPI_Running <= 1'b0;
    else if (SPI_Data_Register) SPI_Running <= 1'b1;

  always @(posedge WR or negedge RST)
    if (!RST) SS <= 2'd0;
    else if (SPI_Control_Register & ~SPI_Running) SS <= D[3:2];

  assign SPI_SS[0] = ~(SS == 2'd1);
  assign SPI_SS[1] = ~(SS == 2'd2);
  assign SPI_SS[2] = ~(SS == 2'd3);

  // ---- multiplexed bus data read
  wire [7:0] cfg_val = {1'b0, Offboard_Select, WA};
  wire [7:0] ctl_val = {SPI_Interrupt_Flag, SPI_Interrupt_Enable, SPI_Running, 1'b0, SS, SPI_Prescaler};
  wire [7:0] dval = ({8{Configure_Register}} & cfg_val) | ({8{SPI_Data_Register}} & MISO) | ({8{SPI_Control_Register}} & ctl_val);
  wire oe_lo = ~RD & (Configure_Register | SPI_Data_Register | SPI_Control_Register);
  wire oe_hi = ~RD & (SPI_Data_Register | SPI_Control_Register);
  assign D[6:0] = oe_lo ? dval[6:0] : 7'bz;
  assign D[7]   = oe_hi ? dval[7] : 1'bz;
endmodule
