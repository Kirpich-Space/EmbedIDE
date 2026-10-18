CC ?= cc
CFLAGS ?= -Wall -Wextra -g

all: blink-host

blink-host: main.c
	$(CC) $(CFLAGS) main.c -o blink-host

clean:
	$(RM) blink-host

.PHONY: all clean
