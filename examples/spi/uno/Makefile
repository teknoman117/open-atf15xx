# Build and flash the SPI example's slave firmware (host avr-gcc + avrdude; Arduino bootloader).
# To return the Uno to the bench instrument: make -C tools/silicon/bench flash
PORT ?= /dev/ttyACM0
spi_slave.hex: spi_slave.c
	avr-gcc -mmcu=atmega328p -DF_CPU=16000000UL -Os -Wall -o spi_slave.elf spi_slave.c
	avr-objcopy -O ihex -R .eeprom spi_slave.elf spi_slave.hex
flash: spi_slave.hex
	avrdude -q -p m328p -c arduino -P $(PORT) -b 115200 -U flash:w:spi_slave.hex:i
clean:
	rm -f spi_slave.elf spi_slave.hex
.PHONY: flash clean
