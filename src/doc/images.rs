//! Image-based books: CBZ/ZIP archives, folders of images, fixed-layout EPUB
//! (manga), image-only MOBI.

use super::{decode_scaled, ext, file_stem, image_dimensions, natural_cmp, Document, Meta, IMAGE_EXTS};
use crate::canvas::Pixmap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

enum Source {
    Files(Vec<PathBuf>),
    Zip { archive: zip::ZipArchive<File>, names: Vec<String> },
    Mem(Vec<Vec<u8>>),
}

pub struct ImageDoc {
    meta: Meta,
    src: Source,
    sizes: Vec<Option<(u32, u32)>>,
    cover: Option<Vec<u8>>,
}

impl ImageDoc {
    fn new(meta: Meta, src: Source) -> Result<Self, String> {
        let n = match &src {
            Source::Files(v) => v.len(),
            Source::Zip { names, .. } => names.len(),
            Source::Mem(v) => v.len(),
        };
        if n == 0 {
            return Err("画像が見つかりませんでした".into());
        }
        Ok(ImageDoc { meta, src, sizes: vec![None; n], cover: None })
    }

    fn comic_meta(title: String, format: &'static str) -> Meta {
        // Comics archives are usually Japanese manga for this app's users;
        // the reader can switch direction from the toolbar.
        let rtl = std::env::var("LANG").map(|l| l.starts_with("ja")).unwrap_or(false);
        Meta { title, format, rtl, readable: true, ..Default::default() }
    }

    pub fn open_dir(dir: &Path) -> Result<Self, String> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| IMAGE_EXTS.contains(&ext(p).as_str()))
            .collect();
        files.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
        let title = dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Self::new(Self::comic_meta(title, "画像フォルダ"), Source::Files(files))
    }

    pub fn open_zip(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| e.to_string())?;
        let archive = zip::ZipArchive::new(file).map_err(|e| format!("ZIP を開けません: {e}"))?;
        let mut names: Vec<String> = archive
            .file_names()
            .filter(|n| !n.starts_with("__MACOSX") && IMAGE_EXTS.contains(&ext(Path::new(n)).as_str()))
            .map(String::from)
            .collect();
        names.sort_by(|a, b| natural_cmp(a, b));
        Self::new(Self::comic_meta(file_stem(path), "CBZ"), Source::Zip { archive, names })
    }

    /// Pages are given entries of an already-open archive (fixed-layout EPUB).
    pub fn from_zip_entries(meta: Meta, archive: zip::ZipArchive<File>, names: Vec<String>, cover: Option<Vec<u8>>) -> Result<Self, String> {
        let mut d = Self::new(meta, Source::Zip { archive, names })?;
        d.cover = cover;
        Ok(d)
    }

    pub fn from_memory(meta: Meta, pages: Vec<Vec<u8>>) -> Result<Self, String> {
        Self::new(meta, Source::Mem(pages))
    }

    fn bytes(&mut self, i: usize) -> Result<Vec<u8>, String> {
        match &mut self.src {
            Source::Files(v) => std::fs::read(&v[i]).map_err(|e| e.to_string()),
            Source::Zip { archive, names } => {
                let mut f = archive.by_name(&names[i]).map_err(|e| e.to_string())?;
                let mut buf = Vec::with_capacity(f.size() as usize);
                f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
                Ok(buf)
            }
            Source::Mem(v) => Ok(v[i].clone()),
        }
    }
}

impl Document for ImageDoc {
    fn meta(&self) -> &Meta {
        &self.meta
    }
    fn page_count(&self) -> usize {
        self.sizes.len()
    }
    fn page_size(&mut self, i: usize) -> Option<(u32, u32)> {
        if i >= self.sizes.len() {
            return None;
        }
        if self.sizes[i].is_none() {
            let b = self.bytes(i).ok()?;
            self.sizes[i] = image_dimensions(&b);
        }
        self.sizes[i]
    }
    fn render(&mut self, i: usize, w: u32, h: u32) -> Result<Pixmap, String> {
        let b = self.bytes(i)?;
        decode_scaled(&b, w, h)
    }
    fn cover(&mut self) -> Option<Vec<u8>> {
        if let Some(c) = &self.cover {
            return Some(c.clone());
        }
        self.bytes(0).ok()
    }
}
