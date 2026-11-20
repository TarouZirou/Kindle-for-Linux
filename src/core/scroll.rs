//! Scrolling / panning / position lookup. Transcription of `proof/KindleCore/Scroll.lean`.

pub fn clamp_offset(off: u64, content: u64, view: u64) -> u64 {
    off.min(content.saturating_sub(view))
}

pub fn center_pad(content: u64, view: u64) -> u64 {
    view.saturating_sub(content) / 2
}

pub fn starts(hs: &[u64], gap: u64, mut acc: u64) -> Vec<u64> {
    let mut out = Vec::with_capacity(hs.len());
    for &h in hs {
        out.push(acc);
        acc = acc + h + gap;
    }
    out
}

pub fn total_height(hs: &[u64], gap: u64) -> u64 {
    hs.iter().sum::<u64>() + gap * (hs.len() as u64).saturating_sub(1)
}

pub fn locate(ss: &[u64], x: u64) -> u64 {
    (ss.iter().filter(|&&s| s <= x).count() as u64).saturating_sub(1)
}
