//! Software drawing primitives on a `0x00RRGGBB` framebuffer. No GPU.

use crate::font::{rotate_in_vertical, vertical_form, GlyphCache};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x as f64 && py >= self.y as f64 && px < (self.x + self.w) as f64 && py < (self.y + self.h) as f64
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        Rect::new(x, y, (r - x).max(0), (b - y).max(0))
    }
    pub fn inset(&self, d: i32) -> Rect {
        Rect::new(self.x + d, self.y + d, (self.w - 2 * d).max(0), (self.h - 2 * d).max(0))
    }
}

/// An image stored in the framebuffer's native pixel format.
#[derive(Clone)]
pub struct Pixmap {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
}

impl Pixmap {
    pub fn from_rgba(w: u32, h: u32, rgba: &[u8]) -> Pixmap {
        let px = rgba
            .chunks_exact(4)
            .map(|p| {
                // Composite onto white so transparent PNGs look right.
                let a = p[3] as u32;
                let c = |v: u8| (v as u32 * a + 255 * (255 - a)) / 255;
                (c(p[0]) << 16) | (c(p[1]) << 8) | c(p[2])
            })
            .collect();
        Pixmap { w, h, px }
    }
    pub fn bytes(&self) -> usize {
        self.px.len() * 4
    }
}

pub fn rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

#[inline]
pub fn blend(dst: u32, src: u32, a: u32) -> u32 {
    if a == 0 {
        return dst;
    }
    if a >= 255 {
        return src;
    }
    let inv = 255 - a;
    let ch = |s: u32| ((((src >> s) & 0xff) * a + ((dst >> s) & 0xff) * inv) / 255) << s;
    ch(16) | ch(8) | ch(0)
}

pub struct Canvas<'a> {
    pub w: i32,
    pub h: i32,
    pub buf: &'a mut [u32],
    clip: Rect,
}

impl<'a> Canvas<'a> {
    pub fn new(buf: &'a mut [u32], w: u32, h: u32) -> Self {
        let (w, h) = (w as i32, h as i32);
        Canvas { w, h, buf, clip: Rect::new(0, 0, w, h) }
    }

    pub fn set_clip(&mut self, r: Option<Rect>) {
        let full = Rect::new(0, 0, self.w, self.h);
        self.clip = r.map(|r| r.intersect(&full)).unwrap_or(full);
    }

    pub fn fill(&mut self, r: Rect, color: u32) {
        let r = r.intersect(&self.clip);
        for y in r.y..r.bottom() {
            let row = (y * self.w) as usize;
            self.buf[row + r.x as usize..row + r.right() as usize].fill(color);
        }
    }

    pub fn fill_alpha(&mut self, r: Rect, color: u32, a: u32) {
        let r = r.intersect(&self.clip);
        for y in r.y..r.bottom() {
            let row = (y * self.w) as usize;
            for p in &mut self.buf[row + r.x as usize..row + r.right() as usize] {
                *p = blend(*p, color, a);
            }
        }
    }

    /// Rectangle with rounded corners (radius `rad`), anti-aliased.
    pub fn fill_round(&mut self, r: Rect, rad: i32, color: u32) {
        let rad = rad.min(r.w / 2).min(r.h / 2).max(0);
        if rad == 0 {
            return self.fill(r, color);
        }
        self.fill(Rect::new(r.x, r.y + rad, r.w, r.h - 2 * rad), color);
        for i in 0..rad {
            let dy = rad as f32 - i as f32 - 0.5;
            let span = (rad as f32 * rad as f32 - dy * dy).max(0.0).sqrt();
            let inset = rad as f32 - span;
            let full = inset.ceil() as i32;
            let frac = full as f32 - inset;
            for (y, _) in [(r.y + i, 0), (r.bottom() - 1 - i, 1)] {
                self.fill(Rect::new(r.x + full, y, r.w - 2 * full, 1), color);
                let a = (frac * 255.0) as u32;
                self.fill_alpha(Rect::new(r.x + full - 1, y, 1, 1), color, a);
                self.fill_alpha(Rect::new(r.right() - full, y, 1, 1), color, a);
            }
        }
    }

    pub fn stroke(&mut self, r: Rect, color: u32) {
        self.fill(Rect::new(r.x, r.y, r.w, 1), color);
        self.fill(Rect::new(r.x, r.bottom() - 1, r.w, 1), color);
        self.fill(Rect::new(r.x, r.y, 1, r.h), color);
        self.fill(Rect::new(r.right() - 1, r.y, 1, r.h), color);
    }

    pub fn blit(&mut self, img: &Pixmap, x: i32, y: i32) {
        let dst = Rect::new(x, y, img.w as i32, img.h as i32).intersect(&self.clip);
        for row in dst.y..dst.bottom() {
            let sy = (row - y) as usize;
            let sx = (dst.x - x) as usize;
            let src = &img.px[sy * img.w as usize + sx..sy * img.w as usize + sx + dst.w as usize];
            let d = (row * self.w + dst.x) as usize;
            self.buf[d..d + dst.w as usize].copy_from_slice(src);
        }
    }

    /// Blit scaled with nearest-neighbour (used only for small thumbnails).
    pub fn blit_scaled(&mut self, img: &Pixmap, r: Rect) {
        if img.w == 0 || img.h == 0 || r.w <= 0 || r.h <= 0 {
            return;
        }
        let d = r.intersect(&self.clip);
        for y in d.y..d.bottom() {
            let sy = ((y - r.y) as u64 * img.h as u64 / r.h as u64) as usize;
            for x in d.x..d.right() {
                let sx = ((x - r.x) as u64 * img.w as u64 / r.w as u64) as usize;
                self.buf[(y * self.w + x) as usize] = img.px[sy * img.w as usize + sx];
            }
        }
    }

    pub fn glyph(&mut self, gc: &mut GlyphCache, c: char, px: f32, rotated: bool, pen_x: i32, pen_y: i32, color: u32) {
        let g = gc.get(c, px, rotated);
        let (gx, gy) = (pen_x + g.left, pen_y + g.top);
        let area = Rect::new(gx, gy, g.w as i32, g.h as i32).intersect(&self.clip);
        for y in area.y..area.bottom() {
            let gyo = (y - gy) as u32;
            for x in area.x..area.right() {
                let a = g.coverage[(gyo * g.w + (x - gx) as u32) as usize] as u32;
                if a > 0 {
                    let i = (y * self.w + x) as usize;
                    self.buf[i] = blend(self.buf[i], color, a);
                }
            }
        }
    }

    /// Draw a single line of horizontal text. `y` is the top of the line box.
    /// Returns the advance width.
    pub fn text(&mut self, gc: &mut GlyphCache, s: &str, x: i32, y: i32, px: f32, color: u32) -> i32 {
        let base = y + gc.ascent(px).round() as i32;
        let mut pen = x as f32;
        for c in s.chars() {
            self.glyph(gc, c, px, false, pen.round() as i32, base, color);
            pen += gc.advance(c, px);
        }
        (pen - x as f32).round() as i32
    }

    /// Text clipped to `max_w`, with an ellipsis when it does not fit.
    pub fn text_fit(&mut self, gc: &mut GlyphCache, s: &str, x: i32, y: i32, px: f32, max_w: i32, color: u32) -> i32 {
        let t = ellipsize(gc, s, px, max_w as f32);
        self.text(gc, &t, x, y, px, color)
    }

    /// Draw one character in a vertical text column centred at `cx`;
    /// `y` is the top of its cell. Returns the cell height.
    pub fn vchar(&mut self, gc: &mut GlyphCache, c: char, cx: i32, y: i32, px: f32, color: u32) -> f32 {
        let (h, rotated, ch) = vcell(gc, c, px);
        if rotated {
            let mid = (gc.ascent(px) + gc.descent(px)) / 2.0;
            self.glyph(gc, ch, px, true, (cx as f32 - mid).round() as i32, y, color);
        } else {
            let adv = gc.advance(ch, px);
            let mut base = y as f32 + px * 0.88;
            let mut x = cx as f32 - adv / 2.0;
            // Font without vertical forms: move 、。 to the top-right of the cell.
            if matches!(ch, '、' | '。' | '，' | '．') {
                x += px * 0.6;
                base -= px * 0.6;
            }
            self.glyph(gc, ch, px, false, x.round() as i32, base.round() as i32, color);
        }
        h
    }
}

/// Height, rotation and glyph used for `c` in vertical text.
pub fn vcell(gc: &mut GlyphCache, c: char, px: f32) -> (f32, bool, char) {
    let v = vertical_form(c);
    if v != c && GlyphCache::has_glyph(v) {
        return (px, false, v);
    }
    if rotate_in_vertical(c) {
        (gc.advance(c, px), true, c)
    } else {
        (px, false, c)
    }
}

/// Wrap `s` into lines no wider than `max_w`.
pub fn wrap(gc: &mut GlyphCache, s: &str, px: f32, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut w = 0.0;
    for c in s.chars() {
        let a = gc.advance(c, px);
        if w + a > max_w && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            w = 0.0;
        }
        cur.push(c);
        w += a;
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

pub fn ellipsize(gc: &mut GlyphCache, s: &str, px: f32, max_w: f32) -> String {
    if gc.measure(s, px) <= max_w {
        return s.to_string();
    }
    let ell = gc.advance('…', px);
    let mut out = String::new();
    let mut w = 0.0;
    for c in s.chars() {
        let a = gc.advance(c, px);
        if w + a + ell > max_w {
            break;
        }
        out.push(c);
        w += a;
    }
    out.push('…');
    out
}
