PREFIX ?= /usr/local
BIN := $(PREFIX)/bin

build:
	cargo build --release

install: build
	install -Dm755 target/release/tickerbar $(BIN)/tickerbar

uninstall:
	rm -f $(BIN)/tickerbar

.PHONY: build install uninstall
