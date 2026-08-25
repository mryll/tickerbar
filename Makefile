PREFIX ?= /usr/local
BIN := $(PREFIX)/bin
OMARCHY_PLUGIN_LINK := $(HOME)/.config/omarchy/plugins/mryll.tickerbar

build:
	cargo build --release

install: build
	install -Dm755 target/release/tickerbar "$(BIN)/tickerbar"
	install -Dm644 config.example.toml "$(PREFIX)/share/tickerbar/config.example.toml"

uninstall:
	rm -f "$(BIN)/tickerbar"

# Omarchy shell (Quickshell) plugin: symlink the in-repo omarchy/ dir into the
# shell's plugin directory. Needs the tickerbar binary on PATH (make install,
# or the AUR package).
install-omarchy:
	@command -v tickerbar >/dev/null 2>&1 || \
		echo "note: tickerbar not found on PATH — the widget will show an explicit error until the binary is installed (make install, or the AUR package)"
	mkdir -p "$(HOME)/.config/omarchy/plugins"
	ln -sfT "$(abspath .)" "$(OMARCHY_PLUGIN_LINK)"
	@echo 'Plugin linked. Add { "id": "mryll.tickerbar" } to a bar section in ~/.config/omarchy/shell.json'

uninstall-omarchy:
	rm -f "$(OMARCHY_PLUGIN_LINK)"

.PHONY: build install uninstall install-omarchy uninstall-omarchy
