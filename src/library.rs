//! Finding books: Kindle content (Kindle for PC under Wine, a USB-connected
//! Kindle) and local documents. Also cover thumbnails.

use crate::canvas::Pixmap;
use crate::doc::{self, epub, ext, file_stem, mobi, natural_cmp, BOOK_EXTS};
use crate::store::{cache_dir, home, Config};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Item {
    /// `None` for books that are in the Kindle cloud library but not downloaded.
    pub path: Option<PathBuf>,
    pub title: String,
    pub author: String,
    pub format: String,
    pub asin: Option<String>,
    /// Must be opened in Kindle Cloud Reader (DRM / KFX / not downloaded).
    pub cloud_only: bool,
}

const KINDLE_EXTS: &[&str] = &["azw", "azw3", "mobi", "kfx", "prc", "pdoc", "azw4"];
const MAX_FILES: usize = 4000;

fn glob_dirs(base: &Path, depth: usize) -> Vec<PathBuf> {
    // Expand one level of "*" for every depth step (simple, no glob crate).
    let mut cur = vec![base.to_path_buf()];
    for _ in 0..depth {
        cur = cur
            .iter()
            .filter_map(|d| std::fs::read_dir(d).ok())
            .flat_map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect::<Vec<_>>())
            .collect();
    }
    cur
}

/// Wine prefixes that may contain Kindle for PC.
fn wine_prefixes() -> Vec<PathBuf> {
    let h = home();
    let mut v = vec![h.join(".wine")];
    if let Some(p) = std::env::var_os("WINEPREFIX") {
        v.push(PathBuf::from(p));
    }
    for base in [
        h.join(".local/share/bottles/bottles"),
        h.join(".var/app/com.usebottles.bottles/data/bottles/bottles"),
        h.join("Games"),
        h.join(".local/share/wineprefixes"),
        h.join(".PlayOnLinux/wineprefix"),
    ] {
        v.extend(glob_dirs(&base, 1));
    }
    v.into_iter().filter(|p| p.join("drive_c").is_dir()).collect()
}

/// Folders that hold downloaded Kindle books.
pub fn kindle_dirs(cfg: &Config) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for prefix in wine_prefixes() {
        for user in glob_dirs(&prefix.join("drive_c/users"), 1) {
            for docs in ["Documents", "My Documents"] {
                out.push(user.join(docs).join("My Kindle Content"));
            }
        }
    }
    for media in ["/run/media", "/media"] {
        for user in glob_dirs(Path::new(media), 1) {
            out.push(user.join("Kindle/documents"));
        }
        out.push(Path::new(media).join("Kindle/documents"));
    }
    out.push(PathBuf::from("/mnt/Kindle/documents"));
    out.push(home().join("Kindle"));
    out.extend(cfg.kindle_folders.iter().cloned());
    let mut seen = HashSet::new();
    out.into_iter().filter(|p| p.is_dir() && seen.insert(p.clone())).collect()
}

/// Kindle for PC keeps a list of every book in the account (also the ones that
/// are not downloaded) in `KindleSyncMetadataCache.xml`.
fn cloud_catalog() -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for prefix in wine_prefixes() {
        for user in glob_dirs(&prefix.join("drive_c/users"), 1) {
            for rel in ["AppData/Local/Amazon/Kindle/Cache", "Local Settings/Application Data/Amazon/Kindle/Cache"] {
                let f = user.join(rel).join("KindleSyncMetadataCache.xml");
                let Ok(xml) = std::fs::read(&f) else { continue };
                parse_catalog(&String::from_utf8_lossy(&xml), &mut out);
            }
        }
    }
    out
}

fn parse_catalog(xml: &str, out: &mut Vec<(String, String, String)>) {
    use crate::doc::markup::{tokenize, Tok};
    let (mut asin, mut title, mut author) = (String::new(), String::new(), String::new());
    let mut field = "";
    for t in tokenize(xml) {
        match t {
            Tok::Start { name, .. } => match name.as_str() {
                "meta_data" => (asin, title, author) = (String::new(), String::new(), String::new()),
                "asin" => field = "asin",
                "title" => field = "title",
                "author" => field = "author",
                _ => field = "",
            },
            Tok::Text(s) => {
                let s = s.trim();
                match field {
                    "asin" => asin = s.to_string(),
                    "title" => title = s.to_string(),
                    "author" if author.is_empty() => author = s.to_string(),
                    _ => {}
                }
                field = "";
            }
            Tok::End(name) => {
                if name == "meta_data" && !asin.is_empty() {
                    out.push((asin.clone(), title.clone(), author.clone()));
                }
                field = "";
            }
        }
    }
}

fn walk(dir: &Path, depth: usize, exts: &[&str], out: &mut Vec<PathBuf>) {
    if depth == 0 || out.len() >= MAX_FILES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.starts_with('.') || name.ends_with(".sdr") || name == "node_modules" {
            continue;
        }
        if p.is_dir() {
            walk(&p, depth - 1, exts, out);
        } else if exts.contains(&ext(&p).as_str()) {
            out.push(p);
        }
        if out.len() >= MAX_FILES {
            return;
        }
    }
}

fn kindle_item(path: PathBuf, catalog: &HashMap<String, (String, String)>) -> Item {
    let info = mobi::quick_info(&path).unwrap_or_default();
    let asin = info.asin.clone().or_else(|| mobi::asin_from_name(&path));
    let from_cat = asin.as_ref().and_then(|a| catalog.get(a));
    let mut title = info.title.clone();
    let mut author = info.author.clone();
    if title.is_empty() || info.kfx {
        if let Some((t, a)) = from_cat {
            title = t.clone();
            author = a.clone();
        }
    }
    if title.is_empty() {
        title = file_stem(&path);
    }
    let fmt = if info.kfx { "KFX" } else if info.version >= 8 { "AZW3" } else { "MOBI" };
    Item {
        cloud_only: info.drm || info.kfx || !(info.compression == 1 || info.compression == 2),
        path: Some(path),
        title,
        author,
        format: fmt.into(),
        asin,
    }
}

/// Scan for Kindle and local books. Runs on a background thread.
pub fn scan(cfg: &Config) -> (Vec<Item>, Vec<Item>) {
    let catalog_list = cloud_catalog();
    let catalog: HashMap<String, (String, String)> = catalog_list.iter().map(|(a, t, au)| (a.clone(), (t.clone(), au.clone()))).collect();

    let kdirs = kindle_dirs(cfg);
    let mut kfiles = Vec::new();
    for d in &kdirs {
        walk(d, 3, KINDLE_EXTS, &mut kfiles);
    }
    // KFX books come with sidecar files; keep only the main book file.
    kfiles.retain(|p| {
        let n = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        !n.contains(".res") && !n.contains("_sample") && !n.ends_with(".azw.md")
    });
    let mut kindle: Vec<Item> = kfiles.into_iter().map(|p| kindle_item(p, &catalog)).collect();
    let downloaded: HashSet<String> = kindle.iter().filter_map(|i| i.asin.clone()).collect();
    for (asin, title, author) in catalog_list {
        if !downloaded.contains(&asin) {
            kindle.push(Item { path: None, title, author, format: "クラウド".into(), asin: Some(asin), cloud_only: true });
        }
    }

    let mut lfiles = Vec::new();
    for d in &cfg.folders {
        walk(d, 4, BOOK_EXTS, &mut lfiles);
    }
    let mut seen = HashSet::new();
    lfiles.retain(|p| !kdirs.iter().any(|k| p.starts_with(k)) && seen.insert(p.clone()));
    let local = lfiles
        .into_iter()
        .map(|p| {
            let e = ext(&p);
            let (mut title, mut author) = (file_stem(&p), String::new());
            let mut cloud_only = false;
            let mut asin = None;
            match e.as_str() {
                "epub" => {
                    if let Some((t, a)) = epub::quick_meta(&p) {
                        if !t.is_empty() {
                            title = t;
                        }
                        author = a;
                    }
                }
                "mobi" | "azw" | "azw3" | "prc" | "pdoc" | "kfx" => {
                    let it = kindle_item(p.clone(), &catalog);
                    title = it.title;
                    author = it.author;
                    cloud_only = it.cloud_only;
                    asin = it.asin;
                }
                _ => {}
            }
            Item { path: Some(p), title, author, format: e.to_ascii_uppercase(), asin, cloud_only }
        })
        .collect();
    (kindle, local)
}

// ---------------------------------------------------------------- thumbnails

fn thumb_cache_path(path: &Path, w: u32, hh: u32) -> Option<PathBuf> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    mtime.hash(&mut h);
    meta.len().hash(&mut h);
    (w, hh).hash(&mut h);
    Some(cache_dir().join("thumbs").join(format!("{:016x}.thumb", h.finish())))
}

fn cover_bytes(path: &Path) -> Option<Vec<u8>> {
    match ext(path).as_str() {
        "epub" => epub::quick_cover(path),
        "mobi" | "azw" | "azw3" | "prc" | "pdoc" | "azw4" => mobi::quick_cover(path),
        "txt" | "kfx" => None,
        _ => doc::open(path).ok()?.cover(),
    }
}

/// Make (or load from the disk cache) a thumbnail no larger than `w × h`.
pub fn thumbnail(path: &Path, w: u32, h: u32) -> Option<Pixmap> {
    let cache = thumb_cache_path(path, w, h);
    if let Some(c) = &cache {
        if let Ok(b) = std::fs::read(c) {
            if b.len() >= 8 {
                let tw = u32::from_le_bytes(b[0..4].try_into().ok()?);
                let th = u32::from_le_bytes(b[4..8].try_into().ok()?);
                if tw <= w && th <= h && b.len() == 8 + (tw * th * 4) as usize {
                    let px = b[8..].chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
                    return Some(Pixmap { w: tw, h: th, px });
                }
            }
        }
    }
    let bytes = cover_bytes(path)?;
    let img = image::load_from_memory(&bytes).ok()?;
    let (fw, fh) = crate::core::zoom::fit_size(img.width() as u64, img.height() as u64, w as u64, h as u64);
    // `fitSize_le` (Lean) guarantees the result fits inside the card.
    let pm = doc::scale_image(img, (fw as u32).max(1), (fh as u32).max(1));
    if let Some(c) = cache {
        let _ = std::fs::create_dir_all(c.parent().unwrap());
        let mut out = Vec::with_capacity(8 + pm.px.len() * 4);
        out.extend(pm.w.to_le_bytes());
        out.extend(pm.h.to_le_bytes());
        for p in &pm.px {
            out.extend(p.to_le_bytes());
        }
        let _ = std::fs::write(c, out);
    }
    Some(pm)
}

#[cfg(test)]
mod tests {
    #[test]
    fn catalog() {
        let xml = r#"<response><add_update_list><meta_data><ASIN>B0TEST0001</ASIN><title pronunciation="x">吾輩は猫である</title><authors><author pronunciation="y">夏目漱石</author></authors></meta_data></add_update_list></response>"#;
        let mut out = vec![];
        super::parse_catalog(xml, &mut out);
        assert_eq!(out, vec![("B0TEST0001".to_string(), "吾輩は猫である".to_string(), "夏目漱石".to_string())]);
    }
}
