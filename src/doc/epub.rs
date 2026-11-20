//! EPUB 2/3. Fixed-layout (manga, photo books) become image pages; everything
//! else is re-flowed as text.

use super::images::ImageDoc;
use super::markup::{attr, html_to_blocks, local, tokenize, Block, Tok};
use super::text::TextDoc;
use super::{file_stem, Document, Meta};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub struct Package {
    pub title: String,
    pub author: String,
    pub language: String,
    pub rtl: bool,
    pub fixed: bool,
    /// Spine documents (archive paths) and their media type.
    pub spine: Vec<(String, String)>,
    pub cover: Option<String>,
    pub drm: bool,
}

fn read_entry(z: &mut zip::ZipArchive<File>, name: &str) -> Option<Vec<u8>> {
    let mut f = z.by_name(name).ok()?;
    let mut buf = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn read_string(z: &mut zip::ZipArchive<File>, name: &str) -> Option<String> {
    read_entry(z, name).map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Resolve `href` relative to the directory of archive path `base`.
pub fn resolve(base: &str, href: &str) -> String {
    let href = percent_decode(href.split('#').next().unwrap_or(""));
    let mut parts: Vec<&str> = if href.starts_with('/') {
        vec![]
    } else {
        base.rsplit_once('/').map(|(d, _)| d.split('/').collect()).unwrap_or_default()
    };
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

pub fn read_package(z: &mut zip::ZipArchive<File>) -> Result<Package, String> {
    let container = read_string(z, "META-INF/container.xml").ok_or("container.xml がありません")?;
    let opf_path = tokenize(&container)
        .into_iter()
        .find_map(|t| match t {
            Tok::Start { name, attrs, .. } if local(&name) == "rootfile" => attr(&attrs, "full-path").map(String::from),
            _ => None,
        })
        .ok_or("OPF が見つかりません")?;
    let opf = read_string(z, &opf_path).ok_or("OPF を読めません")?;
    let drm = z.file_names().any(|n| n == "META-INF/encryption.xml")
        && read_string(z, "META-INF/encryption.xml").map(|e| !e.contains("http://www.idpf.org/2008/embedding") && !e.contains("http://ns.adobe.com/pdf/enc#RC")).unwrap_or(false);

    let mut pkg = Package {
        title: String::new(),
        author: String::new(),
        language: String::new(),
        rtl: false,
        fixed: false,
        spine: vec![],
        cover: None,
        drm,
    };
    let mut manifest: HashMap<String, (String, String)> = HashMap::new();
    let mut cover_id = None;
    let mut spine_ids = Vec::new();
    let mut text_target: Option<&'static str> = None;
    let mut meta_property = String::new();
    for tok in tokenize(&opf) {
        match tok {
            Tok::Start { name, attrs, closed } => match local(&name) {
                "title" if pkg.title.is_empty() && !closed => text_target = Some("title"),
                "creator" if pkg.author.is_empty() && !closed => text_target = Some("creator"),
                "language" if !closed => text_target = Some("language"),
                "meta" => {
                    if attr(&attrs, "name") == Some("cover") {
                        cover_id = attr(&attrs, "content").map(String::from);
                    }
                    if attr(&attrs, "name") == Some("fixed-layout") && attr(&attrs, "content") == Some("true") {
                        pkg.fixed = true;
                    }
                    if let Some(p) = attr(&attrs, "property") {
                        meta_property = p.to_string();
                        text_target = Some("meta");
                    }
                }
                "item" => {
                    let id = attr(&attrs, "id").unwrap_or("").to_string();
                    let href = resolve(&opf_path, attr(&attrs, "href").unwrap_or(""));
                    let mt = attr(&attrs, "media-type").unwrap_or("").to_string();
                    if attr(&attrs, "properties").is_some_and(|p| p.split_whitespace().any(|x| x == "cover-image")) {
                        pkg.cover = Some(href.clone());
                    }
                    manifest.insert(id, (href, mt));
                }
                "spine" => pkg.rtl = attr(&attrs, "page-progression-direction") == Some("rtl"),
                "itemref" => {
                    if attr(&attrs, "linear") != Some("no") {
                        if let Some(id) = attr(&attrs, "idref") {
                            spine_ids.push(id.to_string());
                        }
                    }
                }
                _ => {}
            },
            Tok::Text(t) => {
                let t = t.trim();
                match text_target.take() {
                    Some("title") => pkg.title = t.to_string(),
                    Some("creator") => pkg.author = t.to_string(),
                    Some("language") => pkg.language = t.to_string(),
                    Some("meta") => {
                        if meta_property == "rendition:layout" && t == "pre-paginated" {
                            pkg.fixed = true;
                        }
                    }
                    _ => {}
                }
            }
            Tok::End(_) => text_target = None,
        }
    }
    if pkg.cover.is_none() {
        pkg.cover = cover_id.and_then(|id| manifest.get(&id).map(|(h, _)| h.clone()));
    }
    pkg.spine = spine_ids.iter().filter_map(|id| manifest.get(id).cloned()).collect();
    if pkg.spine.is_empty() {
        return Err("EPUB に本文がありません".into());
    }
    Ok(pkg)
}

/// The image shown by an XHTML page, if the page is essentially just an image.
fn page_image(xhtml: &str, doc_path: &str) -> Option<String> {
    let mut img = None;
    let mut text_len = 0;
    let mut skip = 0;
    for t in tokenize(xhtml) {
        match t {
            Tok::Start { name, attrs, closed } => match local(&name) {
                "img" if img.is_none() => img = attr(&attrs, "src").map(|s| resolve(doc_path, s)),
                "image" if img.is_none() => img = attr(&attrs, "href").map(|s| resolve(doc_path, s)),
                "head" | "style" | "script" if !closed => skip += 1,
                _ => {}
            },
            Tok::End(name) if matches!(local(&name), "head" | "style" | "script") => skip -= 1i32.min(skip),
            Tok::Text(s) if skip == 0 => text_len += s.trim().chars().count(),
            _ => {}
        }
    }
    (text_len < 40).then_some(img).flatten()
}

pub fn open(path: &Path) -> Result<Box<dyn Document>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(file).map_err(|e| format!("EPUB を開けません: {e}"))?;
    let pkg = read_package(&mut z)?;
    let ja = pkg.language.starts_with("ja");
    let mut meta = Meta {
        title: if pkg.title.is_empty() { file_stem(path) } else { pkg.title.clone() },
        author: pkg.author.clone(),
        format: "EPUB",
        rtl: pkg.rtl,
        vertical_text: pkg.rtl && ja,
        drm: pkg.drm,
        readable: !pkg.drm,
        ..Default::default()
    };
    if pkg.drm {
        meta.note = Some("この EPUB は DRM で保護されているため表示できません".into());
        return Ok(Box::new(TextDoc::new(meta, vec![Block::Para("この EPUB は DRM で保護されているため表示できません。".into())])));
    }
    let cover = pkg.cover.as_deref().and_then(|c| read_entry(&mut z, c));

    // Decide: image book or text book. Sample a few spine documents.
    let mut images = Vec::new();
    let mut image_book = true;
    for (i, (href, mt)) in pkg.spine.iter().enumerate() {
        if mt.starts_with("image/") {
            images.push(href.clone());
            continue;
        }
        let Some(x) = read_string(&mut z, href) else { continue };
        match page_image(&x, href) {
            Some(img) => images.push(img),
            None => {
                // A text page early in a fixed-layout book (e.g. colophon) is fine.
                if !pkg.fixed || i < 3 {
                    image_book = false;
                    break;
                }
            }
        }
    }
    if image_book && !images.is_empty() {
        meta.format = "EPUB (固定レイアウト)";
        meta.vertical_text = false;
        return ImageDoc::from_zip_entries(meta, z, images, cover).map(|d| Box::new(d) as Box<dyn Document>);
    }

    let mut blocks = Vec::new();
    for (href, mt) in &pkg.spine {
        if mt.starts_with("image/") {
            continue;
        }
        if let Some(x) = read_string(&mut z, href) {
            let before = blocks.len();
            html_to_blocks(&x, &mut blocks);
            if blocks.len() > before {
                blocks.push(Block::PageBreak);
            }
        }
    }
    Ok(Box::new(EpubText { inner: TextDoc::new(meta, blocks), cover }))
}

/// Text EPUB keeps its cover for the library thumbnail.
struct EpubText {
    inner: TextDoc,
    cover: Option<Vec<u8>>,
}

impl Document for EpubText {
    fn meta(&self) -> &Meta {
        self.inner.meta()
    }
    fn page_count(&self) -> usize {
        self.inner.page_count()
    }
    fn page_size(&mut self, i: usize) -> Option<(u32, u32)> {
        self.inner.page_size(i)
    }
    fn render(&mut self, i: usize, w: u32, h: u32) -> Result<crate::canvas::Pixmap, String> {
        self.inner.render(i, w, h)
    }
    fn relayout(&mut self, p: &super::LayoutParams) -> Vec<u64> {
        self.inner.relayout(p)
    }
    fn cover(&mut self) -> Option<Vec<u8>> {
        self.cover.clone()
    }
}

/// Lightweight metadata for the library (no page decoding).
pub fn quick_meta(path: &Path) -> Option<(String, String)> {
    let mut z = zip::ZipArchive::new(File::open(path).ok()?).ok()?;
    let p = read_package(&mut z).ok()?;
    Some((p.title, p.author))
}

pub fn quick_cover(path: &Path) -> Option<Vec<u8>> {
    let mut z = zip::ZipArchive::new(File::open(path).ok()?).ok()?;
    let p = read_package(&mut z).ok()?;
    if let Some(c) = p.cover.as_deref().and_then(|c| read_entry(&mut z, c)) {
        return Some(c);
    }
    let (href, mt) = p.spine.first()?.clone();
    if mt.starts_with("image/") {
        return read_entry(&mut z, &href);
    }
    let x = read_string(&mut z, &href)?;
    let img = page_image(&x, &href)?;
    read_entry(&mut z, &img)
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn resolve_paths() {
        assert_eq!(resolve("OEBPS/content.opf", "Text/ch1.xhtml#x"), "OEBPS/Text/ch1.xhtml");
        assert_eq!(resolve("OEBPS/Text/ch1.xhtml", "../Images/a%20b.jpg"), "OEBPS/Images/a b.jpg");
        assert_eq!(resolve("content.opf", "img.png"), "img.png");
    }
}
