//! PDF via the system's poppler (`pdftoppm`/`pdfinfo`) or mupdf (`mutool`)
//! command-line tools. Keeping the renderer out of process keeps this
//! program small and means a broken PDF can never crash the reader.

use super::{decode_scaled, file_stem, Document, Meta};
use crate::canvas::Pixmap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, PartialEq)]
enum Backend {
    Poppler,
    Mupdf,
}

pub struct PdfDoc {
    path: PathBuf,
    meta: Meta,
    backend: Backend,
    sizes: Vec<Option<(u32, u32)>>,
}

fn run(cmd: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then_some(out.stdout)
}

impl PdfDoc {
    pub fn open(path: &Path) -> Result<Self, String> {
        let p = path.to_str().ok_or("パスが不正です")?;
        let mut meta = Meta { title: file_stem(path), format: "PDF", readable: true, ..Default::default() };
        if let Some(info) = run("pdfinfo", &[p]) {
            let info = String::from_utf8_lossy(&info);
            let mut n = 0usize;
            for line in info.lines() {
                if let Some((k, v)) = line.split_once(':') {
                    let v = v.trim();
                    match k {
                        "Title" if !v.is_empty() => meta.title = v.to_string(),
                        "Author" => meta.author = v.to_string(),
                        "Pages" => n = v.parse().unwrap_or(0),
                        _ => {}
                    }
                }
            }
            let n = n.min(crate::core::progress::MAX_PAGES as usize);
            let mut sizes = vec![None; n];
            // Page sizes (in points) for every page.
            if n > 0 {
                if let Some(out) = run("pdfinfo", &["-f", "1", "-l", &n.to_string(), p]) {
                    for line in String::from_utf8_lossy(&out).lines() {
                        let Some(rest) = line.strip_prefix("Page") else { continue };
                        let Some((num, size)) = rest.split_once("size:") else { continue };
                        let Ok(i) = num.trim().parse::<usize>() else { continue };
                        let dims: Vec<f32> = size.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                        if dims.len() >= 2 && (1..=n).contains(&i) {
                            sizes[i - 1] = Some((dims[0].round() as u32, dims[1].round() as u32));
                        }
                    }
                }
            }
            if n == 0 {
                return Err("PDF のページ数を取得できませんでした".into());
            }
            return Ok(PdfDoc { path: path.into(), meta, backend: Backend::Poppler, sizes });
        }
        if let Some(out) = run("mutool", &["pages", p]) {
            let out = String::from_utf8_lossy(&out);
            let mut sizes = Vec::new();
            for line in out.lines().filter(|l| l.contains("MediaBox")) {
                let num = |key: &str| -> f32 {
                    line.split(&format!("{key}=\"")).nth(1).and_then(|s| s.split('"').next()).and_then(|s| s.parse().ok()).unwrap_or(0.0)
                };
                let (w, h) = (num("r") - num("l"), num("t") - num("b"));
                sizes.push(Some((w.abs().round() as u32, h.abs().round() as u32)));
            }
            if sizes.is_empty() {
                return Err("PDF のページ数を取得できませんでした".into());
            }
            return Ok(PdfDoc { path: path.into(), meta, backend: Backend::Mupdf, sizes });
        }
        Err("PDF を表示するには poppler (pdftoppm) または mupdf-tools (mutool) をインストールしてください".into())
    }
}

impl Document for PdfDoc {
    fn meta(&self) -> &Meta {
        &self.meta
    }
    fn page_count(&self) -> usize {
        self.sizes.len()
    }
    fn page_size(&mut self, i: usize) -> Option<(u32, u32)> {
        self.sizes.get(i).copied().flatten().or(Some((595, 842)))
    }
    fn render(&mut self, i: usize, w: u32, h: u32) -> Result<Pixmap, String> {
        let p = self.path.to_str().ok_or("パスが不正です")?;
        let page = (i + 1).to_string();
        let (ws, hs) = (w.max(1).to_string(), h.max(1).to_string());
        let png = match self.backend {
            Backend::Poppler => run("pdftoppm", &["-f", &page, "-l", &page, "-scale-to-x", &ws, "-scale-to-y", &hs, "-png", p]),
            Backend::Mupdf => run("mutool", &["draw", "-q", "-F", "png", "-o", "-", "-w", &ws, "-h", &hs, p, &page]),
        }
        .ok_or_else(|| format!("{} ページ目を描画できませんでした", i + 1))?;
        decode_scaled(&png, w, h)
    }
    fn cover(&mut self) -> Option<Vec<u8>> {
        let p = self.path.to_str()?;
        match self.backend {
            Backend::Poppler => run("pdftoppm", &["-f", "1", "-l", "1", "-scale-to", "240", "-png", p]),
            Backend::Mupdf => run("mutool", &["draw", "-q", "-F", "png", "-o", "-", "-w", "240", p, "1"]),
        }
    }
}
