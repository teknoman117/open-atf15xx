/* Uno R3 (ATmega328P, 16 MHz) test instrument for the ATF15xx breakout.
 * Lines: D2..D5 (PD2..PD5), D8 (PB0), D9 (PB1 = OC1A), A5 = 19 (PC5). Serial 115200 8N1, one command per line, one reply line:
 *   ?                 -> "bench 3 D2 D3 D4 D5 D8 D9 A5"
 *   m <pin> <i|u|0|1> -> mode: input, input+pull-up, drive low, drive high      -> "ok"
 *   r                 -> levels of D2 D3 D4 D5 D8 D9 A5 as 7 chars, e.g. "r 1001100"
 *   f <hz>            -> free-running square wave on D9 from Timer1 (CTC toggle; 0 = stop, D9 then
 *                        returns to a low output). Exact frequency F_CPU/(2*(OCR1A+1)) -> "f <actual hz>"
 *   b <n>             -> exactly n pulses on D9 (must be an output), 0.5 us high/low (1 MHz), irqs off -> "ok"
 *   c <pin> <ms>      -> count rising edges on <pin> for <ms> ms (1..60000)    -> "c <count>"
 *   p <pin> <n> <us>  -> n positive pulses (high <us>, low <us>) on an output   -> "ok"
 *   s <pin> <n>       -> n samples of all lines, as fast as possible, hex      -> "s <hex...>"
 * Pins are given as Arduino numbers (2, 3, 4, 5, 8, 9, 19 = A5). Edge counting uses pin-change interrupts
 * (max. ~100 kHz). Nothing is driven at reset: every line starts as an input.
 */
#include <avr/io.h>
#include <avr/interrupt.h>
#include <util/delay.h>
#include <stdlib.h>
#include <string.h>

static void uart_init(void) { UCSR0A = _BV(U2X0); UBRR0 = 16; UCSR0B = _BV(RXEN0) | _BV(TXEN0); }
static void tx(char c) { while (!(UCSR0A & _BV(UDRE0))); UDR0 = c; }
static void puts_(const char *s) { while (*s) tx(*s++); }
static void putl(unsigned long v) { char b[12]; ultoa(v, b, 10); puts_(b); }
static int rx(void) { while (!(UCSR0A & _BV(RXC0))); return UDR0; }

/* pin -> port/bit */
static int pin_ok(int p) { return (p >= 2 && p <= 5) || p == 8 || p == 9 || p == 19; }
static volatile uint8_t *ddr(int p) { return p == 8 || p == 9 ? &DDRB : p == 19 ? &DDRC : &DDRD; }
static volatile uint8_t *port(int p) { return p == 8 || p == 9 ? &PORTB : p == 19 ? &PORTC : &PORTD; }
static volatile uint8_t *pinr(int p) { return p == 8 || p == 9 ? &PINB : p == 19 ? &PINC : &PIND; }
static uint8_t bit(int p) { return p == 8 ? _BV(0) : p == 9 ? _BV(1) : p == 19 ? _BV(5) : _BV(p); }
static int level(int p) { return (*pinr(p) & bit(p)) ? 1 : 0; }

static volatile uint32_t edges; static volatile uint8_t want, last;
ISR(PCINT2_vect) { uint8_t v = PIND & want; if (v & ~last) edges++; last = v; }
ISR(PCINT0_vect) { uint8_t v = PINB & want; if (v & ~last) edges++; last = v; }
ISR(PCINT1_vect) { uint8_t v = PINC & want; if (v & ~last) edges++; last = v; }

static char line[64];
static int toks(char **t, int max) {
    int n = 0; char *s = line;
    while (*s && n < max) { while (*s == ' ') s++; if (!*s) break; t[n++] = s; while (*s && *s != ' ') s++; if (*s) *s++ = 0; }
    return n;
}

int main(void) {
    DDRD &= ~0x3c; PORTD &= ~0x3c; DDRB &= ~3; PORTB &= ~3; DDRC &= ~0x20; PORTC &= ~0x20;   /* all lines high-Z */
    uart_init(); sei();
    puts_("bench ready\r\n");
    for (;;) {
        int n = 0, c;
        while ((c = rx()) != '\n') { if (c != '\r' && n < 63) line[n++] = c; }
        line[n] = 0;
        char *t[5]; int k = toks(t, 5);
        if (!k) continue;
        int p = k > 1 ? atoi(t[1]) : 0;
        if (t[0][0] == 'f' && k == 2) {
            long hz = atol(t[1]);
            if (hz <= 0) { TCCR1A = 0; TCCR1B = 0; PORTB &= ~_BV(1); DDRB |= _BV(1); puts_("f 0\r\n"); continue; }
            uint32_t top = F_CPU / 2 / hz; uint8_t cs = 1; uint32_t pre = 1;   /* prescaler 1 or 8 or 64 */
            if (top > 65536) { cs = 2; pre = 8; top = F_CPU / 2 / 8 / hz; }
            if (top > 65536) { cs = 3; pre = 64; top = F_CPU / 2 / 64 / hz; }
            if (top < 1) top = 1;
            if (top > 65536) top = 65536;
            TCCR1B = 0; TCNT1 = 0; OCR1A = top - 1; TCCR1A = _BV(COM1A0); DDRB |= _BV(1);
            TCCR1B = _BV(WGM12) | cs;
            puts_("f "); putl(F_CPU / 2 / pre / top); puts_("\r\n"); continue;
        }
        if (t[0][0] == 'b' && k == 2) {
            uint32_t cnt = atol(t[1]);
            if (!(DDRB & _BV(1)) || TCCR1A) { puts_("err D9 not a plain output\r\n"); continue; }
            PORTB &= ~_BV(1);
            cli();
            while (cnt--) {                   /* 16 cycles per pulse at 16 MHz = 1 MHz */
                PINB = _BV(1); __builtin_avr_delay_cycles(6);
                PINB = _BV(1); __builtin_avr_delay_cycles(2);
            }
            sei(); puts_("ok\r\n"); continue;
        }
        if (t[0][0] == '?') { puts_("bench 3 D2 D3 D4 D5 D8 D9 A5\r\n"); continue; }
        if (t[0][0] == 'r') {
            tx('r'); tx(' '); int ps[7] = {2, 3, 4, 5, 8, 9, 19};
            for (int i = 0; i < 7; i++) tx('0' + level(ps[i]));
            puts_("\r\n"); continue;
        }
        if (!pin_ok(p)) { puts_("err pin\r\n"); continue; }
        if (t[0][0] == 'm' && k == 3) {
            char m = t[2][0];
            if (m == 'i' || m == 'u') { *ddr(p) &= ~bit(p); if (m == 'u') *port(p) |= bit(p); else *port(p) &= ~bit(p); }
            else if (m == '0') { *port(p) &= ~bit(p); *ddr(p) |= bit(p); }
            else if (m == '1') { *port(p) |= bit(p); *ddr(p) |= bit(p); }
            else { puts_("err mode\r\n"); continue; }
            puts_("ok\r\n");
        } else if (t[0][0] == 'c' && k == 3) {
            long ms = atol(t[2]); if (ms < 1 || ms > 60000) { puts_("err ms\r\n"); continue; }
            cli(); edges = 0; want = bit(p);
            if (p == 8 || p == 9) { last = PINB & want; PCMSK0 = want; PCICR = _BV(PCIE0); }
            else if (p == 19) { last = PINC & want; PCMSK1 = want; PCICR = _BV(PCIE1); }
            else { last = PIND & want; PCMSK2 = want; PCICR = _BV(PCIE2); }
            sei();
            while (ms--) _delay_ms(1);
            cli(); PCICR = 0; PCMSK0 = PCMSK1 = PCMSK2 = 0; uint32_t e = edges; sei();
            puts_("c "); putl(e); puts_("\r\n");
        } else if (t[0][0] == 'p' && k == 4) {
            long cnt = atol(t[2]); int us = atoi(t[3]);
            if (!(*ddr(p) & bit(p))) { puts_("err not output\r\n"); continue; }
            while (cnt-- > 0) {
                *port(p) |= bit(p); for (int i = 0; i < us; i++) _delay_us(1);
                *port(p) &= ~bit(p); for (int i = 0; i < us; i++) _delay_us(1);
            }
            puts_("ok\r\n");
        } else if (t[0][0] == 's' && k == 3) {
            int cnt = atoi(t[2]); if (cnt < 1 || cnt > 200) { puts_("err n\r\n"); continue; }
            uint8_t buf[200];
            for (int i = 0; i < cnt; i++) buf[i] = ((PIND >> 2) & 0x0f) | ((PINB & 3) << 4) | (((PINC >> 5) & 1) << 6);
            puts_("s");
            for (int i = 0; i < cnt; i++) { const char *h = "0123456789abcdef"; tx(' '); tx(h[buf[i] >> 4]); tx(h[buf[i] & 15]); }
            puts_("\r\n");
        } else puts_("err cmd\r\n");
    }
}
