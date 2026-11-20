//! The application: window, event handling and the two screens (library and
//! reader). The GUI is drawn entirely in software into a shared-memory
//! framebuffer (softbuffer) and only redrawn in response to events, so an
//! idle window uses no CPU and no GPU.

use crate::canvas::{rgb, wrap, Canvas, Pixmap, Rect};
use crate::core::layout::{self, Mode};
use crate::core::{progress, scroll, zoom};
use crate::doc::{LayoutParams, Meta};
use crate::font::GlyphCache;
use crate::library::{self, Item};
use crate::store::{now_secs, Config, Entry, History, Theme, REGIONS};
use crate::worker::{self, target_size, Cmd, Key, Reply};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

pub enum UserEvent {
    Worker(Reply),
    Scanned { kindle: Vec<Item>, local: Vec<Item> },
    Thumb { path: PathBuf, pix: Option<Pixmap> },
    Chosen { path: Option<PathBuf>, folder: bool },
}

// ------------------------------------------------------------------ theme

struct Colors {
    bar: u32,
    bar_text: u32,
    sub: u32,
    bg: u32,
    hover: u32,
    active: u32,
    accent: u32,
    border: u32,
    page_bg: u32,
    page_fg: u32,
    reader_bg: u32,
}

fn colors(t: Theme) -> Colors {
    match t {
        Theme::Light => Colors {
            bar: rgb(246, 245, 243),
            bar_text: rgb(32, 32, 34),
            sub: rgb(110, 110, 116),
            bg: rgb(252, 252, 251),
            hover: rgb(228, 226, 222),
            active: rgb(214, 226, 245),
            accent: rgb(227, 140, 32),
            border: rgb(218, 216, 212),
            page_bg: rgb(253, 252, 249),
            page_fg: rgb(28, 28, 30),
            reader_bg: rgb(222, 220, 216),
        },
        Theme::Sepia => Colors {
            bar: rgb(240, 231, 212),
            bar_text: rgb(70, 52, 36),
            sub: rgb(128, 106, 84),
            bg: rgb(248, 241, 227),
            hover: rgb(226, 212, 186),
            active: rgb(220, 200, 160),
            accent: rgb(190, 110, 30),
            border: rgb(214, 198, 170),
            page_bg: rgb(244, 236, 216),
            page_fg: rgb(78, 58, 40),
            reader_bg: rgb(214, 200, 172),
        },
        Theme::Dark => Colors {
            bar: rgb(36, 36, 40),
            bar_text: rgb(230, 230, 232),
            sub: rgb(150, 150, 158),
            bg: rgb(28, 28, 31),
            hover: rgb(60, 60, 66),
            active: rgb(52, 70, 100),
            accent: rgb(240, 160, 60),
            border: rgb(64, 64, 70),
            page_bg: rgb(30, 30, 32),
            page_fg: rgb(212, 210, 205),
            reader_bg: rgb(18, 18, 20),
        },
    }
}

// ---------------------------------------------------------------- actions

#[derive(Clone, Debug, PartialEq)]
enum Act {
    OpenRecent(usize),
    OpenKindle(usize),
    OpenLocal(usize),
    OpenFile,
    AddFolder,
    Rescan,
    OpenCloud,
    MenuRegion,
    SetRegion(usize),
    Theme,
    Help,
    Back,
    MenuMode,
    SetMode(Mode),
    ToggleCover,
    ToggleVertical,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleFit,
    ToggleProps,
    Fullscreen,
    Slider,
    CloudCurrent,
    CloseOverlay,
}

#[derive(Clone, Copy, PartialEq)]
enum Menu {
    Mode(Rect),
    Region(Rect),
}

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Library,
    Reader,
}

// ----------------------------------------------------------------- reader

struct Cached {
    key: Key,
    pix: Pixmap,
    used: u64,
}

struct Reader {
    path: PathBuf,
    gen: u64,
    meta: Option<Meta>,
    error: Option<String>,
    n: u64,
    /// Anchor page (paged modes) or top page (vertical scroll).
    page: u64,
    /// Pixel offset inside `page` (vertical scroll).
    voff: u64,
    mode: Mode,
    cover: bool,
    zoom: u64,
    fit_width: bool,
    vertical_text: bool,
    pan: (u64, u64),
    naturals: Vec<Option<(u32, u32)>>,
    cache: Vec<Cached>,
    pending: HashSet<Key>,
    tick: u64,
    layout_gen: u32,
    params: Option<LayoutParams>,
    relayout_pending: bool,
    starts: Vec<u64>,
    char_off: u64,
    est: u64,
    last_turn: Instant,
    reached: u64,
    turns: u32,
    wheel_acc: f64,
    last_wheel: Instant,
    restored: bool,
}

impl Reader {
    fn reflowable(&self) -> bool {
        self.meta.as_ref().is_some_and(|m| m.reflowable)
    }
    /// Pages advance right-to-left (manga, vertical Japanese text).
    fn rtl(&self) -> bool {
        self.mode == Mode::SpreadRtl || (self.mode == Mode::Single && self.meta.as_ref().is_some_and(|m| m.rtl))
    }
}

/// One page placed on screen.
struct Placed {
    page: u64,
    dest: Rect,
    key: Key,
}

// ------------------------------------------------------------------ state

struct State {
    proxy: EventLoopProxy<UserEvent>,
    cfg: Config,
    history: History,
    screen: Screen,
    reader: Option<Reader>,
    worker: Sender<Cmd>,
    thumbs_tx: Sender<(PathBuf, u32, u32)>,
    gc: GlyphCache,
    scale: f32,
    size: (u32, u32),
    mouse: (f64, f64),
    press: Option<(f64, f64, (u64, u64), u64)>,
    dragged: bool,
    hits: Vec<(Rect, Act)>,
    hover: Option<Act>,
    menu: Option<Menu>,
    toast: Option<(String, Instant)>,
    show_props: bool,
    show_help: bool,
    fullscreen: bool,
    chrome_peek: bool,
    kindle: Vec<Item>,
    local: Vec<Item>,
    scanning: bool,
    lib_scroll: u64,
    lib_content_h: u64,
    thumbs: HashMap<PathBuf, Option<Pixmap>>,
    thumb_req: HashSet<PathBuf>,
    next_gen: u64,
    mods: ModifiersState,
    quit: bool,
    last_save: Instant,
}

pub struct App {
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    state: State,
    open_on_start: Option<PathBuf>,
}

const TOOLBAR: f32 = 46.0;
const STATUS: f32 = 38.0;
const PROPS_W: f32 = 330.0;
const UI_PX: f32 = 14.0;
const CARD_W: f32 = 148.0;
const COVER_H: f32 = 210.0;
const CARD_H: f32 = 270.0;
const GAP: f32 = 22.0;

impl State {
    fn s(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }
    fn px(&self) -> f32 {
        UI_PX * self.scale
    }
    fn c(&self) -> Colors {
        colors(self.cfg.theme)
    }
    fn text(&mut self, cv: &mut Canvas, s: &str, x: i32, y: i32, px: f32, color: u32) -> i32 {
        cv.text(&mut self.gc, s, x, y, px, color)
    }
    fn text_fit(&mut self, cv: &mut Canvas, s: &str, x: i32, y: i32, px: f32, max_w: i32, color: u32) -> i32 {
        cv.text_fit(&mut self.gc, s, x, y, px, max_w, color)
    }
    fn wrap(&mut self, s: &str, px: f32, max_w: f32) -> Vec<String> {
        wrap(&mut self.gc, s, px, max_w)
    }
    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    // ------------------------------------------------------------ actions

    fn open_cloud(&mut self, asin: Option<&str>) {
        let url = match asin {
            Some(a) => format!("{}?asin={}", self.cfg.cloud_url(), a),
            None => self.cfg.cloud_url().to_string(),
        };
        match std::process::Command::new("xdg-open").arg(&url).spawn() {
            Ok(_) => self.toast("Kindle Cloud Reader をブラウザで開きました"),
            Err(_) => self.toast(format!("ブラウザを開けませんでした: {url}")),
        }
    }

    fn open_item(&mut self, item: Item) {
        match (&item.path, item.cloud_only) {
            (Some(p), false) => self.open_path(p.clone()),
            _ => {
                if item.asin.is_some() {
                    self.open_cloud(item.asin.as_deref());
                } else {
                    self.toast("DRM で保護された本です。Kindle Cloud Reader で開いてください");
                }
            }
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        self.save_position();
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        self.next_gen += 1;
        let gen = self.next_gen;
        let h = self.history.get(&path).cloned();
        let est = h.as_ref().map(|e| progress::clamp_speed(e.ms_per_page)).unwrap_or(30_000);
        self.reader = Some(Reader {
            path: path.clone(),
            gen,
            meta: None,
            error: None,
            n: 0,
            page: h.as_ref().map(|e| e.page).unwrap_or(0),
            voff: 0,
            mode: h.as_ref().map(|e| Mode::from_id(e.mode)).unwrap_or(Mode::Single),
            cover: h.as_ref().map(|e| e.cover).unwrap_or(true),
            zoom: h.as_ref().map(|e| zoom::clamp_zoom(e.zoom)).unwrap_or(100),
            fit_width: h.as_ref().map(|e| e.fit_width).unwrap_or(false),
            vertical_text: h.as_ref().map(|e| e.vertical).unwrap_or(false),
            pan: (0, 0),
            naturals: vec![],
            cache: vec![],
            pending: HashSet::new(),
            tick: 0,
            layout_gen: 0,
            params: None,
            relayout_pending: false,
            starts: vec![],
            char_off: h.as_ref().map(|e| e.char_off).unwrap_or(0),
            est,
            last_turn: Instant::now(),
            reached: 0,
            turns: 0,
            wheel_acc: 0.0,
            last_wheel: Instant::now(),
            restored: h.is_some(),
        });
        self.screen = Screen::Reader;
        self.menu = None;
        let _ = self.worker.send(Cmd::Open(path, gen));
    }

    fn save_position(&mut self) {
        let Some(r) = &self.reader else { return };
        let Some(m) = &r.meta else { return };
        if !m.readable {
            return;
        }
        self.last_save = Instant::now();
        self.history.upsert(Entry {
            path: r.path.clone(),
            title: m.title.clone(),
            author: m.author.clone(),
            format: m.format.to_string(),
            page: r.page,
            pages: r.n,
            char_off: r.char_off,
            ms_per_page: r.est,
            last_opened: now_secs(),
            mode: r.mode.id(),
            cover: r.cover,
            zoom: r.zoom,
            vertical: r.vertical_text,
            fit_width: r.fit_width,
            reached: r.reached,
        });
        self.history.save();
    }

    fn back_to_library(&mut self) {
        self.save_position();
        self.reader = None;
        self.screen = Screen::Library;
        self.show_props = false;
        self.menu = None;
    }

    fn choose(&mut self, folder: bool) {
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let tries: Vec<(&str, Vec<&str>)> = if folder {
                vec![("zenity", vec!["--file-selection", "--directory", "--title=フォルダを追加"]), ("kdialog", vec!["--getexistingdirectory", "."])]
            } else {
                vec![
                    ("zenity", vec!["--file-selection", "--title=本を開く", "--file-filter=本 | *.epub *.pdf *.cbz *.zip *.txt *.mobi *.azw *.azw3 *.prc", "--file-filter=すべて | *"]),
                    ("kdialog", vec!["--getopenfilename", ".", "*.epub *.pdf *.cbz *.zip *.txt *.mobi *.azw *.azw3 *.prc"]),
                ]
            };
            for (cmd, args) in tries {
                if let Ok(o) = std::process::Command::new(cmd).args(&args).output() {
                    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    let path = (o.status.success() && !s.is_empty()).then(|| PathBuf::from(s));
                    let _ = proxy.send_event(UserEvent::Chosen { path, folder });
                    return;
                }
            }
            let _ = proxy.send_event(UserEvent::Chosen { path: Some(PathBuf::new()), folder });
        });
    }

    fn rescan(&mut self) {
        self.scanning = true;
        let proxy = self.proxy.clone();
        let cfg = Config { folders: self.cfg.folders.clone(), kindle_folders: self.cfg.kindle_folders.clone(), ..Config::default() };
        std::thread::spawn(move || {
            let (kindle, local) = library::scan(&cfg);
            let _ = proxy.send_event(UserEvent::Scanned { kindle, local });
        });
    }

    fn do_act(&mut self, a: Act) {
        let prev_menu = self.menu.take();
        match a {
            Act::OpenRecent(i) => {
                if let Some(e) = self.history.entries.get(i) {
                    let p = e.path.clone();
                    if p.exists() {
                        self.open_path(p);
                    } else {
                        self.toast("ファイルが見つかりません（移動または削除されました）");
                    }
                }
            }
            Act::OpenKindle(i) => {
                if let Some(it) = self.kindle.get(i).cloned() {
                    self.open_item(it);
                }
            }
            Act::OpenLocal(i) => {
                if let Some(it) = self.local.get(i).cloned() {
                    self.open_item(it);
                }
            }
            Act::OpenFile => self.choose(false),
            Act::AddFolder => self.choose(true),
            Act::Rescan => {
                self.rescan();
                self.toast("ライブラリを再スキャンしています…");
            }
            Act::OpenCloud => self.open_cloud(None),
            Act::CloudCurrent => {
                let asin = self.reader.as_ref().and_then(|r| r.meta.as_ref()).and_then(|m| m.asin.clone());
                self.open_cloud(asin.as_deref());
            }
            Act::MenuRegion => {
                if !matches!(prev_menu, Some(Menu::Region(_))) {
                    let r = self.hits.iter().find(|h| h.1 == Act::MenuRegion).map(|h| h.0).unwrap_or_default();
                    self.menu = Some(Menu::Region(r));
                }
            }
            Act::SetRegion(i) => {
                self.cfg.region = REGIONS[i].0.to_string();
                self.cfg.save();
                self.toast(format!("ストアを「{}」に変更しました", REGIONS[i].1));
            }
            Act::Theme => {
                self.cfg.theme = self.cfg.theme.next();
                self.cfg.save();
                if let Some(r) = self.reader.as_mut() {
                    // Text pages are drawn in theme colours: re-render them.
                    if r.reflowable() {
                        r.cache.clear();
                        r.params = None;
                    }
                }
                self.toast(format!("テーマ: {}", self.cfg.theme.label()));
            }
            Act::Help => self.show_help = !self.show_help,
            Act::Back => self.back_to_library(),
            Act::MenuMode => {
                if !matches!(prev_menu, Some(Menu::Mode(_))) {
                    let r = self.hits.iter().find(|h| h.1 == Act::MenuMode).map(|h| h.0).unwrap_or_default();
                    self.menu = Some(Menu::Mode(r));
                }
            }
            Act::SetMode(m) => self.set_mode(m),
            Act::ToggleCover => {
                if let Some(r) = self.reader.as_mut() {
                    r.cover = !r.cover;
                    r.page = layout::normalize(r.mode, r.cover, r.n, r.page);
                }
            }
            Act::ToggleVertical => {
                if let Some(r) = self.reader.as_mut() {
                    if r.reflowable() {
                        r.vertical_text = !r.vertical_text;
                        // Vertical Japanese text reads right-to-left.
                        if r.mode.is_spread() {
                            r.mode = if r.vertical_text { Mode::SpreadRtl } else { Mode::SpreadLtr };
                        }
                    }
                }
            }
            Act::ZoomIn => self.set_zoom(|z| zoom::zoom_in(z)),
            Act::ZoomOut => self.set_zoom(|z| zoom::zoom_out(z)),
            Act::ZoomReset => self.set_zoom(|_| 100),
            Act::ToggleFit => {
                if let Some(r) = self.reader.as_mut() {
                    r.fit_width = !r.fit_width;
                    r.pan = (0, 0);
                }
            }
            Act::ToggleProps => self.show_props = !self.show_props,
            Act::Fullscreen => self.fullscreen = !self.fullscreen,
            Act::Slider => {}
            Act::CloseOverlay => {
                self.show_help = false;
            }
        }
    }

    fn set_mode(&mut self, m: Mode) {
        let Some(r) = self.reader.as_mut() else { return };
        r.mode = m;
        r.page = layout::normalize(m, r.cover, r.n, r.page);
        r.pan = (0, 0);
        r.voff = 0;
        if r.reflowable() && m.is_spread() {
            r.vertical_text = m == Mode::SpreadRtl && r.vertical_text;
        }
        self.toast(format!("表示: {}", m.label()));
    }

    fn set_zoom(&mut self, f: impl Fn(u64) -> u64) {
        let Some(r) = self.reader.as_mut() else { return };
        let z = zoom::clamp_zoom(f(r.zoom));
        r.zoom = z;
        // Start at the reading edge: right for right-to-left books.
        r.pan = if r.rtl() { (u64::MAX, 0) } else { (0, 0) };
        self.toast(format!("ズーム {z}%"));
    }

    // -------------------------------------------------------- navigation

    fn content_rect(&self) -> Rect {
        let (w, h) = (self.size.0 as i32, self.size.1 as i32);
        let chrome = !self.fullscreen;
        let top = if chrome { self.s(TOOLBAR) } else { 0 };
        let bottom = if chrome { h - self.s(STATUS) } else { h };
        let right = if self.show_props && self.screen == Screen::Reader { w - self.s(PROPS_W) } else { w };
        Rect::new(0, top, right.max(1), (bottom - top).max(1))
    }

    fn after_move(&mut self, old_reached: u64) {
        let area = self.content_rect();
        let Some(r) = self.reader.as_mut() else { return };
        let new_reached = reached_page(r, area, self.scale);
        if new_reached > old_reached {
            let elapsed = r.last_turn.elapsed().as_millis() as u64;
            r.est = progress::update_speed(r.est, elapsed, new_reached - old_reached);
        }
        r.last_turn = Instant::now();
        r.reached = new_reached;
        if r.reflowable() {
            if let Some(&s) = r.starts.get(r.page as usize) {
                r.char_off = s;
            }
        }
        r.turns += 1;
        // Persist often enough that a crash or `kill` loses little.
        if self.last_save.elapsed() > Duration::from_secs(5) {
            self.save_position();
        }
    }

    fn turn(&mut self, forward: bool) {
        let area = self.content_rect();
        let s = self.scale;
        let Some(r) = self.reader.as_mut() else { return };
        if r.n == 0 {
            return;
        }
        let old = reached_page(r, area, s);
        if r.mode == Mode::Vertical {
            let step = (area.h as f64 * 0.9) as i64;
            vscroll(r, area, s, if forward { step } else { -step });
        } else {
            let p = if forward { layout::next_page(r.mode, r.cover, r.n, r.page) } else { layout::prev_page(r.mode, r.cover, r.n, r.page) };
            if p != r.page {
                r.page = p;
                r.pan = if r.rtl() { (u64::MAX, 0) } else { (0, 0) };
                if !forward {
                    r.pan.1 = u64::MAX; // going back: show the end of the page
                }
            }
        }
        self.after_move(old);
    }

    fn go_to(&mut self, page: u64) {
        let area = self.content_rect();
        let s = self.scale;
        let Some(r) = self.reader.as_mut() else { return };
        if r.n == 0 {
            return;
        }
        let old = reached_page(r, area, s);
        r.page = layout::normalize(r.mode, r.cover, r.n, page);
        r.voff = 0;
        r.pan = (0, 0);
        self.after_move(old);
        // Jumps are not reading: don't learn speed from them.
        if let Some(r) = self.reader.as_mut() {
            r.last_turn = Instant::now();
        }
    }

    // ------------------------------------------------------------ drawing

    fn draw(&mut self, cv: &mut Canvas) {
        self.hits.clear();
        let c = self.c();
        cv.set_clip(None);
        match self.screen {
            Screen::Library => self.draw_library(cv, &c),
            Screen::Reader => self.draw_reader(cv, &c),
        }
        cv.set_clip(None);
        self.draw_menu(cv, &c);
        if self.show_help {
            self.draw_help(cv, &c);
        }
        if let Some((msg, t)) = &self.toast {
            if t.elapsed() < Duration::from_millis(2200) {
                let msg = msg.clone();
                let px = self.px();
                let w = self.gc.measure(&msg, px) as i32 + self.s(32.0);
                let h = self.s(38.0);
                let r = Rect::new((cv.w - w) / 2, cv.h - h - self.s(56.0), w, h);
                cv.fill_round(r, self.s(10.0), rgb(40, 40, 44));
                self.text(cv, &msg, r.x + self.s(16.0), r.y + (h - px as i32) / 2 - self.s(2.0), px, rgb(245, 245, 245));
            } else {
                self.toast = None;
            }
        }
    }

    fn button(&mut self, cv: &mut Canvas, x: i32, y: i32, label: &str, act: Act, on: bool, c: &Colors) -> i32 {
        let px = self.px();
        let w = self.gc.measure(label, px) as i32 + self.s(22.0);
        let h = self.s(32.0);
        let r = Rect::new(x, y, w, h);
        let hovered = self.hover.as_ref() == Some(&act);
        if on {
            cv.fill_round(r, self.s(7.0), c.active);
        } else if hovered {
            cv.fill_round(r, self.s(7.0), c.hover);
        }
        self.text(cv, label, x + self.s(11.0), y + (h - px as i32) / 2 - self.s(2.0), px, c.bar_text);
        self.hits.push((r, act));
        w
    }

    fn toolbar_bg(&self, cv: &mut Canvas, c: &Colors) -> Rect {
        let r = Rect::new(0, 0, cv.w, self.s(TOOLBAR));
        cv.fill(r, c.bar);
        cv.fill(Rect::new(0, r.bottom() - 1, cv.w, 1), c.border);
        r
    }

    fn draw_library(&mut self, cv: &mut Canvas, c: &Colors) {
        cv.fill(Rect::new(0, 0, cv.w, cv.h), c.bg);
        let bar = self.toolbar_bg(cv, c);
        let y = bar.y + (bar.h - self.s(32.0)) / 2;
        let tpx = self.px() * 1.15;
        let mut x = self.s(16.0);
        x += self.text(cv, "Kindle for Linux", x, bar.y + (bar.h - tpx as i32) / 2 - self.s(2.0), tpx, c.accent) + self.s(18.0);
        let sp = self.s(4.0);
        x += self.button(cv, x, y, "ファイルを開く", Act::OpenFile, false, c) + sp;
        x += self.button(cv, x, y, "フォルダを追加", Act::AddFolder, false, c) + sp;
        x += self.button(cv, x, y, if self.scanning { "スキャン中…" } else { "再スキャン" }, Act::Rescan, false, c) + sp;
        x += self.button(cv, x, y, "Kindle Cloud Reader", Act::OpenCloud, false, c) + sp;
        let region = format!("ストア: {} {}", self.cfg.region_label(), down_arrow());
        x += self.button(cv, x, y, &region, Act::MenuRegion, matches!(self.menu, Some(Menu::Region(_))), c) + sp;
        let theme = format!("テーマ: {}", self.cfg.theme.label());
        x += self.button(cv, x, y, &theme, Act::Theme, false, c) + sp;
        self.button(cv, x, y, "？", Act::Help, self.show_help, c);

        // Scrollable sections.
        let area = Rect::new(0, bar.bottom(), cv.w, cv.h - bar.bottom());
        cv.set_clip(Some(area));
        let margin = self.s(28.0);
        let card_w = self.s(CARD_W);
        let card_h = self.s(CARD_H);
        let gap = self.s(GAP);
        let cols = ((area.w - 2 * margin + gap) / (card_w + gap)).max(1);
        let mut y = area.y + self.s(18.0) - self.lib_scroll as i32;

        let recent: Vec<(usize, Item, Option<(u64, u64, u64)>)> = self
            .history
            .entries
            .iter()
            .take(24)
            .enumerate()
            .map(|(i, e)| {
                let it = Item { path: Some(e.path.clone()), title: e.title.clone(), author: e.author.clone(), format: e.format.clone(), asin: None, cloud_only: false };
                (i, it, Some((e.reached.max(e.page), e.pages, e.ms_per_page)))
            })
            .collect();
        let kindle: Vec<(usize, Item, Option<(u64, u64, u64)>)> = self.kindle.iter().cloned().enumerate().map(|(i, it)| (i, it, None)).collect();
        let local: Vec<(usize, Item, Option<(u64, u64, u64)>)> = self.local.iter().cloned().enumerate().map(|(i, it)| (i, it, None)).collect();

        let sections: [(&str, String, &[(usize, Item, Option<(u64, u64, u64)>)], fn(usize) -> Act, &str); 3] = [
            ("最近読んだ本", String::new(), &recent, Act::OpenRecent, "まだ本を開いていません。下の一覧やツールバーの「ファイルを開く」から読み始めましょう。"),
            (
                "Kindle（購入・ダウンロード済み）",
                "DRM 付きの本・KFX 形式は Kindle Cloud Reader（ブラウザ）で開きます".into(),
                &kindle,
                Act::OpenKindle,
                "Kindle の本が見つかりません。Wine の Kindle for PC（My Kindle Content）や USB 接続した Kindle 端末を自動で探します。",
            ),
            (
                "ローカルのドキュメント",
                format!("{} などを検索 (EPUB / PDF / CBZ / TXT / MOBI / AZW3)", self.cfg.folders.iter().take(3).map(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()).collect::<Vec<_>>().join("・")),
                &local,
                Act::OpenLocal,
                "ドキュメントが見つかりません。「フォルダを追加」で本のあるフォルダを登録してください。",
            ),
        ];
        let hpx = self.px() * 1.3;
        for (title, sub, items, act, empty) in sections {
            let label = if items.is_empty() { title.to_string() } else { format!("{title}  {}", items.len()) };
            self.text(cv, &label, margin, y, hpx, c.bar_text);
            y += (hpx * 1.5) as i32;
            if !sub.is_empty() {
                self.text(cv, &sub, margin, y, self.px() * 0.9, c.sub);
                y += (self.px() * 1.6) as i32;
            }
            if items.is_empty() {
                let msg = if self.scanning && title != "最近読んだ本" { "検索中…" } else { empty };
                for line in self.wrap(msg, self.px(), (area.w - 2 * margin) as f32) {
                    self.text(cv, &line, margin, y, self.px(), c.sub);
                    y += (self.px() * 1.6) as i32;
                }
                y += self.s(26.0);
                continue;
            }
            for (k, (idx, item, prog)) in items.iter().enumerate() {
                let col = k as i32 % cols;
                let row = k as i32 / cols;
                let r = Rect::new(margin + col * (card_w + gap), y + row * (card_h + gap), card_w, card_h);
                if r.bottom() >= area.y && r.y <= area.bottom() {
                    self.draw_card(cv, c, r, item, *prog, act(*idx));
                }
            }
            let rows = (items.len() as i32 + cols - 1) / cols;
            y += rows * (card_h + gap) + self.s(14.0);
        }
        self.lib_content_h = (y + self.lib_scroll as i32 - area.y).max(0) as u64;
        cv.set_clip(None);
    }

    fn draw_card(&mut self, cv: &mut Canvas, c: &Colors, r: Rect, item: &Item, prog: Option<(u64, u64, u64)>, act: Act) {
        let hovered = self.hover.as_ref() == Some(&act);
        let cover = Rect::new(r.x, r.y, r.w, self.s(COVER_H));
        if hovered {
            cv.fill_round(Rect::new(r.x - self.s(6.0), r.y - self.s(6.0), r.w + self.s(12.0), r.h + self.s(8.0)), self.s(10.0), c.hover);
        }
        // Cover thumbnail or a generated placeholder.
        let thumb = item.path.as_ref().and_then(|p| match self.thumbs.get(p) {
            Some(t) => t.as_ref(),
            None => {
                if self.thumb_req.insert(p.clone()) {
                    let _ = self.thumbs_tx.send((p.clone(), cover.w as u32, cover.h as u32));
                }
                None
            }
        });
        if let Some(t) = thumb {
            let x = cover.x + scroll::center_pad(t.w as u64, cover.w as u64) as i32;
            let y = cover.bottom() - t.h as i32;
            cv.fill(Rect::new(x + self.s(2.0), y + self.s(3.0), t.w as i32, t.h as i32), c.border);
            cv.blit(t, x, y);
        } else {
            let hue = item.title.chars().fold(0u32, |a, ch| a.wrapping_mul(31).wrapping_add(ch as u32));
            let palette = [rgb(70, 92, 122), rgb(120, 76, 70), rgb(64, 110, 92), rgb(108, 86, 130), rgb(140, 110, 60), rgb(80, 80, 88)];
            let bg = palette[(hue % palette.len() as u32) as usize];
            cv.fill_round(cover, self.s(4.0), bg);
            let px = self.px() * 1.05;
            let mut ty = cover.y + self.s(18.0);
            for line in self.wrap(&item.title, px, (cover.w - self.s(20.0)) as f32).into_iter().take(5) {
                self.text(cv, &line, cover.x + self.s(10.0), ty, px, rgb(250, 248, 242));
                ty += (px * 1.4) as i32;
            }
            let f = item.format.clone();
            self.text(cv, &f, cover.x + self.s(10.0), cover.bottom() - self.s(26.0), self.px() * 0.85, rgb(225, 225, 225));
        }
        if item.cloud_only {
            let label = "Cloud Reader";
            let px = self.px() * 0.8;
            let w = self.gc.measure(label, px) as i32 + self.s(12.0);
            let b = Rect::new(cover.right() - w - self.s(6.0), cover.bottom() - self.s(26.0), w, self.s(20.0));
            cv.fill_round(b, self.s(5.0), c.accent);
            self.text(cv, label, b.x + self.s(6.0), b.y + self.s(2.0), px, rgb(255, 255, 255));
        }
        // Progress bar for recently read books.
        let mut ty = cover.bottom() + self.s(8.0);
        let mut sub = item.author.clone();
        if let Some((reached, n, est)) = prog {
            if n > 0 {
                let pct = progress::percent(reached, n);
                let bar = Rect::new(r.x, cover.bottom() + self.s(4.0), r.w, self.s(3.0));
                cv.fill(bar, c.border);
                cv.fill(Rect::new(bar.x, bar.y, (bar.w as u64 * pct / 100) as i32, bar.h), c.accent);
                ty += self.s(4.0);
                let rem = progress::remaining_ms(reached, n, progress::clamp_speed(est));
                sub = if rem == 0 { format!("{pct}% ・ 読了") } else { format!("{pct}% ・ {}", progress::format_remaining(rem).replace("読了まで", "")) };
            }
        }
        let px = self.px() * 0.95;
        self.text_fit(cv, &item.title, r.x, ty, px, r.w, c.bar_text);
        self.text_fit(cv, &sub, r.x, ty + (px * 1.45) as i32, px * 0.92, r.w, c.sub);
        self.hits.push((r, act));
    }

    fn draw_reader(&mut self, cv: &mut Canvas, c: &Colors) {
        let area = self.content_rect();
        cv.fill(Rect::new(0, 0, cv.w, cv.h), c.reader_bg);
        let s = self.scale;
        let theme = (c.page_bg, c.page_fg);
        let font_px = self.cfg.font_px;
        let budget = self.cfg.cache_mb * 1024 * 1024;
        let worker = self.worker.clone();
        let mut loading: Option<String> = None;
        if let Some(r) = self.reader.as_mut() {
            if let Some(e) = &r.error {
                loading = Some(e.clone());
            } else if r.meta.is_none() {
                loading = Some("読み込み中…".into());
            } else if let Some(note) = r.meta.as_ref().and_then(|m| (!m.readable).then(|| m.note.clone()).flatten()) {
                loading = Some(note);
            } else {
                // Re-flow text when the page box / font / colours change.
                if r.reflowable() {
                    let want = text_params(r, area, s, font_px, theme);
                    if r.params != Some(want) {
                        r.params = Some(want);
                        r.layout_gen += 1;
                        r.relayout_pending = true;
                        r.cache.clear();
                        r.pending.clear();
                        let _ = worker.send(Cmd::Relayout(want, r.layout_gen));
                    }
                }
                if r.relayout_pending {
                    loading = Some("レイアウト中…".into());
                } else {
                    let placed = place_pages(r, area, s);
                    cv.set_clip(Some(area));
                    r.tick += 1;
                    let tick = r.tick;
                    for p in &placed {
                        if let Some(e) = r.cache.iter_mut().find(|e| e.key == p.key) {
                            e.used = tick;
                            cv.blit(&e.pix, p.dest.x, p.dest.y);
                        } else {
                            // Show an older rendering of the same page scaled, or a blank page.
                            if let Some(e) = r.cache.iter().filter(|e| e.key.page == p.page as usize).max_by_key(|e| e.used) {
                                cv.blit_scaled(&e.pix, p.dest);
                            } else {
                                cv.fill(p.dest, c.page_bg);
                            }
                            if r.pending.insert(p.key) {
                                let _ = worker.send(Cmd::Render(p.key));
                            }
                        }
                    }
                    // Prefetch the next pages.
                    for k in prefetch_keys(r, area, s, &placed) {
                        if !r.cache.iter().any(|e| e.key == k) && r.pending.insert(k) {
                            let _ = worker.send(Cmd::Render(k));
                        }
                    }
                    // Evict least recently used bitmaps over the memory budget.
                    let visible: HashSet<Key> = placed.iter().map(|p| p.key).collect();
                    let mut total: usize = r.cache.iter().map(|e| e.pix.bytes()).sum();
                    while total > budget && r.cache.len() > visible.len() {
                        let Some((i, _)) = r.cache.iter().enumerate().filter(|(_, e)| !visible.contains(&e.key)).min_by_key(|(_, e)| e.used) else { break };
                        total -= r.cache[i].pix.bytes();
                        r.cache.swap_remove(i);
                    }
                    cv.set_clip(None);
                }
            }
        }
        if let Some(msg) = loading {
            let px = self.px() * 1.1;
            let mut y = area.y + area.h / 2 - self.s(30.0);
            for line in self.wrap(&msg, px, (area.w - self.s(80.0)).max(100) as f32) {
                let w = self.gc.measure(&line, px) as i32;
                self.text(cv, &line, area.x + (area.w - w) / 2, y, px, c.bar_text);
                y += (px * 1.6) as i32;
            }
            let has_asin = self.reader.as_ref().and_then(|r| r.meta.as_ref()).is_some_and(|m| m.asin.is_some() && !m.readable);
            if has_asin {
                let label = "Kindle Cloud Reader で開く";
                let w = self.gc.measure(label, self.px()) as i32 + self.s(22.0);
                self.button(cv, area.x + (area.w - w) / 2, y + self.s(12.0), label, Act::CloudCurrent, true, c);
            }
        }
        let show_chrome = !self.fullscreen || self.chrome_peek;
        if show_chrome {
            self.draw_reader_toolbar(cv, c);
            self.draw_status(cv, c);
        }
        if self.show_props {
            self.draw_props(cv, c);
        }
    }

    fn draw_reader_toolbar(&mut self, cv: &mut Canvas, c: &Colors) {
        let bar = self.toolbar_bg(cv, c);
        let y = bar.y + (bar.h - self.s(32.0)) / 2;
        let sp = self.s(4.0);
        let mut x = self.s(8.0);
        let Some(r) = self.reader.as_ref() else { return };
        let (mode, cover, z, fitw, vert, reflow) = (r.mode, r.cover, r.zoom, r.fit_width, r.vertical_text, r.reflowable());
        let back = format!("{} ライブラリ", first_glyph(&['‹', '←', '<']));
        x += self.button(cv, x, y, &back, Act::Back, false, c) + self.s(10.0);
        let ml = format!("{} {}", mode.label(), down_arrow());
        x += self.button(cv, x, y, &ml, Act::MenuMode, matches!(self.menu, Some(Menu::Mode(_))), c) + sp;
        if mode.is_spread() {
            x += self.button(cv, x, y, "表紙を単独表示", Act::ToggleCover, cover, c) + sp;
        }
        if reflow {
            x += self.button(cv, x, y, "縦書き", Act::ToggleVertical, vert, c) + sp;
        }
        x += self.s(8.0);
        x += self.button(cv, x, y, "－", Act::ZoomOut, false, c);
        let zl = format!("{z}%");
        x += self.button(cv, x, y, &zl, Act::ZoomReset, false, c);
        x += self.button(cv, x, y, "＋", Act::ZoomIn, false, c) + sp;
        if !reflow {
            x += self.button(cv, x, y, "幅に合わせる", Act::ToggleFit, fitw, c) + sp;
        }
        // Right-aligned group.
        let px = self.px();
        let labels = ["？", "全画面", "プロパティ", "テーマ"];
        let total: i32 = labels.iter().map(|l| self.gc.measure(l, px) as i32 + self.s(22.0) + sp).sum();
        let mut rx = (cv.w - total - self.s(8.0)).max(x + self.s(8.0));
        rx += self.button(cv, rx, y, "テーマ", Act::Theme, false, c) + sp;
        rx += self.button(cv, rx, y, "プロパティ", Act::ToggleProps, self.show_props, c) + sp;
        rx += self.button(cv, rx, y, "全画面", Act::Fullscreen, self.fullscreen, c) + sp;
        self.button(cv, rx, y, "？", Act::Help, self.show_help, c);
    }

    fn progress_numbers(&self) -> Option<(u64, u64, u64, u64, u64)> {
        let r = self.reader.as_ref()?;
        if r.n == 0 || r.relayout_pending {
            return None;
        }
        let area = self.content_rect();
        let first = if r.mode == Mode::Vertical { r.page } else { *layout::visible(r.mode, r.cover, r.n, r.page).first()? };
        let last = reached_page(r, area, self.scale);
        let pct = progress::percent(last, r.n);
        let rem = progress::remaining_ms(last, r.n, r.est);
        Some((first, last, r.n, pct, rem))
    }

    fn draw_status(&mut self, cv: &mut Canvas, c: &Colors) {
        let h = self.s(STATUS);
        let bar = Rect::new(0, cv.h - h, cv.w, h);
        cv.fill(bar, c.bar);
        cv.fill(Rect::new(0, bar.y, cv.w, 1), c.border);
        let Some((first, last, n, pct, rem)) = self.progress_numbers() else { return };
        let px = self.px() * 0.95;
        let ty = bar.y + (h - px as i32) / 2 - self.s(2.0);
        let left = if last > first { format!("{}–{} / {} ページ", first + 1, last + 1, n) } else { format!("{} / {} ページ", first + 1, n) };
        let right = format!("{pct}%  ・  {}", progress::format_remaining(rem));
        let lw = self.text(cv, &left, self.s(14.0), ty, px, c.bar_text);
        let rw = self.gc.measure(&right, px) as i32;
        self.text(cv, &right, cv.w - rw - self.s(14.0), ty, px, c.bar_text);
        // Slider (reversed for right-to-left books, like a printed manga).
        let track = Rect::new(self.s(28.0) + lw, bar.y + h / 2 - self.s(2.0), (cv.w - lw - rw - self.s(56.0)).max(0), self.s(4.0));
        if track.w > self.s(40.0) {
            cv.fill_round(track, self.s(2.0), c.border);
            let rtl = self.reader.as_ref().is_some_and(|r| r.rtl());
            let frac = if n > 1 { first * 1000 / (n - 1) } else { 0 };
            let frac = if rtl { 1000 - frac } else { frac };
            let kx = track.x + (track.w as u64 * frac / 1000) as i32;
            if rtl {
                cv.fill_round(Rect::new(kx, track.y, track.right() - kx, track.h), self.s(2.0), c.accent);
            } else {
                cv.fill_round(Rect::new(track.x, track.y, kx - track.x, track.h), self.s(2.0), c.accent);
            }
            let k = self.s(14.0);
            cv.fill_round(Rect::new(kx - k / 2, track.y + track.h / 2 - k / 2, k, k), k / 2, c.accent);
            self.hits.push((Rect::new(track.x - k, bar.y, track.w + 2 * k, h), Act::Slider));
        }
    }

    fn slider_to(&mut self, mx: f64) {
        let Some((rect, _)) = self.hits.iter().find(|h| h.1 == Act::Slider).cloned() else { return };
        let k = self.s(14.0);
        let (x0, w) = (rect.x + k, (rect.w - 2 * k).max(1));
        let mut permille = (((mx as i32 - x0).clamp(0, w)) as u64 * 1000) / w as u64;
        let Some(r) = self.reader.as_ref() else { return };
        if r.rtl() {
            permille = 1000 - permille;
        }
        let n = r.n;
        let target = if n > 1 { (permille * (n - 1) + 500) / 1000 } else { layout::page_for_permille(n, permille) };
        self.go_to(target.min(n.saturating_sub(1)));
    }

    fn draw_props(&mut self, cv: &mut Canvas, c: &Colors) {
        let top = if self.fullscreen && !self.chrome_peek { 0 } else { self.s(TOOLBAR) };
        let bottom = if self.fullscreen && !self.chrome_peek { cv.h } else { cv.h - self.s(STATUS) };
        let panel = Rect::new(cv.w - self.s(PROPS_W), top, self.s(PROPS_W), bottom - top);
        cv.fill(panel, c.bar);
        cv.fill(Rect::new(panel.x, panel.y, 1, panel.h), c.border);
        let Some(r) = self.reader.as_ref() else { return };
        let mut rows: Vec<(&str, String)> = Vec::new();
        let size = std::fs::metadata(&r.path).map(|m| m.len()).unwrap_or(0);
        if let Some(m) = &r.meta {
            rows.push(("タイトル", m.title.clone()));
            rows.push(("著者", if m.author.is_empty() { "—".into() } else { m.author.clone() }));
            rows.push(("形式", m.format.to_string()));
            if let Some(a) = &m.asin {
                rows.push(("ASIN", a.clone()));
            }
            rows.push(("読む方向", if m.rtl { "右→左".into() } else { "左→右".into() }));
        }
        rows.push(("ファイル", r.path.to_string_lossy().into_owned()));
        rows.push(("サイズ", format!("{:.1} MB", size as f64 / 1_048_576.0)));
        if let Some((first, last, n, pct, rem)) = self.progress_numbers() {
            rows.push(("ページ", if last > first { format!("{}–{} / {}", first + 1, last + 1, n) } else { format!("{} / {}", first + 1, n) }));
            rows.push(("進捗", format!("{pct}%  (残り {} ページ)", progress::remaining_pages(last, n))));
            rows.push(("残り時間", progress::format_remaining(rem)));
        }
        let r = self.reader.as_ref().unwrap();
        rows.push(("読む速さ", format!("1ページ平均 {:.0} 秒", r.est as f64 / 1000.0)));
        rows.push(("表示モード", r.mode.label().into()));
        rows.push(("ズーム", format!("{}%{}", r.zoom, if r.fit_width && !r.reflowable() { "（幅に合わせる）" } else { "" })));
        if r.reflowable() {
            rows.push(("組方向", if r.vertical_text { "縦書き".into() } else { "横書き".into() }));
        }
        let mem: usize = r.cache.iter().map(|e| e.pix.bytes()).sum();
        rows.push(("ページキャッシュ", format!("{:.1} MB", mem as f64 / 1_048_576.0)));

        let pad = self.s(18.0);
        let mut y = panel.y + pad;
        let hpx = self.px() * 1.15;
        self.text(cv, "プロパティ", panel.x + pad, y, hpx, c.bar_text);
        y += (hpx * 1.9) as i32;
        let px = self.px() * 0.92;
        let w = (panel.w - 2 * pad) as f32;
        cv.set_clip(Some(panel));
        for (k, v) in rows {
            self.text(cv, k, panel.x + pad, y, px * 0.9, c.sub);
            y += (px * 1.45) as i32;
            for line in self.wrap(&v, px, w).into_iter().take(4) {
                self.text(cv, &line, panel.x + pad, y, px, c.bar_text);
                y += (px * 1.45) as i32;
            }
            y += self.s(8.0);
        }
        cv.set_clip(None);
    }

    fn draw_menu(&mut self, cv: &mut Canvas, c: &Colors) {
        let Some(menu) = self.menu else { return };
        let (anchor, items): (Rect, Vec<(String, Act, bool)>) = match menu {
            Menu::Mode(a) => {
                let cur = self.reader.as_ref().map(|r| r.mode);
                (a, Mode::ALL.iter().map(|m| (m.label().to_string(), Act::SetMode(*m), Some(*m) == cur)).collect())
            }
            Menu::Region(a) => (a, REGIONS.iter().enumerate().map(|(i, r)| (format!("{} ({})", r.1, r.2.trim_start_matches("https://").trim_end_matches('/')), Act::SetRegion(i), r.0 == self.cfg.region)).collect()),
        };
        let px = self.px();
        let ih = self.s(34.0);
        let w = items.iter().map(|i| self.gc.measure(&i.0, px) as i32).max().unwrap_or(100) + self.s(48.0);
        let r = Rect::new(anchor.x, anchor.bottom() + self.s(4.0), w, ih * items.len() as i32 + self.s(8.0));
        cv.fill_round(Rect::new(r.x + 2, r.y + 3, r.w, r.h), self.s(8.0), c.border);
        cv.fill_round(r, self.s(8.0), c.bar);
        for (i, (label, act, on)) in items.into_iter().enumerate() {
            let ir = Rect::new(r.x + self.s(4.0), r.y + self.s(4.0) + i as i32 * ih, r.w - self.s(8.0), ih);
            if self.hover.as_ref() == Some(&act) {
                cv.fill_round(ir, self.s(6.0), c.hover);
            }
            if on {
                self.text(cv, "✓", ir.x + self.s(8.0), ir.y + (ih - px as i32) / 2 - self.s(2.0), px, c.accent);
            }
            self.text(cv, &label, ir.x + self.s(30.0), ir.y + (ih - px as i32) / 2 - self.s(2.0), px, c.bar_text);
            self.hits.push((ir, act));
        }
    }

    fn draw_help(&mut self, cv: &mut Canvas, c: &Colors) {
        let rows = [
            ("→ / ←", "ページ送り（右→左の本では ← が次へ）"),
            ("Space / PageDown", "次へ　（Shift+Space / PageUp で前へ）"),
            ("Home / End", "最初 / 最後のページ"),
            ("クリック（左右の端）", "ページ送り　　ドラッグ: 拡大時の移動"),
            ("Ctrl + ホイール, + / -", "拡大・縮小　（0 で 100%）"),
            ("W", "幅に合わせる"),
            ("M", "表示モード切替（単ページ / 見開き / 縦スクロール）"),
            ("C", "表紙を単独表示"),
            ("V", "縦書き / 横書き（テキストの本）"),
            ("I", "プロパティ"),
            ("T", "テーマ（ライト / セピア / ダーク）"),
            ("F11 / F", "全画面　　Esc: 解除"),
            ("L / Backspace", "ライブラリへ戻る"),
            ("Ctrl + O", "ファイルを開く"),
            ("Ctrl + Q", "終了"),
        ];
        let px = self.px();
        let lh = (px * 1.9) as i32;
        let w = self.s(620.0).min(cv.w - self.s(40.0));
        let h = lh * rows.len() as i32 + self.s(80.0);
        let r = Rect::new((cv.w - w) / 2, (cv.h - h) / 2, w, h);
        cv.fill_alpha(Rect::new(0, 0, cv.w, cv.h), rgb(0, 0, 0), 90);
        cv.fill_round(r, self.s(12.0), c.bar);
        self.hits.push((Rect::new(0, 0, cv.w, cv.h), Act::CloseOverlay));
        let mut y = r.y + self.s(22.0);
        self.text(cv, "キーボードとマウス", r.x + self.s(24.0), y, px * 1.2, c.bar_text);
        y += self.s(40.0);
        for (k, v) in rows {
            self.text(cv, k, r.x + self.s(24.0), y, px, c.accent);
            self.text_fit(cv, v, r.x + self.s(230.0), y, px, r.w - self.s(250.0), c.bar_text);
            y += lh;
        }
    }

    // -------------------------------------------------------------- input

    fn hit(&self, x: f64, y: f64) -> Option<Act> {
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, a)| a.clone())
    }

    fn click(&mut self, x: f64, y: f64) {
        if let Some(a) = self.hit(x, y) {
            if a == Act::Slider {
                self.slider_to(x);
            } else {
                self.do_act(a);
            }
            return;
        }
        if self.menu.take().is_some() {
            return;
        }
        if self.screen == Screen::Reader {
            let area = self.content_rect();
            if !area.contains(x, y) {
                return;
            }
            let Some(r) = self.reader.as_ref() else { return };
            if r.mode == Mode::Vertical {
                return;
            }
            let rel = (x - area.x as f64) / area.w as f64;
            let rtl = r.rtl();
            if rel < 0.35 {
                self.turn(rtl);
            } else if rel > 0.65 {
                self.turn(!rtl);
            } else if self.fullscreen {
                self.chrome_peek = !self.chrome_peek;
            }
        }
    }

    fn wheel(&mut self, dy: f64) {
        if self.show_help {
            return;
        }
        if self.screen == Screen::Library {
            let area_h = self.size.1 as u64 - self.s(TOOLBAR) as u64;
            let target = (self.lib_scroll as i64 - dy as i64).max(0) as u64;
            self.lib_scroll = scroll::clamp_offset(target, self.lib_content_h + self.s(40.0) as u64, area_h);
            return;
        }
        if self.mods.control_key() {
            if dy > 0.0 {
                self.do_act(Act::ZoomIn)
            } else if dy < 0.0 {
                self.do_act(Act::ZoomOut)
            }
            return;
        }
        let area = self.content_rect();
        let s = self.scale;
        let Some(r) = self.reader.as_mut() else { return };
        if r.mode == Mode::Vertical {
            let old = reached_page(r, area, s);
            vscroll(r, area, s, -dy as i64);
            self.after_move(old);
            return;
        }
        // Zoomed in: scroll inside the page first.
        let placed = place_pages(r, area, s);
        let (_, th) = spread_size(&placed);
        if th > area.h as u64 {
            let maxy = th - area.h as u64;
            let cur = r.pan.1.min(maxy);
            if (dy < 0.0 && cur < maxy) || (dy > 0.0 && cur > 0) {
                r.pan.1 = (cur as i64 - dy as i64).clamp(0, maxy as i64) as u64;
                return;
            }
        }
        r.wheel_acc += dy;
        if r.wheel_acc.abs() >= 60.0 * s as f64 && r.last_wheel.elapsed() > Duration::from_millis(180) {
            let forward = r.wheel_acc < 0.0;
            r.wheel_acc = 0.0;
            r.last_wheel = Instant::now();
            self.turn(forward);
        }
    }

    fn key(&mut self, key: &WKey, pressed_text: Option<&str>) -> bool {
        let ctrl = self.mods.control_key();
        let shift = self.mods.shift_key();
        if let WKey::Named(n) = key {
            match n {
                NamedKey::Escape => {
                    if self.menu.take().is_some() || std::mem::take(&mut self.show_help) {
                    } else if self.fullscreen {
                        self.fullscreen = false;
                    }
                    return true;
                }
                NamedKey::F11 => self.fullscreen = !self.fullscreen,
                NamedKey::F1 => self.show_help = !self.show_help,
                _ => {}
            }
        }
        if self.screen == Screen::Library {
            match key {
                WKey::Named(NamedKey::PageDown) => self.wheel(-(self.size.1 as f64 * 0.8)),
                WKey::Named(NamedKey::PageUp) => self.wheel(self.size.1 as f64 * 0.8),
                WKey::Named(NamedKey::ArrowDown) => self.wheel(-60.0),
                WKey::Named(NamedKey::ArrowUp) => self.wheel(60.0),
                WKey::Named(NamedKey::Home) => self.lib_scroll = 0,
                _ => {}
            }
        } else {
            let rtl = self.reader.as_ref().is_some_and(|r| r.rtl());
            let vertical = self.reader.as_ref().is_some_and(|r| r.mode == Mode::Vertical);
            match key {
                WKey::Named(NamedKey::ArrowRight) => self.turn(!rtl),
                WKey::Named(NamedKey::ArrowLeft) => self.turn(rtl),
                WKey::Named(NamedKey::ArrowDown) if vertical => self.wheel(-80.0 * self.scale as f64),
                WKey::Named(NamedKey::ArrowUp) if vertical => self.wheel(80.0 * self.scale as f64),
                WKey::Named(NamedKey::ArrowDown) => self.turn(true),
                WKey::Named(NamedKey::ArrowUp) => self.turn(false),
                WKey::Named(NamedKey::Space) => self.turn(!shift),
                WKey::Named(NamedKey::PageDown) => self.turn(true),
                WKey::Named(NamedKey::PageUp) => self.turn(false),
                WKey::Named(NamedKey::Home) => self.go_to(0),
                WKey::Named(NamedKey::End) => {
                    let n = self.reader.as_ref().map(|r| r.n).unwrap_or(0);
                    self.go_to(n.saturating_sub(1));
                }
                WKey::Named(NamedKey::Backspace) => self.back_to_library(),
                _ => {}
            }
        }
        if let Some(t) = pressed_text {
            let reader = self.screen == Screen::Reader;
            match (t, ctrl) {
                ("q", true) => self.quit = true,
                ("o", true) => self.do_act(Act::OpenFile),
                ("+" | "=" | ";", _) if reader => self.do_act(Act::ZoomIn),
                ("-", _) if reader => self.do_act(Act::ZoomOut),
                ("0", _) if reader => self.do_act(Act::ZoomReset),
                ("w", false) if reader => self.do_act(Act::ToggleFit),
                ("m", false) if reader => {
                    let cur = self.reader.as_ref().map(|r| r.mode).unwrap_or(Mode::Single);
                    let i = Mode::ALL.iter().position(|m| *m == cur).unwrap_or(0);
                    self.set_mode(Mode::ALL[(i + 1) % Mode::ALL.len()]);
                }
                ("c", false) if reader => self.do_act(Act::ToggleCover),
                ("v", false) if reader => self.do_act(Act::ToggleVertical),
                ("i", false) if reader => self.do_act(Act::ToggleProps),
                ("f", false) => self.fullscreen = !self.fullscreen,
                ("t", false) => self.do_act(Act::Theme),
                ("l", _) if reader => self.back_to_library(),
                ("?", _) => self.show_help = !self.show_help,
                _ => {}
            }
        }
        true
    }

    // ------------------------------------------------------ worker events

    fn on_reply(&mut self, rep: Reply) {
        let area = self.content_rect();
        let s = self.scale;
        let window_wide = area.w > area.h * 6 / 5;
        let Some(r) = self.reader.as_mut() else { return };
        match rep {
            Reply::Opened { gen, meta, pages, sizes } if gen == r.gen => {
                r.n = (pages as u64).min(progress::MAX_PAGES);
                r.naturals = sizes;
                if !r.restored {
                    // Sensible defaults for a book opened for the first time.
                    r.vertical_text = meta.vertical_text;
                    r.mode = if window_wide && (!meta.reflowable || meta.vertical_text) {
                        if meta.rtl { Mode::SpreadRtl } else { Mode::SpreadLtr }
                    } else {
                        Mode::Single
                    };
                    r.cover = !meta.reflowable;
                }
                r.page = layout::normalize(r.mode, r.cover, r.n, r.page);
                r.last_turn = Instant::now();
                r.meta = Some(meta);
                if !r.reflowable() {
                    r.reached = reached_page(r, area, s);
                    self.save_position();
                }
            }
            Reply::OpenFailed { gen, err } if gen == r.gen => r.error = Some(err),
            Reply::Relayouted { gen, layout, starts } if gen == r.gen && layout == r.layout_gen => {
                r.relayout_pending = false;
                r.n = (starts.len() as u64).min(progress::MAX_PAGES);
                // Keep the reading position: the page holding the same character
                // (`locate` is verified in Lean to return a valid index).
                let p = if starts.is_empty() { 0 } else { scroll::locate(&starts, r.char_off) };
                r.starts = starts;
                r.page = layout::normalize(r.mode, r.cover, r.n, p);
                r.voff = 0;
                r.reached = reached_page(r, area, s);
                self.save_position();
            }
            Reply::Rendered { gen, key, natural, result } if gen == r.gen => {
                r.pending.remove(&key);
                if key.layout != r.layout_gen {
                    return;
                }
                if let (Some(nat), Some(slot)) = (natural, r.naturals.get_mut(key.page)) {
                    *slot = Some(nat);
                }
                match result {
                    Ok(pix) => {
                        r.tick += 1;
                        r.cache.retain(|e| e.key != key);
                        r.cache.push(Cached { key, pix, used: r.tick });
                    }
                    Err(e) => self.toast = Some((e, Instant::now())),
                }
            }
            Reply::Dropped { gen, key } if gen == r.gen => {
                r.pending.remove(&key);
            }
            _ => {}
        }
    }
}

fn first_glyph(cands: &[char]) -> char {
    cands.iter().copied().find(|&c| GlyphCache::has_glyph(c)).unwrap_or(*cands.last().unwrap())
}

fn down_arrow() -> char {
    first_glyph(&['▾', '▼', 'v'])
}

// ------------------------------------------------------------- page layout

fn text_params(r: &Reader, area: Rect, s: f32, font_px: f32, theme: (u32, u32)) -> LayoutParams {
    let (w, h) = match r.mode {
        Mode::SpreadLtr | Mode::SpreadRtl => (area.w / 2, area.h),
        Mode::Single => (area.w.min((1100.0 * s) as i32), area.h),
        Mode::Vertical => (area.w.min((1000.0 * s) as i32), area.h),
    };
    LayoutParams {
        w: w.max(50) as u32,
        h: h.max(50) as u32,
        font_px: (font_px * s * zoom::scale_by(1000, r.zoom) as f32 / 1000.0).max(6.0),
        vertical: r.vertical_text,
        bg: theme.0,
        fg: theme.1,
    }
}

fn natural(r: &Reader, page: usize, k: &Key) -> (u32, u32) {
    if r.reflowable() {
        return (k.bw, k.bh);
    }
    r.naturals
        .get(page)
        .copied()
        .flatten()
        .or_else(|| r.naturals.iter().flatten().next().copied())
        .unwrap_or((1000, 1414))
}

fn key_for(r: &Reader, page: u64, bw: u32, bh: u32, fit_width: bool) -> Key {
    let text = r.reflowable();
    Key { page: page as usize, bw, bh, fit_width: fit_width && !text, zoom: if text { 100 } else { r.zoom as u32 }, layout: r.layout_gen }
}

fn spread_size(placed: &[Placed]) -> (u64, u64) {
    let w = placed.iter().map(|p| p.dest.w as u64).sum();
    let h = placed.iter().map(|p| p.dest.h as u64).max().unwrap_or(0);
    (w, h)
}

/// Column width and per-page heights for vertical scrolling.
fn vertical_metrics(r: &Reader, area: Rect, s: f32) -> (u32, Vec<u64>, u64) {
    let cw = if r.reflowable() {
        r.params.map(|p| p.w).unwrap_or(area.w as u32)
    } else {
        (area.w as f32).min((area.h as f32 * 0.8).max(600.0 * s)) as u32
    };
    let gap = (12.0 * s) as u64;
    let hs: Vec<u64> = (0..r.n)
        .map(|p| {
            if r.reflowable() {
                r.params.map(|q| q.h as u64).unwrap_or(area.h as u64)
            } else {
                let k = key_for(r, p, cw, 0, true);
                target_size(natural(r, p as usize, &k), &k).1 as u64
            }
        })
        .collect();
    (cw, hs, gap)
}

/// Pages on screen and where to draw them. All offsets and centring use the
/// Lean-verified `clamp_offset` / `center_pad` / `starts` / `locate`.
fn place_pages(r: &mut Reader, area: Rect, s: f32) -> Vec<Placed> {
    if r.n == 0 {
        return vec![];
    }
    if r.mode == Mode::Vertical {
        let (cw, hs, gap) = vertical_metrics(r, area, s);
        let st = scroll::starts(&hs, gap, 0);
        let total = scroll::total_height(&hs, gap);
        let page = r.page.min(r.n - 1);
        let abs = scroll::clamp_offset(st[page as usize] + r.voff, total, area.h as u64);
        r.page = scroll::locate(&st, abs);
        r.voff = abs - st[r.page as usize];
        let k0 = key_for(r, 0, cw, 0, true);
        let colw = target_size((1000, 1000), &k0).0 as u64;
        let colw = if r.reflowable() { cw as u64 } else { colw };
        let x = if colw > area.w as u64 {
            r.pan.0 = scroll::clamp_offset(r.pan.0, colw, area.w as u64);
            area.x - r.pan.0 as i32
        } else {
            area.x + scroll::center_pad(colw, area.w as u64) as i32
        };
        let mut out = Vec::new();
        let mut i = r.page as usize;
        while i < hs.len() && st[i] < abs + area.h as u64 {
            let k = key_for(r, i as u64, cw, 0, true);
            let (w, h) = if r.reflowable() { (cw, hs[i] as u32) } else { target_size(natural(r, i, &k), &k) };
            let y = area.y + (st[i] as i64 - abs as i64) as i32;
            out.push(Placed { page: i as u64, dest: Rect::new(x + scroll::center_pad(w as u64, colw) as i32, y, w as i32, h as i32), key: k });
            i += 1;
        }
        return out;
    }
    let pages = layout::display_order(r.mode, r.cover, r.n, r.page);
    let (bw, bh) = if r.reflowable() {
        r.params.map(|p| (p.w, p.h)).unwrap_or((area.w as u32, area.h as u32))
    } else if r.mode.is_spread() {
        ((area.w / 2).max(1) as u32, area.h as u32)
    } else {
        (area.w as u32, area.h as u32)
    };
    let mut sizes = Vec::new();
    for &p in &pages {
        let k = key_for(r, p, bw, bh, r.fit_width);
        sizes.push((p, k, target_size(natural(r, p as usize, &k), &k)));
    }
    let tw: u64 = sizes.iter().map(|s| s.2 .0 as u64).sum();
    let th: u64 = sizes.iter().map(|s| s.2 .1 as u64).max().unwrap_or(0);
    let (aw, ah) = (area.w as u64, area.h as u64);
    let x0 = if tw > aw {
        r.pan.0 = scroll::clamp_offset(r.pan.0, tw, aw);
        area.x - r.pan.0 as i32
    } else {
        r.pan.0 = 0;
        area.x + scroll::center_pad(tw, aw) as i32
    };
    let y0 = if th > ah {
        r.pan.1 = scroll::clamp_offset(r.pan.1, th, ah);
        area.y - r.pan.1 as i32
    } else {
        r.pan.1 = 0;
        area.y + scroll::center_pad(th, ah) as i32
    };
    let mut x = x0;
    let mut out = Vec::new();
    for (p, k, (w, h)) in sizes {
        let y = y0 + scroll::center_pad(h as u64, th) as i32;
        out.push(Placed { page: p, dest: Rect::new(x, y, w as i32, h as i32), key: k });
        x += w as i32;
    }
    out
}

/// The furthest page that is on screen (drives % and time remaining).
fn reached_page(r: &Reader, area: Rect, s: f32) -> u64 {
    if r.n == 0 {
        return 0;
    }
    if r.mode == Mode::Vertical {
        let (_, hs, gap) = vertical_metrics(r, area, s);
        let st = scroll::starts(&hs, gap, 0);
        let abs = st[r.page.min(r.n - 1) as usize] + r.voff;
        return scroll::locate(&st, abs + (area.h as u64).saturating_sub(1));
    }
    layout::last_visible(r.mode, r.cover, r.n, r.page)
}

fn vscroll(r: &mut Reader, area: Rect, s: f32, delta: i64) {
    if r.n == 0 {
        return;
    }
    let (_, hs, gap) = vertical_metrics(r, area, s);
    let st = scroll::starts(&hs, gap, 0);
    let total = scroll::total_height(&hs, gap);
    let abs = st[r.page.min(r.n - 1) as usize] + r.voff;
    let target = (abs as i64 + delta).max(0) as u64;
    let abs = scroll::clamp_offset(target, total, area.h as u64);
    r.page = scroll::locate(&st, abs);
    r.voff = abs - st[r.page as usize];
}

fn prefetch_keys(r: &Reader, _area: Rect, _s: f32, placed: &[Placed]) -> Vec<Key> {
    let Some(first) = placed.first() else { return vec![] };
    let k = first.key;
    let mut out = Vec::new();
    if r.mode == Mode::Vertical {
        if let Some(last) = placed.iter().map(|p| p.page).max() {
            for p in last + 1..(last + 3).min(r.n) {
                out.push(Key { page: p as usize, ..k });
            }
        }
    } else {
        let next = layout::next_page(r.mode, r.cover, r.n, r.page);
        if next != r.page {
            for p in layout::visible(r.mode, r.cover, r.n, next) {
                out.push(Key { page: p as usize, ..k });
            }
        }
    }
    out
}

// ------------------------------------------------------------ event loop

impl App {
    pub fn new(proxy: EventLoopProxy<UserEvent>, open: Option<PathBuf>) -> App {
        let p1 = proxy.clone();
        let worker = worker::spawn(move |r| {
            let _ = p1.send_event(UserEvent::Worker(r));
        });
        let p2 = proxy.clone();
        let thumbs_tx = worker::spawn_thumbs(move |path, pix| {
            let _ = p2.send_event(UserEvent::Thumb { path, pix });
        });
        let mut state = State {
            proxy,
            cfg: Config::load(),
            history: History::load(),
            screen: Screen::Library,
            reader: None,
            worker,
            thumbs_tx,
            gc: GlyphCache::new(),
            scale: 1.0,
            size: (1, 1),
            mouse: (0.0, 0.0),
            press: None,
            dragged: false,
            hits: vec![],
            hover: None,
            menu: None,
            toast: None,
            show_props: false,
            show_help: false,
            fullscreen: false,
            chrome_peek: false,
            kindle: vec![],
            local: vec![],
            scanning: false,
            lib_scroll: 0,
            lib_content_h: 0,
            thumbs: HashMap::new(),
            thumb_req: HashSet::new(),
            next_gen: 0,
            mods: ModifiersState::empty(),
            quit: false,
            last_save: Instant::now(),
        };
        state.rescan();
        App { window: None, surface: None, state, open_on_start: open }
    }

    fn redraw(&mut self) {
        let (Some(win), Some(surface)) = (&self.window, &mut self.surface) else { return };
        let size = win.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if surface.resize(w, h).is_err() {
            return;
        }
        self.state.size = (size.width, size.height);
        self.state.scale = win.scale_factor() as f32;
        let Ok(mut buf) = surface.buffer_mut() else { return };
        {
            let mut cv = Canvas::new(&mut buf, size.width, size.height);
            self.state.draw(&mut cv);
        }
        let _ = buf.present();
    }

    fn sync_fullscreen(&self) {
        if let Some(w) = &self.window {
            let want = self.state.fullscreen;
            if want != w.fullscreen().is_some() {
                w.set_fullscreen(want.then_some(Fullscreen::Borderless(None)));
            }
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("Kindle for Linux").with_inner_size(LogicalSize::new(1200.0, 820.0)).with_min_inner_size(LogicalSize::new(480.0, 360.0));
        #[cfg(all(unix, not(target_os = "macos")))]
        let attrs = {
            use winit::platform::wayland::WindowAttributesExtWayland;
            WindowAttributesExtWayland::with_name(attrs, "io.github.tarouzirou.KindleForLinux", "")
        };
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("ウィンドウを作成できません: {e}");
                el.exit();
                return;
            }
        };
        let ctx = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        let surface = softbuffer::Surface::new(&ctx, window.clone()).expect("softbuffer surface");
        self.state.scale = window.scale_factor() as f32;
        let sz = window.inner_size();
        self.state.size = (sz.width, sz.height);
        self.window = Some(window);
        self.surface = Some(surface);
        if let Some(p) = self.open_on_start.take() {
            self.state.open_path(p);
        }
        self.window.as_ref().unwrap().request_redraw();
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Worker(r) => self.state.on_reply(r),
            UserEvent::Scanned { kindle, local } => {
                self.state.kindle = kindle;
                self.state.local = local;
                self.state.scanning = false;
            }
            UserEvent::Thumb { path, pix } => {
                if self.state.thumbs.len() > 600 {
                    self.state.thumbs.clear();
                    self.state.thumb_req.clear();
                }
                self.state.thumbs.insert(path, pix);
            }
            UserEvent::Chosen { path, folder } => match path {
                Some(p) if p.as_os_str().is_empty() => {
                    self.state.toast("zenity か kdialog をインストールするか、ファイルをウィンドウにドロップしてください");
                }
                Some(p) if folder => {
                    if !self.state.cfg.folders.contains(&p) {
                        self.state.cfg.folders.push(p);
                        self.state.cfg.save();
                    }
                    self.state.rescan();
                }
                Some(p) => self.state.open_path(p),
                None => {}
            },
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let st = &mut self.state;
        let mut redraw = true;
        match event {
            WindowEvent::CloseRequested => {
                st.save_position();
                el.exit();
                return;
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                // Wake up again only to hide an expiring toast.
                let wake = self.state.toast.as_ref().map(|(_, t)| *t + Duration::from_millis(2300));
                el.set_control_flow(wake.map(ControlFlow::WaitUntil).unwrap_or(ControlFlow::Wait));
                return;
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {}
            WindowEvent::ModifiersChanged(m) => {
                st.mods = m.state();
                redraw = false;
            }
            WindowEvent::DroppedFile(p) => st.open_path(p),
            WindowEvent::CursorMoved { position, .. } => {
                st.mouse = (position.x, position.y);
                let mut changed = false;
                if let Some((px, py, pan0, voff0)) = st.press {
                    let (dx, dy) = (position.x - px, position.y - py);
                    if dx.abs() + dy.abs() > 6.0 {
                        st.dragged = true;
                    }
                    if st.dragged {
                        if st.hits.iter().any(|h| h.1 == Act::Slider && h.0.contains(px, py)) {
                            st.slider_to(position.x);
                        } else {
                            let area = st.content_rect();
                            let s = st.scale;
                            let Some(r) = st.reader.as_mut() else { return };
                            if r.mode == Mode::Vertical {
                                r.voff = voff0;
                                let _ = pan0;
                                vscroll(r, area, s, -dy as i64);
                            } else {
                                r.pan.0 = (pan0.0 as i64 - dx as i64).max(0) as u64;
                                r.pan.1 = (pan0.1 as i64 - dy as i64).max(0) as u64;
                            }
                        }
                        changed = true;
                    }
                }
                let h = st.hit(position.x, position.y);
                if h != st.hover {
                    st.hover = h;
                    changed = true;
                }
                if st.fullscreen && st.screen == Screen::Reader {
                    let peek = position.y < st.s(TOOLBAR) as f64 || position.y > (st.size.1 as i32 - st.s(STATUS)) as f64;
                    if peek != st.chrome_peek && st.menu.is_none() {
                        st.chrome_peek = peek;
                        changed = true;
                    }
                }
                redraw = changed;
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed => {
                    let (pan, voff) = st.reader.as_ref().map(|r| (r.pan, r.voff)).unwrap_or(((0, 0), 0));
                    st.press = Some((st.mouse.0, st.mouse.1, pan, voff));
                    st.dragged = false;
                    redraw = false;
                }
                ElementState::Released => {
                    if !st.dragged {
                        let (x, y) = st.mouse;
                        st.click(x, y);
                    } else if let Some(r) = st.reader.as_mut() {
                        if r.mode == Mode::Vertical {
                            r.last_turn = Instant::now();
                        }
                    }
                    st.press = None;
                    st.dragged = false;
                }
            },
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Back, .. } => st.turn(false),
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Forward, .. } => st.turn(true),
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64 * 60.0 * st.scale as f64,
                    MouseScrollDelta::PixelDelta(p) => p.y,
                };
                st.wheel(dy);
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let text = event.text.as_ref().map(|t| t.to_lowercase());
                st.key(&event.logical_key, text.as_deref());
            }
            _ => redraw = false,
        }
        if self.state.quit {
            self.state.save_position();
            el.exit();
            return;
        }
        self.sync_fullscreen();
        if redraw {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}

pub fn run(open: Option<PathBuf>) -> Result<(), String> {
    if crate::font::init().is_none() {
        return Err("フォントが見つかりません。noto-fonts-cjk をインストールするか、環境変数 KFL_FONT にフォントファイルを指定してください。".into());
    }
    let el = EventLoop::<UserEvent>::with_user_event().build().map_err(|e| e.to_string())?;
    el.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(el.create_proxy(), open);
    el.run_app(&mut app).map_err(|e| e.to_string())
}
