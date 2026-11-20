//! Zoom levels (integer percent). Transcription of `proof/KindleCore/Zoom.lean`.

pub const ZOOM_MIN: u64 = 25;
pub const ZOOM_MAX: u64 = 800;

pub const PRESETS: [u64; 18] = [
    25, 33, 50, 67, 75, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500, 600, 800,
];

pub fn clamp_zoom(z: u64) -> u64 {
    ZOOM_MIN.max(z.min(ZOOM_MAX))
}

pub fn zoom_in(z: u64) -> u64 {
    PRESETS.iter().copied().find(|&x| z < x).unwrap_or(ZOOM_MAX)
}

pub fn zoom_out(z: u64) -> u64 {
    PRESETS.iter().rev().copied().find(|&x| x < z).unwrap_or(ZOOM_MIN)
}

pub fn scale_by(len: u64, z: u64) -> u64 {
    len * z / 100
}

pub fn fit_size(iw: u64, ih: u64, bw: u64, bh: u64) -> (u64, u64) {
    if iw == 0 || ih == 0 {
        (0, 0)
    } else if iw * bh <= bw * ih {
        (iw * bh / ih, bh)
    } else {
        (bw, ih * bw / iw)
    }
}

pub fn fit_width(iw: u64, ih: u64, bw: u64) -> (u64, u64) {
    if iw == 0 {
        (0, 0)
    } else {
        (bw, ih * bw / iw)
    }
}
