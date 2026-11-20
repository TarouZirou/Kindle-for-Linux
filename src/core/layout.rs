//! Page layout and navigation.
//!
//! Line-by-line transcription of `proof/KindleCore/Layout.lean`.
//! Lean's `Nat` subtraction truncates at zero; here that is `saturating_sub`.
//! Every function is checked against the Lean model by `tests/lean_vectors.rs`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Single,
    SpreadLtr,
    SpreadRtl,
    Vertical,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Single, Mode::SpreadRtl, Mode::SpreadLtr, Mode::Vertical];

    pub fn is_spread(self) -> bool {
        matches!(self, Mode::SpreadLtr | Mode::SpreadRtl)
    }

    pub fn id(self) -> u64 {
        match self {
            Mode::Single => 0,
            Mode::SpreadLtr => 1,
            Mode::SpreadRtl => 2,
            Mode::Vertical => 3,
        }
    }

    pub fn from_id(id: u64) -> Mode {
        match id {
            1 => Mode::SpreadLtr,
            2 => Mode::SpreadRtl,
            3 => Mode::Vertical,
            _ => Mode::Single,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Single => "単ページ",
            Mode::SpreadRtl => "見開き (右→左)",
            Mode::SpreadLtr => "見開き (左→右)",
            Mode::Vertical => "縦スクロール",
        }
    }
}

fn shift(cover: bool) -> u64 {
    if cover {
        1
    } else {
        0
    }
}

pub fn spread_of(cover: bool, p: u64) -> u64 {
    (p + shift(cover)) / 2
}

pub fn spread_start(cover: bool, s: u64) -> u64 {
    (2 * s).saturating_sub(shift(cover))
}

pub fn spread_end(cover: bool, s: u64) -> u64 {
    (2 * s + 2).saturating_sub(shift(cover))
}

pub fn spread_count(cover: bool, n: u64) -> u64 {
    if n == 0 {
        0
    } else {
        (n + shift(cover) + 1) / 2
    }
}

pub fn spread_pages(cover: bool, n: u64, s: u64) -> Vec<u64> {
    let start = spread_start(cover, s);
    let len = spread_end(cover, s).min(n).saturating_sub(start);
    (start..start + len).collect()
}

pub fn anchor(m: Mode, cover: bool, p: u64) -> u64 {
    if m.is_spread() {
        spread_start(cover, spread_of(cover, p))
    } else {
        p
    }
}

pub fn normalize(m: Mode, cover: bool, n: u64, p: u64) -> u64 {
    anchor(m, cover, p.min(n.saturating_sub(1)))
}

pub fn next_page(m: Mode, cover: bool, n: u64, p: u64) -> u64 {
    if m.is_spread() {
        if spread_of(cover, p) + 1 < spread_count(cover, n) {
            spread_start(cover, spread_of(cover, p) + 1)
        } else {
            p
        }
    } else if p + 1 < n {
        p + 1
    } else {
        p
    }
}

pub fn prev_page(m: Mode, cover: bool, _n: u64, p: u64) -> u64 {
    if m.is_spread() {
        spread_start(cover, spread_of(cover, p).saturating_sub(1))
    } else {
        p.saturating_sub(1)
    }
}

pub fn visible(m: Mode, cover: bool, n: u64, p: u64) -> Vec<u64> {
    if m.is_spread() {
        spread_pages(cover, n, spread_of(cover, p))
    } else if p < n {
        vec![p]
    } else {
        vec![]
    }
}

pub fn display_order(m: Mode, cover: bool, n: u64, p: u64) -> Vec<u64> {
    let mut v = visible(m, cover, n, p);
    if m == Mode::SpreadRtl {
        v.reverse();
    }
    v
}

pub fn last_visible(m: Mode, cover: bool, n: u64, p: u64) -> u64 {
    if m.is_spread() {
        spread_end(cover, spread_of(cover, p)).min(n).saturating_sub(1)
    } else {
        p
    }
}

pub fn page_for_permille(n: u64, k: u64) -> u64 {
    (k.min(1000) * n / 1000).min(n.saturating_sub(1))
}
