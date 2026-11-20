//! Kindle for Linux — entry point.

use std::path::PathBuf;

const HELP: &str = "\
Kindle for Linux — 超軽量な電子書籍・コミックリーダー

使い方: kindle-for-linux [オプション] [ファイル]

  ファイル            起動と同時に開く本 (EPUB / PDF / CBZ / TXT / MOBI / AZW3 / 画像フォルダ)
  --region <地域>     Kindle Cloud Reader のストア (jp us uk de fr es it ca au in br mx)
  -h, --help          このヘルプ
  -V, --version       バージョン
";

fn main() {
    let mut open = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return;
            }
            "-V" | "--version" => {
                println!("kindle-for-linux {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--region" => {
                if let Some(r) = args.next() {
                    let mut cfg = kindle_for_linux::store::Config::load();
                    if kindle_for_linux::store::REGIONS.iter().any(|x| x.0 == r) {
                        cfg.region = r;
                        cfg.save();
                    } else {
                        eprintln!("不明な地域: {r}");
                        std::process::exit(2);
                    }
                }
            }
            _ => open = Some(PathBuf::from(a)),
        }
    }
    if let Err(e) = kindle_for_linux::app::run(open) {
        eprintln!("kindle-for-linux: {e}");
        std::process::exit(1);
    }
}
