//! Re-flowable text (TXT, text EPUB, MOBI): line breaking, pagination in
//! horizontal or vertical (縦書き) writing, and page rendering.

use super::markup::{strip_aozora, Block};
use super::{file_stem, Document, LayoutParams, Meta};
use crate::canvas::{vcell, Canvas, Pixmap};
use crate::font::GlyphCache;
use std::path::Path;

/// Characters that must not start a line (行頭禁則); they hang at the end instead.
fn no_line_start(c: char) -> bool {
    matches!(
        c,
        '、' | '。' | '，' | '．' | ',' | '.' | '」' | '』' | '）' | ')' | '】' | '〕' | '］' | '｝' | '〉' | '》' | '！'
            | '？' | '!' | '?' | 'ー' | '…' | '‥' | 'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'っ' | 'ゃ' | 'ゅ' | 'ょ'
            | 'ァ' | 'ィ' | 'ゥ' | 'ェ' | 'ォ' | 'ッ' | 'ャ' | 'ュ' | 'ョ' | '・' | ':' | ';' | '：' | '；'
    )
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '\'' | '’' | '-')
}

#[derive(Clone, Copy)]
struct Line {
    block: u32,
    start: u32,
    end: u32,
    /// Top of the line (horizontal) or centre of the column (vertical).
    pos: f32,
    px: f32,
}

struct Para {
    heading: bool,
    brk: bool,
    chars: Vec<char>,
}

pub struct TextDoc {
    meta: Meta,
    paras: Vec<Para>,
    para_start: Vec<u64>,
    pages: Vec<Vec<Line>>,
    params: Option<LayoutParams>,
    gc: GlyphCache,
}

impl TextDoc {
    pub fn new(mut meta: Meta, blocks: Vec<Block>) -> Self {
        meta.reflowable = true;
        meta.readable = true;
        let mut paras = Vec::with_capacity(blocks.len());
        let mut para_start = Vec::with_capacity(blocks.len());
        let mut off = 0u64;
        for b in blocks {
            para_start.push(off);
            let p = match b {
                Block::Para(s) => Para { heading: false, brk: false, chars: s.chars().collect() },
                Block::Heading(s) => Para { heading: true, brk: false, chars: s.chars().collect() },
                Block::PageBreak => Para { heading: false, brk: true, chars: vec![] },
            };
            off += p.chars.len() as u64 + 1;
            paras.push(p);
        }
        TextDoc { meta, paras, para_start, pages: vec![vec![]], params: None, gc: GlyphCache::new() }
    }

    pub fn open_txt(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let text = decode_text(&bytes);
        let aozora = text.contains('《') || text.contains("［＃");
        let mut blocks = Vec::new();
        for line in text.lines() {
            let line = if aozora { strip_aozora(line) } else { line.to_string() };
            let t = line.trim_end();
            blocks.push(Block::Para(t.to_string()));
        }
        // Collapse runs of blank lines.
        blocks.dedup_by(|a, b| a.text().is_empty() && b.text().is_empty());
        let mut meta = Meta { title: file_stem(path), format: "TXT", ..Default::default() };
        let japanese = text.chars().take(2000).filter(|c| ('\u{3040}'..='\u{30ff}').contains(c)).count() > 20;
        if aozora && text.contains("底本") {
            let mut it = blocks.iter().map(|b| b.text()).filter(|t| !t.is_empty());
            if let (Some(t), Some(a)) = (it.next(), it.next()) {
                if t.chars().count() < 60 && a.chars().count() < 40 {
                    meta.title = t.to_string();
                    meta.author = a.to_string();
                }
            }
            // Drop the explanatory block about ruby notation.
            if let Some(s) = blocks.iter().position(|b| b.text().starts_with("-------")) {
                if let Some(e) = blocks[s + 1..].iter().position(|b| b.text().starts_with("-------")) {
                    blocks.drain(s..=s + 1 + e);
                }
            }
        }
        meta.vertical_text = japanese;
        meta.rtl = japanese;
        Ok(TextDoc::new(meta, blocks))
    }

    fn layout(&mut self, p: &LayoutParams) {
        let (w, h) = (p.w as f32, p.h as f32);
        let mx = (w * 0.07).max(16.0);
        let my = (h * 0.06).max(16.0);
        let mut pages: Vec<Vec<Line>> = Vec::new();
        let mut page: Vec<Line> = Vec::new();
        let gc = &mut self.gc;

        if !p.vertical {
            let avail = w - 2.0 * mx;
            let bottom = h - my;
            let mut y = my;
            for (bi, para) in self.paras.iter().enumerate() {
                if para.brk {
                    if !page.is_empty() {
                        pages.push(std::mem::take(&mut page));
                        y = my;
                    }
                    continue;
                }
                let px = if para.heading { p.font_px * 1.3 } else { p.font_px };
                let lh = px * 1.8;
                if para.heading && !page.is_empty() {
                    y += lh * 0.6;
                }
                if para.chars.is_empty() {
                    y += lh * 0.5;
                    continue;
                }
                let cs = &para.chars;
                let mut i = 0;
                while i < cs.len() {
                    let mut x = 0.0;
                    let mut j = i;
                    let mut last_space = None;
                    while j < cs.len() {
                        let a = gc.advance(cs[j], px);
                        if x + a > avail && j > i {
                            if no_line_start(cs[j]) {
                                j += 1; // hang punctuation
                            } else if is_word_char(cs[j]) {
                                if let Some(s) = last_space {
                                    j = s + 1;
                                }
                            }
                            break;
                        }
                        if cs[j] == ' ' {
                            last_space = Some(j);
                        }
                        x += a;
                        j += 1;
                    }
                    if y + lh > bottom && !page.is_empty() {
                        pages.push(std::mem::take(&mut page));
                        y = my;
                    }
                    page.push(Line { block: bi as u32, start: i as u32, end: j as u32, pos: y, px });
                    y += lh;
                    i = j;
                    while i < cs.len() && cs[i] == ' ' {
                        i += 1;
                    }
                }
                y += px * 0.35;
            }
        } else {
            let avail = h - 2.0 * my;
            let col = p.font_px * 1.8;
            let first = w - mx - col / 2.0;
            let mut x = first;
            for (bi, para) in self.paras.iter().enumerate() {
                if para.brk {
                    if !page.is_empty() {
                        pages.push(std::mem::take(&mut page));
                        x = first;
                    }
                    continue;
                }
                let px = if para.heading { p.font_px * 1.2 } else { p.font_px };
                if para.heading && !page.is_empty() {
                    x -= col * 0.6;
                }
                if para.chars.is_empty() {
                    x -= col * 0.5;
                    continue;
                }
                let cs = &para.chars;
                let mut i = 0;
                while i < cs.len() {
                    let mut y = 0.0;
                    let mut j = i;
                    while j < cs.len() {
                        let (a, _, _) = vcell(gc, cs[j], px);
                        if y + a > avail && j > i {
                            if no_line_start(cs[j]) {
                                j += 1;
                            }
                            break;
                        }
                        y += a;
                        j += 1;
                    }
                    if x - col / 2.0 < mx && !page.is_empty() {
                        pages.push(std::mem::take(&mut page));
                        x = first;
                    }
                    page.push(Line { block: bi as u32, start: i as u32, end: j as u32, pos: x, px });
                    x -= col;
                    i = j;
                }
            }
        }
        if !page.is_empty() || pages.is_empty() {
            pages.push(page);
        }
        pages.truncate(crate::core::progress::MAX_PAGES as usize);
        self.pages = pages;
    }

    fn page_start(&self, page: &[Line]) -> u64 {
        page.first().map(|l| self.para_start[l.block as usize] + l.start as u64).unwrap_or(0)
    }
}

impl Document for TextDoc {
    fn meta(&self) -> &Meta {
        &self.meta
    }
    fn page_count(&self) -> usize {
        self.pages.len()
    }
    fn page_size(&mut self, _i: usize) -> Option<(u32, u32)> {
        self.params.map(|p| (p.w, p.h))
    }
    fn relayout(&mut self, p: &LayoutParams) -> Vec<u64> {
        let same_flow = self.params.map(|o| (o.w, o.h, o.font_px, o.vertical) == (p.w, p.h, p.font_px, p.vertical));
        if same_flow != Some(true) {
            self.layout(p);
        }
        self.params = Some(*p);
        let mut starts: Vec<u64> = self.pages.iter().map(|pg| self.page_start(pg)).collect();
        if let Some(s) = starts.first_mut() {
            *s = 0;
        }
        // Keep the sequence ascending (the Lean `locate` lemma assumes it).
        for i in 1..starts.len() {
            starts[i] = starts[i].max(starts[i - 1]);
        }
        starts
    }
    fn render(&mut self, i: usize, w: u32, h: u32) -> Result<Pixmap, String> {
        let p = self.params.ok_or("レイアウト前です")?;
        let mut px = vec![p.bg; (w.max(1) * h.max(1)) as usize];
        let mut cv = Canvas::new(&mut px, w.max(1), h.max(1));
        let my = (p.h as f32 * 0.06).max(16.0);
        let mx = (p.w as f32 * 0.07).max(16.0);
        let Some(lines) = self.pages.get(i) else { return Ok(Pixmap { w, h, px }) };
        let gc = &mut self.gc;
        for l in lines.clone() {
            let para = &self.paras[l.block as usize];
            let chars = &para.chars[l.start as usize..l.end as usize];
            let color = if para.heading { blend_heading(p.fg, p.bg) } else { p.fg };
            if p.vertical {
                let mut y = my;
                for &c in chars {
                    y += cv.vchar(gc, c, l.pos.round() as i32, y.round() as i32, l.px, color);
                }
            } else {
                let lh = l.px * 1.8;
                let base = l.pos + (lh - (gc.ascent(l.px) - gc.descent(l.px))) / 2.0 + gc.ascent(l.px);
                let mut x = mx;
                for &c in chars {
                    cv.glyph(gc, c, l.px, false, x.round() as i32, base.round() as i32, color);
                    x += gc.advance(c, l.px);
                }
            }
        }
        Ok(Pixmap { w: w.max(1), h: h.max(1), px })
    }
}

fn blend_heading(fg: u32, bg: u32) -> u32 {
    crate::canvas::blend(fg, bg, 40)
}

/// UTF-8 (with or without BOM), else Shift_JIS, else EUC-JP.
pub fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    if bytes.starts_with(b"\xFF\xFE") || bytes.starts_with(b"\xFE\xFF") {
        let enc = if bytes[0] == 0xFF { encoding_rs::UTF_16LE } else { encoding_rs::UTF_16BE };
        return enc.decode(bytes).0.into_owned();
    }
    let (s, _, err) = encoding_rs::SHIFT_JIS.decode(bytes);
    if !err {
        return s.into_owned();
    }
    let (e, _, err2) = encoding_rs::EUC_JP.decode(bytes);
    if !err2 {
        return e.into_owned();
    }
    s.into_owned()
}
