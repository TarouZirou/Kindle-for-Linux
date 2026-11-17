"""Main window: a WebKitGTK view dedicated to Kindle Cloud Reader."""

from __future__ import annotations

import re
from urllib.parse import urlsplit

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
gi.require_version("WebKit", "6.0")

from gi.repository import Adw, Gdk, Gio, GLib, Gtk, WebKit  # noqa: E402

from .config import CACHE_DIR, DATA_DIR, REGIONS, Config  # noqa: E402

ZOOM_STEP = 0.1

# Hosts that are allowed to load inside the app. Everything else opens in the
# user's default browser.
_AMAZON_HOST = re.compile(
    r"(^|\.)amazon\.(com|co\.jp|co\.uk|de|fr|es|it|nl|se|pl|ca|com\.au|in|com\.br"
    r"|com\.mx|com\.tr|com\.be|sg|ae|sa|eg)$"
)
_AMAZON_SUFFIXES = (
    "media-amazon.com",
    "ssl-images-amazon.com",
    "images-amazon.com",
    "amazon-adsystem.com",
    "amazoncognito.com",
    "amazontrust.com",
)


def is_amazon_host(host: str | None) -> bool:
    if not host:
        return False
    host = host.lower().rstrip(".")
    if _AMAZON_HOST.search(host):
        return True
    return any(host == s or host.endswith("." + s) for s in _AMAZON_SUFFIXES)


def is_internal_uri(uri: str | None) -> bool:
    if not uri:
        return False
    parts = urlsplit(uri)
    if parts.scheme in ("about", "blob", "data"):
        return True
    return parts.scheme in ("http", "https") and is_amazon_host(parts.hostname)


def create_network_session() -> WebKit.NetworkSession:
    """A persistent session so the Amazon login survives restarts."""
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    session = WebKit.NetworkSession.new(str(DATA_DIR / "webkit"), str(CACHE_DIR / "webkit"))
    cookies = session.get_cookie_manager()
    cookies.set_persistent_storage(
        str(DATA_DIR / "cookies.sqlite"), WebKit.CookiePersistentStorage.SQLITE
    )
    cookies.set_accept_policy(WebKit.CookieAcceptPolicy.NO_THIRD_PARTY)
    return session


class KindleWindow(Adw.ApplicationWindow):
    def __init__(self, app: Adw.Application, config: Config, session: WebKit.NetworkSession,
                 start_url: str | None = None):
        super().__init__(application=app, title="Kindle for Linux")
        self.config = config
        self.session = session
        self.set_default_size(config.width, config.height)
        if config.maximized:
            self.maximize()

        # --- Web view -------------------------------------------------------
        self.view = WebKit.WebView(network_session=session)
        self._configure_webview(self.view)
        self.view.set_zoom_level(config.zoom)
        self.view.connect("decide-policy", self._on_decide_policy)
        self.view.connect("create", self._on_create)
        self.view.connect("load-changed", self._on_load_changed)
        self.view.connect("load-failed", self._on_load_failed)
        self.view.connect("notify::title", self._on_title_changed)
        self.view.connect("notify::estimated-load-progress", self._on_progress)
        self.view.connect("permission-request", self._on_permission_request)
        self.view.connect("web-process-terminated", self._on_web_process_terminated)
        self.view.connect("context-menu", self._on_context_menu)

        # --- Header bar -----------------------------------------------------
        header = Adw.HeaderBar()
        self.back_button = Gtk.Button(icon_name="go-previous-symbolic",
                                      tooltip_text="戻る (Alt+←)", action_name="win.back")
        home_button = Gtk.Button(icon_name="view-grid-symbolic",
                                 tooltip_text="ライブラリ (Ctrl+L)", action_name="win.home")
        reload_button = Gtk.Button(icon_name="view-refresh-symbolic",
                                   tooltip_text="再読み込み (Ctrl+R)", action_name="win.reload")
        header.pack_start(self.back_button)
        header.pack_start(home_button)
        header.pack_start(reload_button)

        self.window_title = Adw.WindowTitle(title="Kindle for Linux", subtitle="")
        header.set_title_widget(self.window_title)

        menu_button = Gtk.MenuButton(icon_name="open-menu-symbolic",
                                     menu_model=self._build_menu(), tooltip_text="メニュー")
        header.pack_end(menu_button)
        fullscreen_button = Gtk.Button(icon_name="view-fullscreen-symbolic",
                                       tooltip_text="全画面 (F11)", action_name="win.fullscreen")
        header.pack_end(fullscreen_button)

        # --- Layout ---------------------------------------------------------
        self.progress = Gtk.ProgressBar(valign=Gtk.Align.START, can_target=False)
        self.progress.add_css_class("osd")
        self.progress.set_visible(False)

        overlay = Gtk.Overlay(child=self.view)
        overlay.add_overlay(self.progress)

        self.toasts = Adw.ToastOverlay(child=overlay)
        self.toolbar_view = Adw.ToolbarView(content=self.toasts)
        self.toolbar_view.add_top_bar(header)
        self.set_content(self.toolbar_view)

        self._install_actions()
        self.connect("notify::fullscreened", self._on_fullscreen_changed)
        # Esc leaves fullscreen, but is passed through to the page otherwise.
        keys = Gtk.EventControllerKey(propagation_phase=Gtk.PropagationPhase.CAPTURE)
        keys.connect("key-pressed", self._on_key_pressed)
        self.add_controller(keys)
        self.connect("close-request", self._on_close_request)

        self.view.load_uri(start_url or self._initial_url())

    # ------------------------------------------------------------------ setup
    def _configure_webview(self, view: WebKit.WebView) -> None:
        settings = view.get_settings()
        settings.set_enable_developer_extras(True)
        settings.set_enable_back_forward_navigation_gestures(True)
        settings.set_javascript_can_open_windows_automatically(False)
        settings.set_enable_smooth_scrolling(True)
        settings.set_default_charset("utf-8")
        if self.config.user_agent:
            settings.set_user_agent(self.config.user_agent)

    def _initial_url(self) -> str:
        last = self.config.last_url
        home_host = urlsplit(self.config.home_url).hostname
        if self.config.restore_last_page and last and urlsplit(last).hostname == home_host:
            return last
        return self.config.home_url

    def _build_menu(self) -> Gio.Menu:
        menu = Gio.Menu()

        view_section = Gio.Menu()
        view_section.append("拡大 (Ctrl++)", "win.zoom-in")
        view_section.append("縮小 (Ctrl+-)", "win.zoom-out")
        view_section.append("等倍 (Ctrl+0)", "win.zoom-reset")
        menu.append_section(None, view_section)

        regions = Gio.Menu()
        for key, (label, _url) in REGIONS.items():
            item = Gio.MenuItem.new(label, None)
            item.set_action_and_target_value("win.region", GLib.Variant.new_string(key))
            regions.append_item(item)
        region_section = Gio.Menu()
        region_section.append_submenu("ストア (地域)", regions)
        restore = Gio.MenuItem.new("前回のページを復元", "win.restore-last-page")
        region_section.append_item(restore)
        menu.append_section(None, region_section)

        misc = Gio.Menu()
        misc.append("外部ブラウザで開く", "win.open-external")
        misc.append("開発者ツール (Ctrl+Shift+I)", "win.inspector")
        misc.append("ログアウト / データ消去…", "win.clear-data")
        menu.append_section(None, misc)

        about = Gio.Menu()
        about.append("キーボードショートカット", "win.shortcuts")
        about.append("Kindle for Linux について", "app.about")
        about.append("終了 (Ctrl+Q)", "app.quit")
        menu.append_section(None, about)
        return menu

    def _install_actions(self) -> None:
        app = self.get_application()

        def add(name, callback, accels=()):
            action = Gio.SimpleAction.new(name, None)
            action.connect("activate", lambda *_: callback())
            self.add_action(action)
            if accels:
                app.set_accels_for_action(f"win.{name}", list(accels))
            return action

        self.back_action = add("back", self.view.go_back, ["<Alt>Left", "<Alt>KP_Left"])
        self.forward_action = add("forward", self.view.go_forward, ["<Alt>Right", "<Alt>KP_Right"])
        add("home", lambda: self.view.load_uri(self.config.home_url), ["<Control>l"])
        add("reload", self.view.reload, ["<Control>r", "F5"])
        add("hard-reload", self.view.reload_bypass_cache, ["<Control><Shift>r", "<Shift>F5"])
        add("zoom-in", lambda: self._zoom(+ZOOM_STEP),
            ["<Control>plus", "<Control>equal", "<Control>KP_Add"])
        add("zoom-out", lambda: self._zoom(-ZOOM_STEP), ["<Control>minus", "<Control>KP_Subtract"])
        add("zoom-reset", lambda: self._zoom(None), ["<Control>0", "<Control>KP_0"])
        add("fullscreen", self._toggle_fullscreen, ["F11"])
        add("open-external", self._open_current_externally)
        add("inspector", lambda: self.view.get_inspector().show(), ["<Control><Shift>i"])
        add("clear-data", self._confirm_clear_data)
        add("shortcuts", self._show_shortcuts, ["<Control>question"])

        region = Gio.SimpleAction.new_stateful(
            "region", GLib.VariantType.new("s"), GLib.Variant.new_string(self.config.region))
        region.connect("activate", self._on_region_selected)
        self.add_action(region)

        restore = Gio.SimpleAction.new_stateful(
            "restore-last-page", None, GLib.Variant.new_boolean(self.config.restore_last_page))
        restore.connect("activate", self._on_restore_toggled)
        self.add_action(restore)

        self.back_action.set_enabled(False)
        self.forward_action.set_enabled(False)

    # ---------------------------------------------------------------- actions
    def _zoom(self, delta: float | None) -> None:
        level = 1.0 if delta is None else self.view.get_zoom_level() + delta
        level = round(min(max(level, 0.3), 3.0), 2)
        self.view.set_zoom_level(level)
        self.config.zoom = level
        self.toast(f"ズーム {int(level * 100)}%", timeout=1)

    def _toggle_fullscreen(self) -> None:
        if self.is_fullscreen():
            self.unfullscreen()
        else:
            self.fullscreen()

    def _on_key_pressed(self, _ctrl, keyval, _keycode, _state) -> bool:
        if keyval == Gdk.KEY_Escape and self.is_fullscreen():
            self.unfullscreen()
            return True
        return False

    def _on_fullscreen_changed(self, *_):
        fullscreen = self.is_fullscreen()
        self.toolbar_view.set_reveal_top_bars(not fullscreen)
        if fullscreen:
            self.toast("Esc または F11 で全画面を終了", timeout=2)

    def _on_region_selected(self, action: Gio.SimpleAction, value: GLib.Variant) -> None:
        action.set_state(value)
        self.config.region = value.get_string()
        self.config.last_url = ""
        self.config.save()
        self.view.load_uri(self.config.home_url)

    def _on_restore_toggled(self, action: Gio.SimpleAction, _value) -> None:
        new = not action.get_state().get_boolean()
        action.set_state(GLib.Variant.new_boolean(new))
        self.config.restore_last_page = new
        self.config.save()

    def _open_current_externally(self) -> None:
        uri = self.view.get_uri()
        if uri:
            self.open_external(uri)

    def open_external(self, uri: str) -> None:
        Gtk.UriLauncher.new(uri).launch(self, None, None, None)

    def _confirm_clear_data(self) -> None:
        dialog = Adw.AlertDialog(
            heading="ログアウトしてデータを消去しますか？",
            body="Cookie・キャッシュ・ローカルストレージをすべて削除します。"
                 "次回は Amazon アカウントへの再ログインが必要です。",
        )
        dialog.add_response("cancel", "キャンセル")
        dialog.add_response("clear", "消去")
        dialog.set_response_appearance("clear", Adw.ResponseAppearance.DESTRUCTIVE)
        dialog.set_default_response("cancel")
        dialog.set_close_response("cancel")
        dialog.connect("response", self._on_clear_response)
        dialog.present(self)

    def _on_clear_response(self, _dialog, response: str) -> None:
        if response != "clear":
            return
        manager = self.session.get_website_data_manager()

        def done(mgr, result):
            try:
                mgr.clear_finish(result)
            except GLib.Error as err:
                self.toast(f"消去に失敗しました: {err.message}")
                return
            self.config.last_url = ""
            self.config.save()
            self.toast("データを消去しました")
            self.view.load_uri(self.config.home_url)

        manager.clear(WebKit.WebsiteDataTypes.ALL, 0, None, done)

    def _show_shortcuts(self) -> None:
        rows = [
            ("F11", "全画面の切り替え"),
            ("Esc", "全画面を終了"),
            ("Ctrl + L", "ライブラリへ戻る"),
            ("Alt + ← / →", "戻る / 進む"),
            ("Ctrl + R / F5", "再読み込み"),
            ("Ctrl + + / - / 0", "拡大 / 縮小 / 等倍"),
            ("Ctrl + Shift + I", "開発者ツール"),
            ("Ctrl + Q", "終了"),
            ("← / →", "ページ送り (Kindle Cloud Reader 側の機能)"),
        ]
        group = Adw.PreferencesGroup()
        for keys, desc in rows:
            row = Adw.ActionRow(title=desc)
            label = Gtk.Label(label=keys)
            label.add_css_class("dim-label")
            row.add_suffix(label)
            group.add(row)
        page = Adw.PreferencesPage()
        page.add(group)
        dialog = Adw.Dialog(title="キーボードショートカット", content_width=420, content_height=520)
        tv = Adw.ToolbarView(content=page)
        tv.add_top_bar(Adw.HeaderBar())
        dialog.set_child(tv)
        dialog.present(self)

    def toast(self, text: str, timeout: int = 3) -> None:
        toast = Adw.Toast(title=text, timeout=timeout)
        self.toasts.add_toast(toast)

    # --------------------------------------------------------- webview events
    def _on_decide_policy(self, _view, decision, decision_type) -> bool:
        if decision_type == WebKit.PolicyDecisionType.NAVIGATION_ACTION:
            action = decision.get_navigation_action()
            uri = action.get_request().get_uri()
            if (action.get_navigation_type() == WebKit.NavigationType.LINK_CLICKED
                    and not is_internal_uri(uri)):
                decision.ignore()
                self.open_external(uri)
                return True
        elif decision_type == WebKit.PolicyDecisionType.RESPONSE:
            if not decision.is_mime_type_supported() and decision.is_main_frame_main_resource():
                decision.download()
                return True
        return False

    def _on_create(self, view, action):
        """window.open() / target=_blank."""
        uri = action.get_request().get_uri()
        if uri and not is_internal_uri(uri):
            self.open_external(uri)
            return None
        # Amazon pages (e.g. sign-in or help popups) open in an in-app popup
        # that shares the login session.
        popup_view = WebKit.WebView(related_view=view)
        self._configure_webview(popup_view)
        popup_view.connect("decide-policy", self._on_decide_policy)
        popup = Adw.Window(transient_for=self, default_width=900, default_height=700,
                           title="Kindle for Linux")
        tv = Adw.ToolbarView(content=popup_view)
        tv.add_top_bar(Adw.HeaderBar())
        popup.set_content(tv)
        popup_view.connect("ready-to-show", lambda *_: popup.present())
        popup_view.connect("close", lambda *_: popup.destroy())
        popup_view.connect("notify::title",
                           lambda v, _p: popup.set_title(v.get_title() or "Kindle for Linux"))
        return popup_view

    def _on_load_changed(self, view, event) -> None:
        self.back_action.set_enabled(view.can_go_back())
        self.forward_action.set_enabled(view.can_go_forward())
        uri = view.get_uri() or ""
        host = urlsplit(uri).hostname or ""
        self.window_title.set_subtitle(host)
        if event == WebKit.LoadEvent.FINISHED and host.startswith("read.amazon."):
            self.config.last_url = uri

    def _on_load_failed(self, _view, _event, uri, error) -> bool:
        if error.matches(WebKit.NetworkError.quark(), WebKit.NetworkError.CANCELLED):
            return False
        if error.matches(WebKit.PolicyError.quark(),
                         WebKit.PolicyError.FRAME_LOAD_INTERRUPTED_BY_POLICY_CHANGE):
            return False
        self.toast(f"読み込みに失敗しました: {error.message}")
        return False

    def _on_title_changed(self, view, _pspec) -> None:
        title = view.get_title() or "Kindle for Linux"
        self.window_title.set_title(title)
        self.set_title(title)

    def _on_progress(self, view, _pspec) -> None:
        progress = view.get_estimated_load_progress()
        self.progress.set_fraction(progress)
        self.progress.set_visible(0.0 < progress < 1.0)

    def _on_permission_request(self, _view, request) -> bool:
        # Kindle Cloud Reader does not need camera/mic/location/etc.
        request.deny()
        return True

    def _on_web_process_terminated(self, view, reason) -> None:
        self.toast("ページがクラッシュしたため再読み込みします")
        GLib.timeout_add_seconds(1, lambda: (view.reload(), False)[1])

    def _on_context_menu(self, _view, menu, _hit) -> bool:
        # Hide navigation items that would load pages in a broken state;
        # keep copy, inspect element, etc.
        hidden = {
            WebKit.ContextMenuAction.OPEN_LINK_IN_NEW_WINDOW,
            WebKit.ContextMenuAction.OPEN_IMAGE_IN_NEW_WINDOW,
            WebKit.ContextMenuAction.OPEN_FRAME_IN_NEW_WINDOW,
            WebKit.ContextMenuAction.OPEN_VIDEO_IN_NEW_WINDOW,
            WebKit.ContextMenuAction.OPEN_AUDIO_IN_NEW_WINDOW,
        }
        for item in list(menu.get_items()):
            if item.get_stock_action() in hidden:
                menu.remove(item)
        return False

    # ----------------------------------------------------------------- close
    def _on_close_request(self, *_):
        self.save_state()
        return False

    def save_state(self) -> None:
        self.config.maximized = self.is_maximized()
        if not self.config.maximized and not self.is_fullscreen():
            width, height = self.get_default_size()
            if width > 0 and height > 0:
                self.config.width, self.config.height = width, height
        self.config.save()
