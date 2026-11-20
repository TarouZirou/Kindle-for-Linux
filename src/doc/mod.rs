//! Document formats. Every format renders pages to [`Pixmap`]s on the worker
//! thread; the UI only ever blits finished bitmaps.

pub mod epub;
pub mod images;
pub mod markup;
pub mod mobi;
pub mod pdf;
pub mod text;

use crate::canvas::Pixmap;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub title: String,
    pub author: String,
    pub format: &'static str,
    /// Book reads right-to-left (manga, Japanese vertical text).
    pub rtl: bool,
    /// Prefer vertical (縦書き) text layout.
    pub vertical_text: bool,
    /// Text that is re-flowed to the window size (TXT, text EPUB/MOBI).
    pub reflowable: bool,
    pub asin: Option<String>,
    /// Protected by DRM: can only be read in Kindle Cloud Reader.
    pub drm: bool,
    /// Supported for reading in this app.
    pub readable: bool,
    pub note: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutParams {
    pub w: u32,
    pub h: u32,
    pub font_px: f32,
    pub vertical: bool,
    pub bg: u32,
    pub fg: u32,
}

pub trait Document: Send {
    fn meta(&self) -> &Meta;
    fn page_count(&self) -> usize;
    /// Natural size of a page (pixels), if known.
    fn page_size(&mut self, i: usize) -> Option<(u32, u32)>;
    /// Render page `i` scaled to exactly `w × h`.
    fn render(&mut self, i: usize, w: u32, h: u32) -> Result<Pixmap, String>;
    /// Re-flow text for a new page size / font. Returns the starting
    /// character offset of every page (ascending, first is 0).
    fn relayout(&mut self, _p: &LayoutParams) -> Vec<u64> {
        Vec::new()
    }
    /// Cover image (encoded bytes) for thumbnails.
    fn cover(&mut self) -> Option<Vec<u8>> {
        None
    }
}

pub fn ext(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

pub const BOOK_EXTS: &[&str] = &["epub", "pdf", "cbz", "zip", "txt", "mobi", "azw", "azw3", "prc", "kfx", "pdoc"];
pub const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp"];

pub fn open(path: &Path) -> Result<Box<dyn Document>, String> {
    if path.is_dir() {
        return images::ImageDoc::open_dir(path).map(|d| Box::new(d) as Box<dyn Document>);
    }
    match ext(path).as_str() {
        "cbz" | "zip" => images::ImageDoc::open_zip(path).map(|d| Box::new(d) as _),
        "epub" => epub::open(path),
        "pdf" => pdf::PdfDoc::open(path).map(|d| Box::new(d) as _),
        "txt" | "text" => text::TextDoc::open_txt(path).map(|d| Box::new(d) as _),
        "mobi" | "azw" | "azw3" | "prc" | "pdoc" | "kfx" => mobi::open(path),
        e if IMAGE_EXTS.contains(&e) => images::ImageDoc::open_dir(path.parent().unwrap_or(Path::new("."))).map(|d| Box::new(d) as _),
        e => Err(format!("対応していない形式です: .{e}")),
    }
}

/// Decode encoded image bytes and scale to exactly `w × h`.
pub fn decode_scaled(bytes: &[u8], w: u32, h: u32) -> Result<Pixmap, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("画像を読めません: {e}"))?;
    Ok(scale_image(img, w, h))
}

pub fn scale_image(img: image::DynamicImage, w: u32, h: u32) -> Pixmap {
    let (w, h) = (w.max(1), h.max(1));
    let scaled = if img.width() == w && img.height() == h {
        img
    } else if img.width() >= w * 2 {
        img.thumbnail_exact(w, h)
    } else {
        img.resize_exact(w, h, image::imageops::FilterType::Triangle)
    };
    let rgba = scaled.to_rgba8();
    Pixmap::from_rgba(rgba.width(), rgba.height(), rgba.as_raw())
}

pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()
}

/// "page2" < "page10".
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let na = a.iter().take_while(|c| c.is_ascii_digit()).count();
                let nb = b.iter().take_while(|c| c.is_ascii_digit()).count();
                let (da, db) = (&a[..na], &b[..nb]);
                let ta = std::str::from_utf8(da).unwrap().trim_start_matches('0');
                let tb = std::str::from_utf8(db).unwrap().trim_start_matches('0');
                let ord = ta.len().cmp(&tb.len()).then(ta.cmp(tb));
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
                a = &a[na..];
                b = &b[nb..];
            }
            (Some(x), Some(y)) => {
                let ord = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

pub fn file_stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn natural() {
        let mut v = vec!["p10.jpg", "p2.jpg", "P1.jpg", "p02b.jpg"];
        v.sort_by(|a, b| super::natural_cmp(a, b));
        assert_eq!(v, vec!["P1.jpg", "p2.jpg", "p02b.jpg", "p10.jpg"]);
    }
}
