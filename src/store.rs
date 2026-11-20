//! Settings and reading history, stored as plain text in XDG directories.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const APP: &str = "kindle-for-linux";

fn xdg(var: &str, fallback: &str) -> PathBuf {
    let base = std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home().join(fallback));
    base.join(APP)
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}
pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config")
}
pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share")
}
pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache")
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn write_atomic(path: &Path, data: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, data).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Sepia,
    Dark,
}

impl Theme {
    pub fn next(self) -> Theme {
        match self {
            Theme::Light => Theme::Sepia,
            Theme::Sepia => Theme::Dark,
            Theme::Dark => Theme::Light,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Sepia => "sepia",
            Theme::Dark => "dark",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Theme::Light => "ライト",
            Theme::Sepia => "セピア",
            Theme::Dark => "ダーク",
        }
    }
}

pub const REGIONS: &[(&str, &str, &str)] = &[
    ("jp", "日本", "https://read.amazon.co.jp/"),
    ("us", "米国", "https://read.amazon.com/"),
    ("uk", "英国", "https://read.amazon.co.uk/"),
    ("de", "ドイツ", "https://read.amazon.de/"),
    ("fr", "フランス", "https://read.amazon.fr/"),
    ("es", "スペイン", "https://read.amazon.es/"),
    ("it", "イタリア", "https://read.amazon.it/"),
    ("ca", "カナダ", "https://read.amazon.ca/"),
    ("au", "オーストラリア", "https://read.amazon.com.au/"),
    ("in", "インド", "https://read.amazon.in/"),
    ("br", "ブラジル", "https://read.amazon.com.br/"),
    ("mx", "メキシコ", "https://read.amazon.com.mx/"),
];

pub struct Config {
    pub region: String,
    pub theme: Theme,
    /// Folders scanned for local documents.
    pub folders: Vec<PathBuf>,
    /// Extra folders containing Kindle content.
    pub kindle_folders: Vec<PathBuf>,
    /// Base text size in logical pixels.
    pub font_px: f32,
    /// Memory budget for rendered pages (MiB).
    pub cache_mb: usize,
}

impl Default for Config {
    fn default() -> Self {
        let h = home();
        let ja = std::env::var("LANG").map(|l| l.starts_with("ja")).unwrap_or(false);
        let mut folders = vec![h.join("Documents"), h.join("Books"), h.join("Downloads"), h.join("Comics"), h.join("Manga")];
        // Localised XDG folders (e.g. ~/ドキュメント, ~/ダウンロード).
        if let Ok(dirs) = std::fs::read_to_string(h.join(".config/user-dirs.dirs")) {
            for line in dirs.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    if k == "XDG_DOCUMENTS_DIR" || k == "XDG_DOWNLOAD_DIR" {
                        let v = v.trim_matches('"').replace("$HOME", &h.to_string_lossy());
                        let p = PathBuf::from(v);
                        if !folders.contains(&p) {
                            folders.push(p);
                        }
                    }
                }
            }
        }
        Config {
            region: if ja { "jp".into() } else { "us".into() },
            theme: Theme::Light,
            folders,
            kindle_folders: vec![],
            font_px: 18.0,
            cache_mb: 96,
        }
    }
}

impl Config {
    fn path() -> PathBuf {
        config_dir().join("config")
    }

    pub fn load() -> Config {
        let mut c = Config::default();
        let Ok(text) = std::fs::read_to_string(Self::path()) else { return c };
        let mut folders = Vec::new();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "region" if REGIONS.iter().any(|r| r.0 == v) => c.region = v.into(),
                "theme" => {
                    c.theme = match v {
                        "dark" => Theme::Dark,
                        "sepia" => Theme::Sepia,
                        _ => Theme::Light,
                    }
                }
                "folder" => folders.push(PathBuf::from(v)),
                "kindle_folder" => c.kindle_folders.push(PathBuf::from(v)),
                "font_px" => c.font_px = v.parse::<f32>().unwrap_or(18.0).clamp(10.0, 48.0),
                "cache_mb" => c.cache_mb = v.parse::<usize>().unwrap_or(96).clamp(16, 2048),
                _ => {}
            }
        }
        if !folders.is_empty() {
            c.folders = folders;
        }
        c
    }

    pub fn save(&self) {
        let mut s = String::from("# Kindle for Linux settings\n");
        s += &format!("region={}\ntheme={}\nfont_px={}\ncache_mb={}\n", self.region, self.theme.name(), self.font_px, self.cache_mb);
        for f in &self.folders {
            s += &format!("folder={}\n", f.display());
        }
        for f in &self.kindle_folders {
            s += &format!("kindle_folder={}\n", f.display());
        }
        write_atomic(&Self::path(), &s);
    }

    pub fn cloud_url(&self) -> &'static str {
        REGIONS.iter().find(|r| r.0 == self.region).unwrap_or(&REGIONS[0]).2
    }

    pub fn region_label(&self) -> &'static str {
        REGIONS.iter().find(|r| r.0 == self.region).unwrap_or(&REGIONS[0]).1
    }
}

#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub path: PathBuf,
    pub title: String,
    pub author: String,
    pub format: String,
    pub page: u64,
    pub pages: u64,
    /// Character offset of the current page (re-flowable books).
    pub char_off: u64,
    pub ms_per_page: u64,
    pub last_opened: u64,
    pub mode: u64,
    pub cover: bool,
    pub zoom: u64,
    pub vertical: bool,
    pub fit_width: bool,
    pub reached: u64,
}

pub struct History {
    pub entries: Vec<Entry>,
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unesc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

impl History {
    fn path() -> PathBuf {
        data_dir().join("history.tsv")
    }

    pub fn load() -> History {
        let mut entries = Vec::new();
        if let Ok(text) = std::fs::read_to_string(Self::path()) {
            for line in text.lines().skip_while(|l| l.starts_with('#')) {
                let f: Vec<String> = line.split('\t').map(unesc).collect();
                if f.len() < 15 {
                    continue;
                }
                let n = |i: usize| f[i].parse::<u64>().unwrap_or(0);
                entries.push(Entry {
                    path: PathBuf::from(&f[0]),
                    title: f[1].clone(),
                    author: f[2].clone(),
                    format: f[3].clone(),
                    page: n(4),
                    pages: n(5),
                    char_off: n(6),
                    ms_per_page: n(7),
                    last_opened: n(8),
                    mode: n(9),
                    cover: n(10) == 1,
                    zoom: n(11),
                    vertical: n(12) == 1,
                    fit_width: n(13) == 1,
                    reached: n(14),
                });
            }
        }
        entries.sort_by(|a, b| b.last_opened.cmp(&a.last_opened));
        History { entries }
    }

    pub fn save(&self) {
        let mut s = String::from("# path\ttitle\tauthor\tformat\tpage\tpages\tchar_off\tms_per_page\tlast_opened\tmode\tcover\tzoom\tvertical\tfit_width\treached\n");
        for e in self.entries.iter().take(500) {
            let fields = [
                esc(&e.path.to_string_lossy()),
                esc(&e.title),
                esc(&e.author),
                esc(&e.format),
                e.page.to_string(),
                e.pages.to_string(),
                e.char_off.to_string(),
                e.ms_per_page.to_string(),
                e.last_opened.to_string(),
                e.mode.to_string(),
                (e.cover as u8).to_string(),
                e.zoom.to_string(),
                (e.vertical as u8).to_string(),
                (e.fit_width as u8).to_string(),
                e.reached.to_string(),
            ];
            s += &fields.join("\t");
            s.push('\n');
        }
        write_atomic(&Self::path(), &s);
    }

    pub fn get(&self, path: &Path) -> Option<&Entry> {
        self.entries.iter().find(|e| e.path == path)
    }

    pub fn upsert(&mut self, e: Entry) {
        self.entries.retain(|x| x.path != e.path);
        self.entries.insert(0, e);
    }

    pub fn remove(&mut self, path: &Path) {
        self.entries.retain(|x| x.path != path);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn escaping_roundtrip() {
        let s = "a\tb\\c\nd";
        assert_eq!(super::unesc(&super::esc(s)), s);
    }
}
