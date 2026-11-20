PREFIX  ?= /usr
DESTDIR ?=
APP_ID   = io.github.tarouzirou.KindleForLinux

.PHONY: all build test verify install uninstall

all: build

build:
	cargo build --release --locked

test:
	cargo test --release --locked

# Lean proofs + vectors + Rust differential tests (needs `lake` from elan).
verify:
	./scripts/verify.sh

install:
	install -Dm755 target/release/kindle-for-linux $(DESTDIR)$(PREFIX)/bin/kindle-for-linux
	install -Dm644 data/$(APP_ID).desktop $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop
	install -Dm644 data/$(APP_ID).svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/kindle-for-linux/LICENSE

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/kindle-for-linux
	rm -f $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop
	rm -f $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	rm -rf $(DESTDIR)$(PREFIX)/share/licenses/kindle-for-linux
