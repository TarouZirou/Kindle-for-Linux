//! Font loading (memory-mapped, so a 20 MB CJK font costs almost no RSS) and
//! a small glyph-bitmap cache.

use ab_glyph::{Font as _, FontRef, PxScale, ScaleFont};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

static FONT: OnceLock<FontRef<'static>> = OnceLock::new();

const CANDIDATES: &[&str] = &[
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSerifCJK-Regular.ttc",
    "/usr/share/fonts/OTF/ipag.ttf",
    "/usr/share/fonts/TTF/ipag.ttf",
    "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
    "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

fn candidates() -> Vec<(PathBuf, u32)> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("KFL_FONT") {
        out.push((PathBuf::from(p), 0));
    }
    for pattern in ["sans-serif:lang=ja", "sans-serif"] {
        if let Ok(o) = Command::new("fc-match").args(["-f", "%{file}\n%{index}", pattern]).output() {
            let s = String::from_utf8_lossy(&o.stdout);
            let mut it = s.lines();
            if let Some(file) = it.next().filter(|f| !f.is_empty()) {
                let idx = it.next().and_then(|i| i.trim().parse().ok()).unwrap_or(0);
                out.push((PathBuf::from(file), idx));
            }
        }
    }
    out.extend(CANDIDATES.iter().map(|p| (PathBuf::from(p), 0)));
    out
}

/// Load the UI/text font once. Returns `None` if no usable font exists.
pub fn init() -> Option<&'static FontRef<'static>> {
    if let Some(f) = FONT.get() {
        return Some(f);
    }
    for (path, index) in candidates() {
        let Ok(file) = std::fs::File::open(&path) else { continue };
        // SAFETY: font files are not modified while we run; the map is leaked
        // so the font can be shared between threads for the program lifetime.
        let Ok(map) = (unsafe { memmap2::Mmap::map(&file) }) else { continue };
        let bytes: &'static [u8] = Box::leak(Box::new(map));
        if let Ok(font) = FontRef::try_from_slice_and_index(bytes, index) {
            let _ = FONT.set(font);
            return FONT.get();
        }
    }
    None
}

pub fn font() -> &'static FontRef<'static> {
    init().expect("no usable font found (install noto-fonts-cjk or set KFL_FONT)")
}

/// A rasterised glyph: 8-bit coverage, positioned relative to the pen.
pub struct Glyph {
    pub w: u32,
    pub h: u32,
    /// Offset of the bitmap's top-left from the pen position (baseline).
    pub left: i32,
    pub top: i32,
    pub advance: f32,
    pub coverage: Vec<u8>,
}

/// Characters drawn rotated 90° in vertical text.
pub fn rotate_in_vertical(c: char) -> bool {
    matches!(c, 'ー' | '～' | '〜' | '－' | '＝' | '→' | '←' | '-' | '=' | '~' | '―' | '─')
        || (c.is_ascii() && !c.is_ascii_whitespace() && !c.is_ascii_digit())
        || ('\u{FF01}'..='\u{FF5E}').contains(&c) && !is_fullwidth_upright(c)
}

fn is_fullwidth_upright(c: char) -> bool {
    // Full-width letters/digits stay upright; full-width brackets rotate.
    c.is_alphanumeric() || matches!(c, '！' | '？' | '％' | '＆' | '＠' | '＃' | '＊')
}

/// Map characters to their vertical presentation forms where they exist.
pub fn vertical_form(c: char) -> char {
    match c {
        '、' => '︑',
        '。' => '︒',
        '，' => '︐',
        '「' => '﹁',
        '」' => '﹂',
        '『' => '﹃',
        '』' => '﹄',
        '（' | '(' => '︵',
        '）' | ')' => '︶',
        '【' => '︻',
        '】' => '︼',
        '〔' => '︹',
        '〕' => '︺',
        '｛' => '︷',
        '｝' => '︸',
        '〈' => '︿',
        '〉' => '﹀',
        '《' => '︽',
        '》' => '︾',
        '［' => '﹇',
        '］' => '﹈',
        '…' => '︙',
        '‥' => '︰',
        '—' => '︱',
        _ => c,
    }
}

#[derive(Default)]
pub struct GlyphCache {
    map: HashMap<(char, u32, bool), Glyph>,
}

impl GlyphCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ascent(&self, px: f32) -> f32 {
        font().as_scaled(PxScale::from(px)).ascent()
    }

    pub fn descent(&self, px: f32) -> f32 {
        font().as_scaled(PxScale::from(px)).descent()
    }

    pub fn has_glyph(c: char) -> bool {
        font().glyph_id(c).0 != 0
    }

    pub fn advance(&mut self, c: char, px: f32) -> f32 {
        let f = font().as_scaled(PxScale::from(px));
        f.h_advance(f.glyph_id(c))
    }

    /// Rasterise (and cache) a glyph. `rotated` turns it 90° clockwise for
    /// vertical text.
    pub fn get(&mut self, c: char, px: f32, rotated: bool) -> &Glyph {
        let key = (c, (px * 4.0) as u32, rotated);
        if self.map.len() > 6000 {
            self.map.clear();
        }
        self.map.entry(key).or_insert_with(|| rasterize(c, px, rotated))
    }

    pub fn measure(&mut self, text: &str, px: f32) -> f32 {
        text.chars().map(|c| self.advance(c, px)).sum()
    }
}

fn rasterize(c: char, px: f32, rotated: bool) -> Glyph {
    let font = font();
    let scale = PxScale::from(px);
    let sf = font.as_scaled(scale);
    let id = font.glyph_id(c);
    let advance = sf.h_advance(id);
    let glyph = id.with_scale_and_position(scale, ab_glyph::point(0.0, 0.0));
    let Some(outline) = font.outline_glyph(glyph) else {
        return Glyph { w: 0, h: 0, left: 0, top: 0, advance, coverage: vec![] };
    };
    let b = outline.px_bounds();
    let (w, h) = (b.width().ceil().max(0.0) as u32, b.height().ceil().max(0.0) as u32);
    let mut cov = vec![0u8; (w * h) as usize];
    outline.draw(|x, y, v| {
        if x < w && y < h {
            cov[(y * w + x) as usize] = (v.clamp(0.0, 1.0) * 255.0) as u8;
        }
    });
    let (left, top) = (b.min.x.floor() as i32, b.min.y.floor() as i32);
    if !rotated {
        return Glyph { w, h, left, top, advance, coverage: cov };
    }
    // Rotate 90° clockwise around the pen; the caller centres it in the column.
    let mut rot = vec![0u8; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = (h - 1 - y, x);
            rot[(ny * h + nx) as usize] = cov[(y * w + x) as usize];
        }
    }
    // (x, y) ↦ (−y, x): the baseline becomes a vertical line through the pen,
    // and the pen now advances downwards.
    Glyph { w: h, h: w, left: -(top + h as i32), top: left, advance, coverage: rot }
}
