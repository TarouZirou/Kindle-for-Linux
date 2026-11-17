"""Application entry point."""

from __future__ import annotations

import argparse
import shutil
import signal
import sys

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
gi.require_version("WebKit", "6.0")

from gi.repository import Adw, Gio, GLib, Gtk  # noqa: E402

from . import __version__  # noqa: E402
from .config import CACHE_DIR, DATA_DIR, REGIONS, Config  # noqa: E402
from .window import KindleWindow, create_network_session  # noqa: E402

APP_ID = "io.github.tarouzirou.KindleForLinux"


class KindleApplication(Adw.Application):
    def __init__(self, config: Config, start_url: str | None = None):
        super().__init__(application_id=APP_ID, flags=Gio.ApplicationFlags.DEFAULT_FLAGS)
        self.config = config
        self.start_url = start_url
        self.session = None
        self.window = None

        quit_action = Gio.SimpleAction.new("quit", None)
        quit_action.connect("activate", lambda *_: self._quit())
        self.add_action(quit_action)
        self.set_accels_for_action("app.quit", ["<Control>q"])

        about = Gio.SimpleAction.new("about", None)
        about.connect("activate", lambda *_: self._show_about())
        self.add_action(about)

    def do_startup(self):
        Adw.Application.do_startup(self)
        # Save window state / last page when killed by the session manager.
        for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, sig, self._on_signal)

    def _on_signal(self):
        self._quit()
        return GLib.SOURCE_REMOVE

    def do_shutdown(self):
        if self.window is not None:
            self.window.save_state()
        Adw.Application.do_shutdown(self)

    def do_activate(self):
        if self.window is None:
            self.session = create_network_session()
            self.window = KindleWindow(self, self.config, self.session, self.start_url)
        self.window.present()

    def _quit(self):
        if self.window is not None:
            self.window.close()
        self.quit()

    def _show_about(self):
        dialog = Adw.AboutDialog(
            application_name="Kindle for Linux",
            application_icon=APP_ID,
            version=__version__,
            developer_name="TarouZirou",
            license_type=Gtk.License.MIT_X11,
            website="https://github.com/TarouZirou/Kindle-for-Linux",
            issue_url="https://github.com/TarouZirou/Kindle-for-Linux/issues",
            comments="Kindle Cloud Reader 専用の軽量デスクトップクライアント。\n"
                     "Amazon および Kindle は Amazon.com, Inc. の商標です。"
                     "本ソフトウェアは Amazon とは無関係の非公式クライアントです。",
        )
        dialog.present(self.window)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="kindle-for-linux",
        description="Kindle Cloud Reader desktop client for Linux",
    )
    parser.add_argument("--region", choices=sorted(REGIONS),
                        help="Amazon store to use (saved for next launch)")
    parser.add_argument("--url", help="open this URL instead of the library")
    parser.add_argument("--user-agent", help="override the browser user agent (saved)")
    parser.add_argument("--reset", action="store_true",
                        help="delete all saved login data, cache and settings, then exit")
    parser.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)

    if args.reset:
        from .config import CONFIG_DIR

        for path in (DATA_DIR, CACHE_DIR, CONFIG_DIR):
            shutil.rmtree(path, ignore_errors=True)
        print("Kindle for Linux: all data removed.")
        return 0

    config = Config.load()
    changed = False
    if args.region:
        config.region, config.last_url, changed = args.region, "", True
    if args.user_agent is not None:
        config.user_agent, changed = args.user_agent, True
    if changed:
        config.save()

    app = KindleApplication(config, start_url=args.url)
    return app.run([sys.argv[0]])
