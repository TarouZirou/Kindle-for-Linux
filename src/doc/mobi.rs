//! Kindle MOBI / AZW / AZW3 (PalmDB) files.
//!
//! * Metadata (title, author, ASIN, cover) is read from every file, including
//!   DRM-protected ones (the headers are not encrypted).
//! * Text is shown for DRM-free files using PalmDOC or no compression.
//! * DRM-protected books and KFX files are **not** decrypted; the app offers
//!   to open them in Kindle Cloud Reader instead.

use super::images::ImageDoc;
use super::markup::{html_to_blocks, Block};
use super::text::TextDoc;
use super::{file_stem, Document, Meta};
use std::io::Read;
use std::path::Path;

fn u16be(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32be(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

#[derive(Debug, Default, Clone)]
pub struct Info {
    pub title: String,
    pub author: String,
    pub asin: Option<String>,
    pub cdetype: String,
    pub language: String,
    pub rtl: bool,
    pub drm: bool,
    pub kfx: bool,
    pub compression: u16,
    pub encoding: u32,
    pub text_records: usize,
    pub version: u32,
    pub first_image: u32,
    pub cover: Option<u32>,
    pub fdst: Option<u32>,
    pub extra_flags: u16,
}

/// ASIN embedded in a Kindle file/folder name such as `B00ABCDEFG_EBOK`.
pub fn asin_from_name(path: &Path) -> Option<String> {
    for comp in path.iter().rev().take(2) {
        let s = comp.to_string_lossy();
        let head: String = s.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        if head.len() == 10 && (head.starts_with('B') || head.chars().all(|c| c.is_ascii_digit() || c == 'X')) && head.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
            return Some(head);
        }
    }
    None
}

fn records(b: &[u8]) -> Option<Vec<usize>> {
    let n = u16be(b, 76)? as usize;
    let mut offs = Vec::with_capacity(n + 1);
    for i in 0..n {
        offs.push(u32be(b, 78 + 8 * i)? as usize);
    }
    offs.push(b.len());
    Some(offs)
}

fn record<'a>(b: &'a [u8], offs: &[usize], i: usize) -> Option<&'a [u8]> {
    let (s, e) = (*offs.get(i)?, *offs.get(i + 1)?);
    b.get(s..e.max(s))
}

/// Parse headers. `b` may be just a prefix of the file (record 0 must be in it).
pub fn parse_info(b: &[u8]) -> Option<Info> {
    if b.starts_with(b"CONT") || b.starts_with(b"\xeaDRMION") {
        return Some(Info { kfx: true, drm: b.starts_with(b"\xeaDRMION"), ..Default::default() });
    }
    if b.get(60..68)? != b"BOOKMOBI" && b.get(60..68)? != b"TEXtREAd" {
        return None;
    }
    let offs = records(b)?;
    let r0s = offs[0];
    let r0 = b.get(r0s..)?;
    let mut info = Info {
        compression: u16be(r0, 0)?,
        text_records: u16be(r0, 8)? as usize,
        drm: u16be(r0, 12)? != 0,
        ..Default::default()
    };
    info.title = String::from_utf8_lossy(&b[..32]).trim_end_matches('\0').replace('_', " ");
    if r0.get(16..20) == Some(b"MOBI") {
        let hlen = u32be(r0, 20)? as usize;
        info.encoding = u32be(r0, 28)?;
        info.version = u32be(r0, 36).unwrap_or(6);
        info.first_image = u32be(r0, 108).unwrap_or(u32::MAX);
        if hlen + 16 >= 0xE4 {
            info.extra_flags = u16be(r0, 0xF2).unwrap_or(0);
        }
        if info.version >= 8 {
            info.fdst = u32be(r0, 0xC0).filter(|&x| x != u32::MAX);
        }
        let name_off = u32be(r0, 84)? as usize;
        let name_len = u32be(r0, 88)? as usize;
        if let Some(n) = r0.get(name_off..name_off + name_len) {
            info.title = decode(n, info.encoding);
        }
        let exth_flags = u32be(r0, 128).unwrap_or(0);
        if exth_flags & 0x40 != 0 {
            let e = 16 + hlen;
            if r0.get(e..e + 4) == Some(b"EXTH") {
                let count = u32be(r0, e + 8)? as usize;
                let mut p = e + 12;
                for _ in 0..count {
                    let (Some(t), Some(l)) = (u32be(r0, p), u32be(r0, p + 4)) else { break };
                    let l = l as usize;
                    if l < 8 {
                        break;
                    }
                    let Some(d) = r0.get(p + 8..p + l) else { break };
                    let s = || decode(d, info.encoding).trim().to_string();
                    match t {
                        100 => {
                            if info.author.is_empty() {
                                info.author = s()
                            } else {
                                info.author = format!("{}, {}", info.author, s())
                            }
                        }
                        113 | 504 if info.asin.is_none() && !s().is_empty() => info.asin = Some(s()),
                        201 => info.cover = u32be(d, 0),
                        501 => info.cdetype = s(),
                        503 => info.title = s(),
                        524 => info.language = s(),
                        527 => info.rtl = s() == "rtl",
                        _ => {}
                    }
                    p += l;
                }
            }
        }
    }
    Some(info)
}

fn decode(b: &[u8], encoding: u32) -> String {
    if encoding == 1252 {
        encoding_rs::WINDOWS_1252.decode(b).0.into_owned()
    } else {
        String::from_utf8_lossy(b).into_owned()
    }
}

/// PalmDOC LZ77 decompression.
pub fn palmdoc(input: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(input.len() * 2);
    let mut i = 0;
    while i < input.len() {
        let c = input[i];
        i += 1;
        match c {
            1..=8 => {
                let end = (i + c as usize).min(input.len());
                out.extend_from_slice(&input[i..end]);
                i = end;
            }
            0 | 9..=0x7f => out.push(c),
            0xc0..=0xff => {
                out.push(b' ');
                out.push(c ^ 0x80);
            }
            _ => {
                let Some(&n) = input.get(i) else { break };
                i += 1;
                let v = ((c as usize) << 8 | n as usize) & 0x3fff;
                let (dist, len) = (v >> 3, (v & 7) + 3);
                if dist == 0 || dist > out.len() {
                    break;
                }
                let start = out.len() - dist;
                for k in 0..len {
                    out.push(out[start + k]);
                }
            }
        }
    }
    out
}

fn trailing_entry(d: &[u8], end: usize) -> usize {
    let (mut result, mut bitpos, mut pos) = (0usize, 0, end);
    while pos > 0 {
        let v = d[pos - 1];
        result |= ((v & 0x7f) as usize) << bitpos;
        bitpos += 7;
        pos -= 1;
        if v & 0x80 != 0 || bitpos >= 28 {
            break;
        }
    }
    result
}

/// Size of the trailing data appended to each text record.
fn trailing_size(d: &[u8], flags: u16) -> usize {
    let mut num = 0usize;
    let mut f = flags >> 1;
    while f != 0 {
        if f & 1 != 0 {
            num += trailing_entry(d, d.len().saturating_sub(num));
        }
        f >>= 1;
    }
    if flags & 1 != 0 && num < d.len() {
        num += (d[d.len() - num - 1] & 3) as usize + 1;
    }
    num.min(d.len())
}

pub fn quick_info(path: &Path) -> Option<Info> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 256 * 1024];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    let mut info = parse_info(&buf)?;
    if info.asin.is_none() {
        info.asin = asin_from_name(path);
    }
    Some(info)
}

pub fn quick_cover(path: &Path) -> Option<Vec<u8>> {
    let b = std::fs::read(path).ok()?;
    let info = parse_info(&b)?;
    let offs = records(&b)?;
    let idx = info.first_image.checked_add(info.cover?)? as usize;
    let r = record(&b, &offs, idx)?;
    (r.starts_with(b"\xff\xd8") || r.starts_with(b"\x89PNG") || r.starts_with(b"GIF8")).then(|| r.to_vec())
}

fn locked(path: &Path, mut meta: Meta, info: &Info) -> Box<dyn Document> {
    meta.readable = false;
    meta.drm = info.drm || info.kfx;
    let why = if info.kfx {
        "KFX 形式の Kindle 本です。"
    } else if info.drm {
        "DRM で保護された Kindle 本です。"
    } else {
        "この圧縮形式 (HUFF/CDIC) には対応していません。"
    };
    meta.note = Some(format!("{why}Kindle Cloud Reader で開いてください。"));
    let _ = path;
    Box::new(TextDoc::new(meta, vec![Block::Para(format!("{why}このアプリでは表示できません。ライブラリから Kindle Cloud Reader で開いてください。"))]))
}

pub fn open(path: &Path) -> Result<Box<dyn Document>, String> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    let info = parse_info(&b).ok_or("Kindle 形式のファイルとして読めません")?;
    let mut meta = Meta {
        title: if info.title.is_empty() { file_stem(path) } else { info.title.clone() },
        author: info.author.clone(),
        format: if info.kfx { "KFX" } else if info.version >= 8 { "AZW3 (KF8)" } else { "MOBI" },
        asin: info.asin.clone().or_else(|| asin_from_name(path)),
        rtl: info.rtl,
        vertical_text: info.rtl && info.language.starts_with("ja"),
        ..Default::default()
    };
    if info.kfx || info.drm || !(info.compression == 1 || info.compression == 2) {
        return Ok(locked(path, meta, &info));
    }
    let offs = records(&b).ok_or("レコード表が壊れています")?;
    let mut text = Vec::new();
    for i in 1..=info.text_records {
        let Some(r) = record(&b, &offs, i) else { break };
        let r = &r[..r.len() - trailing_size(r, info.extra_flags)];
        if info.compression == 2 {
            text.extend(palmdoc(r));
        } else {
            text.extend_from_slice(r);
        }
    }
    // KF8: only the first flow is HTML; the rest are stylesheets etc.
    if let Some(fd) = info.fdst {
        if let Some(r) = record(&b, &offs, fd as usize) {
            if r.starts_with(b"FDST") && u32be(r, 8).unwrap_or(0) > 1 {
                if let Some(end) = u32be(r, 16) {
                    text.truncate(end as usize);
                }
            }
        }
    }
    let html = decode(&text, info.encoding);
    let mut blocks = Vec::new();
    html_to_blocks(&html, &mut blocks);
    meta.readable = true;
    let chars: usize = blocks.iter().map(|b| b.text().chars().count()).sum();
    // Image-only books (comics): show the image records as pages.
    if chars < 200 && info.first_image != u32::MAX {
        let mut pages = Vec::new();
        let mut i = info.first_image as usize;
        while let Some(r) = record(&b, &offs, i) {
            if r.starts_with(b"\xff\xd8") || r.starts_with(b"\x89PNG") || r.starts_with(b"GIF8") {
                pages.push(r.to_vec());
            } else if !pages.is_empty() && (r.starts_with(b"FLIS") || r.starts_with(b"FCIS") || r.starts_with(b"EOF")) {
                break;
            }
            i += 1;
        }
        if pages.len() > 1 {
            // The cover is usually stored first *and* as a page; keep order as-is.
            return ImageDoc::from_memory(meta, pages).map(|d| Box::new(d) as Box<dyn Document>);
        }
    }
    let cover = info.cover.and_then(|c| record(&b, &offs, (info.first_image + c) as usize)).map(|r| r.to_vec());
    Ok(Box::new(MobiText { inner: TextDoc::new(meta, blocks), cover }))
}

struct MobiText {
    inner: TextDoc,
    cover: Option<Vec<u8>>,
}

impl Document for MobiText {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palmdoc_roundtrip_literals_and_backrefs() {
        // "abcabcabc": literals "abc" then a back-reference (dist 3, len 6).
        let v: u16 = 0x8000 | (3 << 3) | (6 - 3);
        let input = [b'a', b'b', b'c', (v >> 8) as u8, v as u8];
        assert_eq!(palmdoc(&input), b"abcabcabc");
        // 0xC0+ encodes a space followed by a character.
        assert_eq!(palmdoc(&[0xC1 ^ 0x00]), vec![b' ', 0x41]);
        // A corrupt back-reference must not panic.
        assert_eq!(palmdoc(&[0x80, 0x09]), Vec::<u8>::new());
    }

    #[test]
    fn asin_names() {
        assert_eq!(asin_from_name(Path::new("/x/B01ABCDEF2_EBOK/B01ABCDEF2_EBOK.azw")).as_deref(), Some("B01ABCDEF2"));
        assert_eq!(asin_from_name(Path::new("/x/My Book.azw3")), None);
    }

    #[test]
    fn trailing() {
        // multibyte flag: last byte & 3 = 1 → 2 bytes trailing.
        assert_eq!(trailing_size(&[b'x', b'y', 0x01], 1), 2);
        assert_eq!(trailing_size(&[b'x'], 0), 0);
    }

    /// Build a minimal MOBI file in memory and read it back.
    pub fn build_mobi(title: &str, html: &str, drm: bool) -> Vec<u8> {
        let mut r0 = vec![0u8; 16];
        r0[0..2].copy_from_slice(&1u16.to_be_bytes()); // no compression
        r0[4..8].copy_from_slice(&(html.len() as u32).to_be_bytes());
        r0[8..10].copy_from_slice(&1u16.to_be_bytes());
        r0[10..12].copy_from_slice(&4096u16.to_be_bytes());
        r0[12..14].copy_from_slice(&(drm as u16).to_be_bytes());
        let mut mobi = vec![0u8; 0xE8];
        mobi[0..4].copy_from_slice(b"MOBI");
        mobi[4..8].copy_from_slice(&0xE8u32.to_be_bytes());
        mobi[12..16].copy_from_slice(&65001u32.to_be_bytes());
        mobi[20..24].copy_from_slice(&6u32.to_be_bytes());
        mobi[92..96].copy_from_slice(&u32::MAX.to_be_bytes()); // first image (108-16)
        mobi[112..116].copy_from_slice(&0x40u32.to_be_bytes()); // EXTH flag (128-16)
        let mut exth = b"EXTH".to_vec();
        let recs: Vec<(u32, &[u8])> = vec![(100, "著者".as_bytes()), (113, b"B000TEST01")];
        let body: Vec<u8> = recs.iter().flat_map(|(t, d)| [t.to_be_bytes().to_vec(), ((d.len() + 8) as u32).to_be_bytes().to_vec(), d.to_vec()].concat()).collect();
        exth.extend(((body.len() + 12) as u32).to_be_bytes());
        exth.extend((recs.len() as u32).to_be_bytes());
        exth.extend(body);
        let name_off = 16 + mobi.len() + exth.len();
        mobi[68..72].copy_from_slice(&(name_off as u32).to_be_bytes()); // 84-16
        mobi[72..76].copy_from_slice(&(title.len() as u32).to_be_bytes()); // 88-16
        r0.extend(mobi);
        r0.extend(exth);
        r0.extend(title.as_bytes());
        r0.extend([0, 0]);
        let mut f = vec![0u8; 78 + 16 + 2];
        f[0..4].copy_from_slice(b"test");
        f[60..68].copy_from_slice(b"BOOKMOBI");
        f[76..78].copy_from_slice(&2u16.to_be_bytes());
        let r0_off = f.len();
        let r1_off = r0_off + r0.len();
        f[78..82].copy_from_slice(&(r0_off as u32).to_be_bytes());
        f[86..90].copy_from_slice(&(r1_off as u32).to_be_bytes());
        f.extend(r0);
        f.extend(html.as_bytes());
        f
    }

    #[test]
    fn parse_synthetic_mobi() {
        let b = build_mobi("テスト本", "<html><body><p>こんにちは</p></body></html>", false);
        let info = parse_info(&b).unwrap();
        assert_eq!(info.title, "テスト本");
        assert_eq!(info.author, "著者");
        assert_eq!(info.asin.as_deref(), Some("B000TEST01"));
        assert!(!info.drm);
        let drm = parse_info(&build_mobi("x", "y", true)).unwrap();
        assert!(drm.drm);
    }
}
