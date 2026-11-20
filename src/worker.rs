//! Background thread that owns the open document and renders pages, so the
//! UI thread never blocks on decoding.

use crate::canvas::Pixmap;
use crate::core::zoom::{fit_size, fit_width, scale_by};
use crate::doc::{self, LayoutParams, Meta};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

/// What to render: page `page` fitted into a `bw × bh` box (or to width `bw`
/// when `fit_width`), then zoomed by `zoom` percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub page: usize,
    pub bw: u32,
    pub bh: u32,
    pub fit_width: bool,
    pub zoom: u32,
    pub layout: u32,
}

/// Largest bitmap side we are willing to allocate.
pub const MAX_SIDE: u64 = 16384;

/// Final on-screen size of a page. Uses the Lean-verified `fit_size` /
/// `fit_width` / `scale_by`.
pub fn target_size(natural: (u32, u32), k: &Key) -> (u32, u32) {
    let (nw, nh) = (natural.0 as u64, natural.1 as u64);
    let (w, h) = if k.fit_width || k.bh == 0 { fit_width(nw, nh, k.bw as u64) } else { fit_size(nw, nh, k.bw as u64, k.bh as u64) };
    let (w, h) = (scale_by(w, k.zoom as u64), scale_by(h, k.zoom as u64));
    (w.clamp(1, MAX_SIDE) as u32, h.clamp(1, MAX_SIDE) as u32)
}

pub enum Cmd {
    Open(PathBuf, u64),
    Render(Key),
    Relayout(LayoutParams, u32),
}

pub enum Reply {
    Opened { gen: u64, meta: Meta, pages: usize, sizes: Vec<Option<(u32, u32)>> },
    OpenFailed { gen: u64, err: String },
    Rendered { gen: u64, key: Key, natural: Option<(u32, u32)>, result: Result<Pixmap, String> },
    Dropped { gen: u64, key: Key },
    Relayouted { gen: u64, layout: u32, starts: Vec<u64> },
}

pub fn spawn(send: impl Fn(Reply) + Send + 'static) -> Sender<Cmd> {
    let (tx, rx) = channel::<Cmd>();
    std::thread::Builder::new().name("render".into()).spawn(move || run(rx, send)).expect("spawn worker");
    tx
}

fn run(rx: Receiver<Cmd>, send: impl Fn(Reply)) {
    let mut doc: Option<Box<dyn doc::Document>> = None;
    let mut gen = 0u64;
    let mut queue: Vec<Key> = Vec::new();
    loop {
        // Block only when there is nothing queued.
        let first = if queue.is_empty() {
            match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => return,
            }
        } else {
            None
        };
        for cmd in first.into_iter().chain(rx.try_iter()) {
            match cmd {
                Cmd::Open(path, g) => {
                    for k in queue.drain(..) {
                        send(Reply::Dropped { gen, key: k });
                    }
                    gen = g;
                    doc = None;
                    match doc::open(&path) {
                        Ok(mut d) => {
                            let n = d.page_count();
                            let mut sizes = vec![None; n];
                            if !d.meta().reflowable && n > 0 {
                                sizes[0] = d.page_size(0);
                                // PDFs know all sizes up front; image books are measured lazily.
                                if d.meta().format == "PDF" {
                                    for (i, s) in sizes.iter_mut().enumerate() {
                                        *s = d.page_size(i);
                                    }
                                }
                            }
                            send(Reply::Opened { gen, meta: d.meta().clone(), pages: n, sizes });
                            doc = Some(d);
                        }
                        Err(err) => send(Reply::OpenFailed { gen, err }),
                    }
                }
                Cmd::Relayout(p, layout) => {
                    // Pending renders belong to the old layout.
                    for k in queue.drain(..) {
                        send(Reply::Dropped { gen, key: k });
                    }
                    if let Some(d) = doc.as_mut() {
                        let starts = d.relayout(&p);
                        send(Reply::Relayouted { gen, layout, starts });
                    }
                }
                Cmd::Render(k) => {
                    if !queue.contains(&k) {
                        queue.push(k);
                    }
                }
            }
        }
        // Newest request first: that is what the reader is looking at now.
        if let Some(k) = queue.pop() {
            let Some(d) = doc.as_mut() else {
                send(Reply::Dropped { gen, key: k });
                continue;
            };
            if k.page >= d.page_count() {
                send(Reply::Dropped { gen, key: k });
                continue;
            }
            let natural = d.page_size(k.page);
            let (w, h) = target_size(natural.unwrap_or((k.bw.max(1), k.bh.max(1))), &k);
            let result = d.render(k.page, w, h);
            send(Reply::Rendered { gen, key: k, natural, result });
            // Too many stale requests: drop the oldest ones.
            while queue.len() > 8 {
                let k = queue.remove(0);
                send(Reply::Dropped { gen, key: k });
            }
        }
    }
}

/// Thumbnail generator thread.
pub fn spawn_thumbs(send: impl Fn(PathBuf, Option<Pixmap>) + Send + 'static) -> Sender<(PathBuf, u32, u32)> {
    let (tx, rx) = channel::<(PathBuf, u32, u32)>();
    std::thread::Builder::new()
        .name("thumbs".into())
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                // Most recently requested (= currently visible) first.
                let mut batch: Vec<_> = std::iter::once(first).chain(rx.try_iter()).collect();
                while let Some((p, w, h)) = batch.pop() {
                    let t = crate::library::thumbnail(&p, w, h);
                    send(p, t);
                    batch.extend(rx.try_iter());
                }
            }
        })
        .expect("spawn thumbs");
    tx
}
