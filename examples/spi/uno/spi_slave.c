/* Uno R3 (ATmega328P, 16 MHz) firmware for the SPI example: the SPI slave the ATF1508AS-PLCC84
 * bench variant talks to, plus the lines the example needs (reset, unused dedicated inputs).
 * Wiring (the bench's, tools/silicon/README.md): D2 = SCK (pin 4), D3 = MOSI (pin 5),
 * D4 = MISO (pin 81, driven), D5 = SS (pin 80, active high), D8 = rst_n (GCLR, pin 1),
 * D9 = GCLK2/OE2 (pin 2), A5 = OE1 (pin 84).
 *
 * SPI: mode 0 (sample MOSI on the rising SCK edge, next MISO bit on the falling edge), LSB first,
 * 8 bits per byte, bit-banged on INT0 (D2, both edges): fine for the example's 1 kHz SCK, up to a
 * few tens of kHz. The slave answers a chosen first byte, then every byte it received before
 * (echo), so the master can check both directions.
 *
 * Serial 115200 8N1, one command per line, one reply line:
 *   ?                -> "spi-slave 1"
 *   m <pin> <i|0|1>  -> D8, D9 or A5 (19): input, drive low, drive high              -> "ok"
 *   x <hex>          -> slave on, first reply <hex>                                  -> "ok"
 *   x -              -> slave off (MISO released)                                    -> "ok"
 *   y                -> bytes received since the last y (at most 32), hex, and the SCK
 *                       edges seen while selected  -> "y <n> <hex...> / <rising> <falling>"
 * Nothing is driven at reset.
 *
 *   make -C examples/spi/uno flash
 */
#include <avr/io.h>
#include <avr/interrupt.h>
#include <stdlib.h>
#include <string.h>

static void uart_init(void) { UCSR0A = _BV(U2X0); UBRR0 = 16; UCSR0B = _BV(RXEN0) | _BV(TXEN0); }
static void tx(char c) { while (!(UCSR0A & _BV(UDRE0))); UDR0 = c; }
static void puts_(const char *s) { while (*s) tx(*s++); }
static void putl(unsigned long v) { char b[12]; ultoa(v, b, 10); puts_(b); }
static int rx(void) { while (!(UCSR0A & _BV(RXC0))); return UDR0; }

static volatile uint8_t spi_n, spi_rx, spi_tx, spi_cnt, spi_buf[32];
static volatile uint16_t spi_rise, spi_fall;

static void miso(uint8_t b) { if (b) PORTD |= _BV(4); else PORTD &= ~_BV(4); }

ISR(INT0_vect) {
    uint8_t v = PIND;
    if (!(v & _BV(5))) { spi_n = 0; spi_rx = 0; miso(spi_tx & 1); return; }   /* not selected */
    if (v & _BV(2)) {                                   /* SCK rising: sample MOSI */
        spi_rise++;
        if (v & _BV(3)) spi_rx |= (uint8_t)(1 << spi_n);
        if (++spi_n == 8) {
            if (spi_cnt < sizeof spi_buf) spi_buf[spi_cnt++] = spi_rx;
            spi_tx = spi_rx; spi_n = 0; spi_rx = 0;
        }
    } else {                                            /* SCK falling: next MISO bit */
        spi_fall++;
        miso((spi_tx >> spi_n) & 1);
    }
}

/* the control lines: D8 = PB0, D9 = PB1, A5 = PC5 */
static int line(int p, volatile uint8_t **ddr, volatile uint8_t **port, uint8_t *bit) {
    if (p == 8 || p == 9) { *ddr = &DDRB; *port = &PORTB; *bit = p == 8 ? _BV(0) : _BV(1); return 1; }
    if (p == 19) { *ddr = &DDRC; *port = &PORTC; *bit = _BV(5); return 1; }
    return 0;
}

static char buf[64];

int main(void) {
    DDRD &= ~0x3c; PORTD &= ~0x3c; DDRB &= ~3; PORTB &= ~3; DDRC &= ~0x20; PORTC &= ~0x20;   /* all high-Z */
    uart_init(); sei();
    puts_("spi-slave ready\r\n");
    for (;;) {
        int n = 0, c;
        while ((c = rx()) != '\n') { if (c != '\r' && n < 63) buf[n++] = c; }
        buf[n] = 0;
        char *a = buf, *t[3]; int k = 0;
        while (*a && k < 3) { while (*a == ' ') a++; if (!*a) break; t[k++] = a; while (*a && *a != ' ') a++; if (*a) *a++ = 0; }
        if (!k) continue;
        if (t[0][0] == '?') { puts_("spi-slave 1\r\n"); continue; }
        if (t[0][0] == 'm' && k == 3) {
            volatile uint8_t *ddr, *port; uint8_t bit;
            if (!line(atoi(t[1]), &ddr, &port, &bit)) { puts_("err pin\r\n"); continue; }
            char m = t[2][0];
            if (m == 'i') { *ddr &= ~bit; *port &= ~bit; }
            else if (m == '0') { *port &= ~bit; *ddr |= bit; }
            else if (m == '1') { *port |= bit; *ddr |= bit; }
            else { puts_("err mode\r\n"); continue; }
            puts_("ok\r\n"); continue;
        }
        if (t[0][0] == 'x' && k == 2) {
            EIMSK &= ~_BV(INT0);
            if (t[1][0] == '-') { DDRD &= ~_BV(4); PORTD &= ~_BV(4); puts_("ok\r\n"); continue; }
            DDRD &= ~(_BV(2) | _BV(3) | _BV(5)); PORTD &= ~(_BV(2) | _BV(3) | _BV(5));
            spi_tx = (uint8_t)strtol(t[1], 0, 16); spi_n = 0; spi_rx = 0; spi_cnt = 0;
            miso(spi_tx & 1); DDRD |= _BV(4);
            EICRA = (EICRA & ~3) | _BV(ISC00); EIFR = _BV(INTF0); EIMSK |= _BV(INT0);
            puts_("ok\r\n"); continue;
        }
        if (t[0][0] == 'y') {
            const char *h = "0123456789abcdef";
            cli(); uint8_t n2 = spi_cnt, b[32]; memcpy(b, (const void *)spi_buf, n2); spi_cnt = 0;
            uint16_t r = spi_rise, f = spi_fall; spi_rise = spi_fall = 0; sei();
            puts_("y "); putl(n2);
            for (int i = 0; i < n2; i++) { tx(' '); tx(h[b[i] >> 4]); tx(h[b[i] & 15]); }
            puts_(" / "); putl(r); tx(' '); putl(f); puts_("\r\n"); continue;
        }
        puts_("err cmd\r\n");
    }
}
