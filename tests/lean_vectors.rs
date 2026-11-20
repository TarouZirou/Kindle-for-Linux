//! Replays every vector produced by the Lean model (`proof/Vectors.lean`)
//! against the Rust implementation.

use kindle_for_linux::core::{layout, layout::Mode, progress, scroll, zoom};

fn nums(s: &str) -> Vec<u64> {
    if s == "-" {
        vec![]
    } else {
        s.split(',').map(|x| x.parse().unwrap()).collect()
    }
}

#[test]
fn rust_core_matches_lean_model() {
    let text = include_str!("vectors.txt");
    let mut checked = 0;
    for (lineno, line) in text.lines().enumerate() {
        let f: Vec<&str> = line.split(' ').collect();
        let u = |i: usize| -> u64 { f[i].parse().unwrap() };
        let ctx = format!("line {}: {line}", lineno + 1);
        match f[0] {
            "L" => {
                let (m, c, n, p) = (Mode::from_id(u(1)), u(2) == 1, u(3), u(4));
                assert_eq!(layout::spread_of(c, p), u(5), "{ctx}");
                assert_eq!(layout::spread_count(c, n), u(6), "{ctx}");
                assert_eq!(layout::normalize(m, c, n, p), u(7), "{ctx}");
                assert_eq!(layout::next_page(m, c, n, p), u(8), "{ctx}");
                assert_eq!(layout::prev_page(m, c, n, p), u(9), "{ctx}");
                assert_eq!(layout::last_visible(m, c, n, p), u(10), "{ctx}");
                assert_eq!(layout::visible(m, c, n, p), nums(f[11]), "{ctx}");
                assert_eq!(layout::display_order(m, c, n, p), nums(f[12]), "{ctx}");
            }
            "P" => assert_eq!(layout::page_for_permille(u(1), u(2)), u(3), "{ctx}"),
            "Z" => {
                let z = u(1);
                assert_eq!(zoom::clamp_zoom(z), u(2), "{ctx}");
                assert_eq!(zoom::zoom_in(z), u(3), "{ctx}");
                assert_eq!(zoom::zoom_out(z), u(4), "{ctx}");
                assert_eq!(zoom::scale_by(1234, z), u(5), "{ctx}");
            }
            "F" => {
                assert_eq!(zoom::fit_size(u(1), u(2), u(3), u(4)), (u(5), u(6)), "{ctx}");
                assert_eq!(zoom::fit_width(u(1), u(2), u(3)), (u(7), u(8)), "{ctx}");
            }
            "S" => {
                assert_eq!(scroll::clamp_offset(u(1), u(2), u(3)), u(4), "{ctx}");
                assert_eq!(scroll::center_pad(u(2), u(3)), u(5), "{ctx}");
            }
            "ST" => {
                let hs = nums(f[1]);
                assert_eq!(scroll::starts(&hs, u(2), 0), nums(f[3]), "{ctx}");
                assert_eq!(scroll::total_height(&hs, u(2)), u(4), "{ctx}");
            }
            "LOC" => assert_eq!(scroll::locate(&nums(f[1]), u(2)), u(3), "{ctx}"),
            "PCT" => {
                let (r, n, est) = (u(1), u(2), u(3));
                assert_eq!(progress::percent(r, n), u(4), "{ctx}");
                assert_eq!(progress::remaining_pages(r, n), u(5), "{ctx}");
                assert_eq!(progress::remaining_ms(r, n, est), u(6), "{ctx}");
            }
            "SPD" => assert_eq!(progress::update_speed(u(1), u(2), u(3)), u(4), "{ctx}"),
            "HM" => {
                assert_eq!(progress::hours_minutes(u(1)), (u(2), u(3)), "{ctx}");
                assert_eq!(progress::clamp_speed(u(1)), u(4), "{ctx}");
            }
            other => panic!("unknown vector kind {other} ({ctx})"),
        }
        checked += 1;
    }
    assert!(checked > 8000, "too few vectors: {checked}");
}
