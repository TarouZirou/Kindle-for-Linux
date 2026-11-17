PREFIX  ?= /usr
DESTDIR ?=
PKGDATADIR = $(PREFIX)/share/kindle-for-linux
APP_ID = io.github.tarouzirou.KindleForLinux

.PHONY: all install uninstall run

all:
	@echo "Nothing to build. Use 'make install' or 'make run'."

run:
	./bin/kindle-for-linux

install:
	install -d $(DESTDIR)$(PKGDATADIR)/kindle_for_linux
	install -m644 kindle_for_linux/*.py $(DESTDIR)$(PKGDATADIR)/kindle_for_linux/
	install -Dm755 bin/kindle-for-linux $(DESTDIR)$(PREFIX)/bin/kindle-for-linux
	sed -i 's|@PKGDATADIR@|$(PKGDATADIR)|' $(DESTDIR)$(PREFIX)/bin/kindle-for-linux
	install -Dm644 data/$(APP_ID).desktop $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop
	install -Dm644 data/$(APP_ID).svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/kindle-for-linux/LICENSE

uninstall:
	rm -rf $(DESTDIR)$(PKGDATADIR)
	rm -f $(DESTDIR)$(PREFIX)/bin/kindle-for-linux
	rm -f $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop
	rm -f $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	rm -rf $(DESTDIR)$(PREFIX)/share/licenses/kindle-for-linux
