//! A tiny, forgiving XML/HTML tokenizer — just enough for OPF files and
//! book XHTML/MOBI markup — plus HTML → paragraph extraction.

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Start { name: String, attrs: Vec<(String, String)>, closed: bool },
    End(String),
    Text(String),
}

/// Local part of a (possibly namespaced) name: `dc:title` → `title`.
pub fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

pub fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == key || local(k) == key).map(|(_, v)| v.as_str())
}

pub fn tokenize(s: &str) -> Vec<Tok> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            let end = s[i..].find('<').map(|p| i + p).unwrap_or(b.len());
            out.push(Tok::Text(decode_entities(&s[i..end])));
            i = end;
            continue;
        }
        let rest = &s[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").map(|p| p + 3).unwrap_or(rest.len());
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").unwrap_or(rest.len());
            out.push(Tok::Text(rest[9..end.max(9)].to_string()));
            i += (end + 3).min(rest.len());
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            i += rest.find('>').map(|p| p + 1).unwrap_or(rest.len());
            continue;
        }
        // Find the closing '>' while respecting quotes.
        let mut j = 1;
        let mut quote = 0u8;
        let rb = rest.as_bytes();
        while j < rb.len() {
            let c = rb[j];
            if quote != 0 {
                if c == quote {
                    quote = 0;
                }
            } else if c == b'"' || c == b'\'' {
                quote = c;
            } else if c == b'>' {
                break;
            }
            j += 1;
        }
        let inner = &rest[1..j.min(rest.len())];
        i += (j + 1).min(rest.len());
        if let Some(name) = inner.strip_prefix('/') {
            out.push(Tok::End(name.trim().to_ascii_lowercase()));
            continue;
        }
        let closed = inner.trim_end().ends_with('/');
        let inner = inner.trim_end().trim_end_matches('/');
        let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
        let name = inner[..name_end].to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        out.push(Tok::Start { name, attrs: parse_attrs(&inner[name_end..]), closed });
    }
    out
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let ks = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' {
            i += 1;
        }
        if ks == i {
            i += 1;
            continue;
        }
        let key = s[ks..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            let val = if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let vs = i + 1;
                i = vs;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                let v = &s[vs..i.min(s.len())];
                i += 1;
                v
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                &s[vs..i]
            };
            out.push((key, decode_entities(val)));
        } else {
            out.push((key, String::new()));
        }
    }
    out
}

pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        // Entity names are ASCII, so searching bytes is boundary-safe.
        let end = rest.as_bytes().iter().take(12).position(|&c| c == b';');
        let decoded = end.and_then(|e| {
            let ent = &rest[1..e];
            let c = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                "hellip" => Some('…'),
                "mdash" => Some('—'),
                "ndash" => Some('–'),
                "rsquo" => Some('’'),
                "lsquo" => Some('‘'),
                "rdquo" => Some('”'),
                "ldquo" => Some('“'),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                    u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32)
                }
                _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
                _ => None,
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Para(String),
    Heading(String),
    PageBreak,
}

impl Block {
    pub fn text(&self) -> &str {
        match self {
            Block::Para(s) | Block::Heading(s) => s,
            Block::PageBreak => "",
        }
    }
}

const SKIP: &[&str] = &["head", "script", "style", "rt", "rp", "title", "svg", "nav"];
const BLOCK: &[&str] = &[
    "p", "div", "br", "li", "tr", "blockquote", "section", "article", "dt", "dd", "pre", "hr", "table", "ul",
    "ol", "body", "h1", "h2", "h3", "h4", "h5", "h6", "aside", "figure", "figcaption",
];

/// Extract readable paragraphs from (X)HTML.
pub fn html_to_blocks(html: &str, out: &mut Vec<Block>) {
    let mut skip = 0usize;
    let mut cur = String::new();
    let mut heading = false;
    let mut pre = false;
    let flush = |cur: &mut String, heading: bool, out: &mut Vec<Block>| {
        let t = cur.trim().to_string();
        cur.clear();
        if !t.is_empty() {
            out.push(if heading { Block::Heading(t) } else { Block::Para(t) });
        }
    };
    for tok in tokenize(html) {
        match tok {
            Tok::Start { name, closed, .. } => {
                let n = local(&name);
                if SKIP.contains(&n) {
                    if !closed {
                        skip += 1;
                    }
                    continue;
                }
                if n == "pagebreak" {
                    flush(&mut cur, heading, out);
                    if !matches!(out.last(), None | Some(Block::PageBreak)) {
                        out.push(Block::PageBreak);
                    }
                    continue;
                }
                if BLOCK.contains(&n) {
                    flush(&mut cur, heading, out);
                    heading = n.len() == 2 && n.starts_with('h') && n != "hr";
                    pre = n == "pre";
                }
            }
            Tok::End(name) => {
                let n = local(&name);
                if SKIP.contains(&n) {
                    skip = skip.saturating_sub(1);
                    continue;
                }
                if BLOCK.contains(&n) {
                    flush(&mut cur, heading, out);
                    heading = false;
                    pre = false;
                }
            }
            Tok::Text(t) if skip == 0 => {
                if pre {
                    for (i, line) in t.split('\n').enumerate() {
                        if i > 0 {
                            flush(&mut cur, false, out);
                        }
                        cur.push_str(line);
                    }
                    continue;
                }
                for c in t.chars() {
                    if c.is_ascii_whitespace() {
                        if !cur.is_empty() && !cur.ends_with(' ') {
                            cur.push(' ');
                        }
                    } else {
                        cur.push(c);
                    }
                }
            }
            Tok::Text(_) => {}
        }
    }
    flush(&mut cur, heading, out);
}

/// Strip Aozora Bunko markup: ruby 《…》, ruby start ｜ and notes ［＃…］.
pub fn strip_aozora(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut depth_ruby = 0;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '《' => depth_ruby += 1,
            '》' if depth_ruby > 0 => depth_ruby -= 1,
            '｜' => {}
            '［' if chars.peek() == Some(&'＃') => {
                for d in chars.by_ref() {
                    if d == '］' {
                        break;
                    }
                }
            }
            _ if depth_ruby > 0 => {}
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_and_ruby() {
        let mut b = Vec::new();
        html_to_blocks(
            "<html><head><title>x</title></head><body><h1>第一章</h1><p>吾輩は<ruby>猫<rt>ねこ</rt></ruby>である。</p><p>名前は&amp;まだ無い。</p></body></html>",
            &mut b,
        );
        assert_eq!(
            b,
            vec![Block::Heading("第一章".into()), Block::Para("吾輩は猫である。".into()), Block::Para("名前は&まだ無い。".into())]
        );
    }

    #[test]
    fn aozora() {
        assert_eq!(strip_aozora("｜吾輩《わがはい》は猫である［＃「猫」に傍点］。"), "吾輩は猫である。");
    }

    #[test]
    fn attrs() {
        let t = tokenize(r#"<item id="c" href="a%20b.xhtml" media-type='image/jpeg' properties=cover-image/>"#);
        match &t[0] {
            Tok::Start { attrs, closed, .. } => {
                assert!(closed);
                assert_eq!(attr(attrs, "properties"), Some("cover-image"));
                assert_eq!(attr(attrs, "media-type"), Some("image/jpeg"));
            }
            _ => panic!(),
        }
    }
}
