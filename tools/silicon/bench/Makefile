# Build and flash the Uno test instrument (host avr-gcc + avrdude; Arduino bootloader).
PORT ?= /dev/ttyACM0
bench.hex: bench.c
	avr-gcc -mmcu=atmega328p -DF_CPU=16000000UL -Os -Wall -o bench.elf bench.c
	avr-objcopy -O ihex -R .eeprom bench.elf bench.hex
flash: bench.hex
	avrdude -q -p m328p -c arduino -P $(PORT) -b 115200 -U flash:w:bench.hex:i
clean:
	rm -f bench.elf bench.hex
.PHONY: flash clean
