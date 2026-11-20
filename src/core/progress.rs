//! Reading progress and time-to-finish. Transcription of `proof/KindleCore/Progress.lean`.

pub const MS_MIN: u64 = 1000;
pub const MS_MAX: u64 = 600_000;

/// Inputs are capped so that, by `remainingMs_bounded`, nothing overflows `u64`.
pub const MAX_PAGES: u64 = 1_000_000;

pub fn percent(reached: u64, n: u64) -> u64 {
    if n == 0 {
        0
    } else {
        (reached + 1).min(n) * 100 / n
    }
}

pub fn remaining_pages(reached: u64, n: u64) -> u64 {
    n.saturating_sub(reached + 1)
}

pub fn clamp_speed(ms: u64) -> u64 {
    MS_MIN.max(ms.min(MS_MAX))
}

pub fn accept_sample(elapsed_ms: u64, pages: u64) -> Option<u64> {
    if pages == 0 {
        return None;
    }
    let s = elapsed_ms / pages;
    if s < MS_MIN || MS_MAX < s {
        None
    } else {
        Some(s)
    }
}

pub fn update_speed(est: u64, elapsed_ms: u64, pages: u64) -> u64 {
    match accept_sample(elapsed_ms, pages) {
        None => est,
        Some(s) => (7 * est + 3 * s) / 10,
    }
}

pub fn remaining_ms(reached: u64, n: u64, est: u64) -> u64 {
    remaining_pages(reached, n) * est
}

pub fn ceil_minutes(ms: u64) -> u64 {
    (ms + 59_999) / 60_000
}

pub fn hours_minutes(ms: u64) -> (u64, u64) {
    (ceil_minutes(ms) / 60, ceil_minutes(ms) % 60)
}

/// 「あと 2時間15分」 style text.
pub fn format_remaining(ms: u64) -> String {
    let (h, m) = hours_minutes(ms);
    match (h, m) {
        (0, 0) => "読了".to_string(),
        (0, m) => format!("読了まであと {m}分"),
        (h, 0) => format!("読了まであと {h}時間"),
        (h, m) => format!("読了まであと {h}時間{m}分"),
    }
}
