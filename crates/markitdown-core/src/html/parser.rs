//! CPython 3.12's `html.parser.HTMLParser` (with `convert_charrefs=False`,
//! as BeautifulSoup runs it) driving bs4 4.15's tree construction.
//!
//! The scanner follows `goahead()` statement by statement, including its
//! two passes (`feed` then `close`), because malformed markup is resolved by
//! exactly where those passes stop.

use super::entities::BS4;
use super::unescape;
use crate::dom::{Dom, Kind, ROOT};
use crate::pystr;

/// bs4 HTMLTreeBuilder.empty_element_tags
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "keygen", "link", "menuitem",
    "meta", "param", "source", "track", "wbr", "basefont", "bgsound", "command", "frame", "image",
    "isindex", "nextid", "spacer",
];

pub fn parse(text: &str) -> Dom {
    let mut b = Builder::new();
    let mut p = Parser {
        raw: text.to_string(),
        cdata_elem: None,
        escapable: true,
    };
    p.goahead(false, &mut b);
    p.goahead(true, &mut b);
    b.finish()
}

/// bs4's BeautifulSoup object as a tree builder target.
struct Builder {
    dom: Dom,
    stack: Vec<usize>,
    names: Vec<String>,
    current_data: Vec<String>,
    preserve_ws_depth: usize,
    already_closed_empty: Vec<String>,
}

impl Builder {
    fn new() -> Self {
        Builder {
            dom: Dom::new("[document]"),
            stack: vec![ROOT],
            names: vec!["[document]".into()],
            current_data: Vec::new(),
            preserve_ws_depth: 0,
            already_closed_empty: Vec::new(),
        }
    }

    fn current(&self) -> usize {
        *self.stack.last().unwrap_or(&ROOT)
    }

    fn data(&mut self, s: &str) {
        self.current_data.push(s.to_string());
    }

    /// `endData(containerClass)`: is_comment selects Comment/Doctype nodes.
    fn end_data(&mut self, comment: bool) {
        if self.current_data.is_empty() {
            return;
        }
        let mut data = self.current_data.concat();
        self.current_data.clear();
        if self.preserve_ws_depth == 0
            && data
                .chars()
                .all(|c| matches!(c, ' ' | '\n' | '\t' | '\x0c' | '\r'))
        {
            data = if data.contains('\n') {
                "\n".into()
            } else {
                " ".into()
            };
        }
        let parent = self.current();
        self.dom.push(
            parent,
            if comment {
                Kind::Comment(data)
            } else {
                Kind::Text(data)
            },
        );
    }

    fn special(&mut self, data: &str, comment: bool) {
        self.end_data(false);
        self.data(data);
        self.end_data(comment);
    }

    fn start_tag(&mut self, name: &str, attrs: Vec<(String, Option<String>)>, handle_empty: bool) {
        self.end_data(false);
        let mut dict: Vec<(String, String)> = Vec::new();
        for (k, v) in attrs {
            let v = v.unwrap_or_default();
            match dict.iter_mut().find(|(dk, _)| *dk == k) {
                Some(slot) => slot.1 = v,
                None => dict.push((k, v)),
            }
        }
        let parent = self.current();
        let id = self.dom.element(parent, name.to_string(), dict);
        self.stack.push(id);
        self.names.push(name.to_string());
        if matches!(name, "pre" | "textarea") {
            self.preserve_ws_depth += 1;
        }
        if VOID.contains(&name) && handle_empty {
            self.end_tag(name, false);
            self.already_closed_empty.push(name.to_string());
        }
    }

    fn end_tag(&mut self, name: &str, check_already_closed: bool) {
        if check_already_closed
            && let Some(i) = self.already_closed_empty.iter().position(|n| n == name)
        {
            self.already_closed_empty.remove(i);
            return;
        }
        self.end_data(false);
        // _popToTag: pop up to and including the most recent open `name`
        if !self.names[1..].iter().any(|n| n == name) {
            return;
        }
        while self.stack.len() > 1 {
            let popped = self.names.pop().unwrap_or_default();
            self.stack.pop();
            if matches!(popped.as_str(), "pre" | "textarea") {
                self.preserve_ws_depth -= 1;
            }
            if popped == name {
                break;
            }
        }
    }

    fn finish(mut self) -> Dom {
        self.end_data(false);
        self.dom
    }
}

struct Parser {
    raw: String,
    cdata_elem: Option<String>,
    /// RCDATA (title/textarea): character references are still recognised
    escapable: bool,
}

fn is_ws(c: u8) -> bool {
    matches!(c, b'\t' | b'\n' | b'\r' | b'\x0c' | b' ')
}

fn tag_name_end(b: &[u8], p: usize) -> usize {
    let mut i = p + 1;
    while i < b.len() && !matches!(b[i], b'\t' | b'\n' | b'\r' | b'\x0c' | b' ' | b'/' | b'>') {
        i += 1;
    }
    i
}

/// `tagfind_tolerant` with the name starting at p:
/// `([a-zA-Z][^\t\n\r\f />]*)(?:[\t\n\r\f ]|/(?!>))*`; (name end, match end)
fn tagfind(b: &[u8], p: usize) -> (usize, usize) {
    let name_end = tag_name_end(b, p);
    let mut i = name_end;
    loop {
        let slash = i < b.len() && b[i] == b'/' && b.get(i + 1) != Some(&b'>');
        if !(i < b.len() && is_ws(b[i]) || slash) {
            break;
        }
        i += 1;
    }
    (name_end, i)
}

/// `[\t\n\r\f ]*=[\t\n\r\f ]*` followed by a quoted or bare value; returns
/// (value start, value end) or None when the optional group does not match.
fn attr_value(b: &[u8], mut i: usize) -> Option<(usize, usize)> {
    while i < b.len() && is_ws(b[i]) {
        i += 1;
    }
    if b.get(i) != Some(&b'=') {
        return None;
    }
    i += 1;
    while i < b.len() && is_ws(b[i]) {
        i += 1;
    }
    match b.get(i) {
        Some(&q @ (b'\'' | b'"')) => {
            let close = b[i + 1..].iter().position(|&c| c == q)?;
            Some((i, i + 1 + close + 1))
        }
        _ => {
            let s = i;
            while i < b.len() && !matches!(b[i], b'>' | b'\t' | b'\n' | b'\r' | b'\x0c' | b' ') {
                i += 1;
            }
            Some((s, i))
        }
    }
}

/// `locatetagend` with the tag name starting at p; returns the match end.
fn locatetagend(b: &[u8], p: usize) -> usize {
    let ws_or_slash = |c: u8| matches!(c, b'\t' | b'\n' | b'\r' | b'\x0c' | b' ' | b'/');
    let mut i = tag_name_end(b, p);
    while i < b.len() && ws_or_slash(b[i]) {
        i += 1;
    }
    loop {
        let prev_ok = matches!(b[i - 1], b'\'' | b'"') || ws_or_slash(b[i - 1]);
        if !(prev_ok && i < b.len() && !ws_or_slash(b[i]) && b[i] != b'>') {
            break;
        }
        i += 1;
        while i < b.len() && !ws_or_slash(b[i]) && !matches!(b[i], b'=' | b'>') {
            i += 1;
        }
        if let Some((_, end)) = attr_value(b, i) {
            i = end;
        }
        while i < b.len() && ws_or_slash(b[i]) {
            i += 1;
        }
    }
    if b.get(i) == Some(&b'>') {
        i += 1;
    }
    i
}

/// `attrfind_tolerant` at k: (name range, value range if any, match end).
/// (name range, value range if any, match end)
type AttrMatch = ((usize, usize), Option<(usize, usize)>, usize);

fn attrfind(b: &[u8], k: usize) -> Option<AttrMatch> {
    let prev_ok = k > 0
        && matches!(
            b[k - 1],
            b'\'' | b'"' | b'\t' | b'\n' | b'\r' | b'\x0c' | b' ' | b'/'
        );
    if !prev_ok
        || k >= b.len()
        || matches!(b[k], b'\t' | b'\n' | b'\r' | b'\x0c' | b' ' | b'/' | b'>')
    {
        return None;
    }
    let mut i = k + 1;
    while i < b.len()
        && !matches!(
            b[i],
            b'\t' | b'\n' | b'\r' | b'\x0c' | b' ' | b'/' | b'=' | b'>'
        )
    {
        i += 1;
    }
    let name = (k, i);
    let value = attr_value(b, i);
    if let Some((_, end)) = value {
        i = end;
    }
    loop {
        let slash = i < b.len() && b[i] == b'/' && b.get(i + 1) != Some(&b'>');
        if !(i < b.len() && is_ws(b[i]) || slash) {
            break;
        }
        i += 1;
    }
    Some((name, value, i))
}

fn starttagopen(b: &[u8], i: usize) -> bool {
    b.get(i) == Some(&b'<') && b.get(i + 1).is_some_and(u8::is_ascii_alphabetic)
}

fn endtagopen(b: &[u8], i: usize) -> bool {
    b[i..].starts_with(b"</") && b.get(i + 2).is_some_and(u8::is_ascii_alphabetic)
}

fn find(b: &[u8], pat: &[u8], from: usize) -> Option<usize> {
    if from > b.len() {
        return None;
    }
    b[from..]
        .windows(pat.len())
        .position(|w| w == pat)
        .map(|p| p + from)
}

impl Parser {
    /// The next "interesting" position at or after i.
    fn interesting(&self, i: usize) -> Option<usize> {
        let b = self.raw.as_bytes();
        match &self.cdata_elem {
            None => b[i..]
                .iter()
                .position(|&c| c == b'<' || c == b'&')
                .map(|p| p + i),
            Some(elem) => {
                let mut j = i;
                while j < b.len() {
                    if self.escapable && b[j] == b'&' {
                        return Some(j);
                    }
                    if b[j] == b'<'
                        && b.get(j + 1) == Some(&b'/')
                        && b.len() >= j + 2 + elem.len()
                        && b[j + 2..j + 2 + elem.len()].eq_ignore_ascii_case(elem.as_bytes())
                        && b.get(j + 2 + elem.len()).is_some_and(|&c| {
                            matches!(c, b'\t' | b'\n' | b'\r' | b'\x0c' | b' ' | b'/' | b'>')
                        })
                    {
                        return Some(j);
                    }
                    j += 1;
                }
                None
            }
        }
    }

    fn goahead(&mut self, end: bool, t: &mut Builder) {
        let n = self.raw.len();
        let mut i = 0;
        while i < n {
            let j = match self.interesting(i) {
                Some(j) => j,
                None if self.cdata_elem.is_some() => break,
                None => n,
            };
            if i < j {
                t.data(&self.raw[i..j]);
            }
            i = j;
            if i == n {
                break;
            }
            let b = self.raw.as_bytes();
            if b[i] == b'<' {
                let k = if starttagopen(b, i) {
                    self.parse_starttag(i, t)
                } else if b[i..].starts_with(b"</") {
                    self.parse_endtag(i, t)
                } else if b[i..].starts_with(b"<!--") {
                    self.parse_comment(i, t)
                } else if b[i..].starts_with(b"<?") {
                    self.parse_pi(i, t)
                } else if b[i..].starts_with(b"<!") {
                    self.parse_html_declaration(i, t)
                } else if i + 1 < n || end {
                    t.data("<");
                    (i + 1) as isize
                } else {
                    break;
                };
                if k < 0 {
                    if !end {
                        break;
                    }
                    self.unterminated(i, t);
                    i = n;
                } else {
                    i = k as usize;
                }
            } else if b[i..].starts_with(b"&#") {
                match charref(b, i) {
                    Some((name_end, mend)) => {
                        handle_charref(&self.raw[i + 2..name_end], t);
                        i = if b[mend - 1] == b';' { mend } else { mend - 1 };
                        continue;
                    }
                    None => {
                        if self.raw[i..].contains(';') {
                            t.data(&self.raw[i..i + 2]);
                            i += 2;
                        }
                        break;
                    }
                }
            } else {
                // '&'
                if let Some((name_end, mend)) = entityref(b, i) {
                    handle_entityref(&self.raw[i + 1..name_end], t);
                    i = if b[mend - 1] == b';' { mend } else { mend - 1 };
                    continue;
                }
                let incomplete = b
                    .get(i + 1)
                    .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'#');
                if incomplete {
                    if end && n - i == 2 {
                        i += 1;
                    }
                    break;
                } else if i + 1 < n {
                    t.data("&");
                    i += 1;
                } else {
                    break;
                }
            }
        }
        if end && i < n {
            t.data(&self.raw[i..n]);
            i = n;
        }
        self.raw = self.raw[i..].to_string();
    }

    /// `goahead` with `end` set, when a construct at i never closes.
    fn unterminated(&mut self, i: usize, t: &mut Builder) {
        let raw = self.raw.clone();
        let b = raw.as_bytes();
        let n = b.len();
        if starttagopen(b, i) {
        } else if b[i..].starts_with(b"</") {
            if i + 2 == n {
                t.data("</");
            } else if endtagopen(b, i) {
            } else {
                t.special(&raw[i + 2..], true);
            }
        } else if b[i..].starts_with(b"<!--") {
            let mut j = n;
            for suffix in ["--!", "--", "-"] {
                if n >= i + 4 + suffix.len() && raw[i + 4..].ends_with(suffix) {
                    j -= suffix.len();
                    break;
                }
            }
            t.special(&raw[i + 4..j], true);
        } else if b[i..].starts_with(b"<![CDATA[") {
            unknown_decl(&raw[i + 3..], t);
        } else if raw
            .get(i..i + 9)
            .is_some_and(|s| s.eq_ignore_ascii_case("<!doctype"))
        {
            handle_decl(&raw[i + 2..], t);
        } else if b[i..].starts_with(b"<!") {
            t.special(&raw[i + 2..], true);
        } else if b[i..].starts_with(b"<?") {
            t.special(&raw[i + 2..], false);
        }
    }

    fn parse_starttag(&mut self, i: usize, t: &mut Builder) -> isize {
        let b = self.raw.as_bytes();
        let j = locatetagend(b, i + 1);
        if b[j - 1] != b'>' {
            return -1;
        }
        let endpos = j;
        let (name_end, mut k) = tagfind(b, i + 1);
        let tag = self.raw[i + 1..name_end].to_lowercase();
        let mut attrs = Vec::new();
        while k < endpos {
            let Some((name, value, mend)) = attrfind(b, k) else {
                break;
            };
            let attrname = self.raw[name.0..name.1].to_lowercase();
            let value = value.map(|(s, e)| {
                let v = &self.raw[s..e];
                let v = if v.len() >= 2
                    && (v.starts_with('\'') && v.ends_with('\'')
                        || v.starts_with('"') && v.ends_with('"'))
                {
                    &v[1..v.len() - 1]
                } else {
                    v
                };
                if v.is_empty() {
                    String::new()
                } else {
                    unescape(v)
                }
            });
            attrs.push((attrname, value));
            k = mend;
        }
        let end = pystr::strip(&self.raw[k..endpos]);
        if end != ">" && end != "/>" {
            t.data(&self.raw[i..endpos]);
            return endpos as isize;
        }
        if end.ends_with("/>") {
            t.start_tag(&tag, attrs, false);
            t.end_tag(&tag, false);
        } else {
            t.start_tag(&tag, attrs, true);
            if matches!(tag.as_str(), "script" | "style") {
                self.cdata_elem = Some(tag);
                self.escapable = false;
            } else if matches!(tag.as_str(), "textarea" | "title") {
                self.cdata_elem = Some(tag);
                self.escapable = true;
            }
        }
        endpos as isize
    }

    fn parse_endtag(&mut self, i: usize, t: &mut Builder) -> isize {
        let b = self.raw.as_bytes();
        if find(b, b">", i + 2).is_none() {
            return -1;
        }
        if !endtagopen(b, i) {
            if b.get(i + 2) == Some(&b'>') {
                return (i + 3) as isize;
            }
            return self.parse_bogus_comment(i, t);
        }
        let j = locatetagend(b, i + 2);
        if b[j - 1] != b'>' {
            return -1;
        }
        let (name_end, _) = tagfind(b, i + 2);
        let tag = self.raw[i + 2..name_end].to_lowercase();
        t.end_tag(&tag, true);
        self.cdata_elem = None;
        self.escapable = true;
        j as isize
    }

    fn parse_comment(&mut self, i: usize, t: &mut Builder) -> isize {
        let b = self.raw.as_bytes();
        // commentclose: --!?>
        let mut close = None;
        let mut j = i + 4;
        while j < b.len() {
            if b[j..].starts_with(b"--!>") {
                close = Some((j, j + 4));
                break;
            }
            if b[j..].starts_with(b"-->") {
                close = Some((j, j + 3));
                break;
            }
            j += 1;
        }
        if close.is_none() {
            // commentabruptclose: -?> right after "<!--"
            if b[i + 4..].starts_with(b"->") {
                close = Some((i + 4, i + 6));
            } else if b[i + 4..].starts_with(b">") {
                close = Some((i + 4, i + 5));
            }
        }
        let Some((start, end)) = close else { return -1 };
        let data = self.raw[i + 4..start].to_string();
        t.special(&data, true);
        end as isize
    }

    fn parse_bogus_comment(&mut self, i: usize, t: &mut Builder) -> isize {
        let Some(pos) = find(self.raw.as_bytes(), b">", i + 2) else {
            return -1;
        };
        let data = self.raw[i + 2..pos].to_string();
        t.special(&data, true);
        (pos + 1) as isize
    }

    fn parse_pi(&mut self, i: usize, t: &mut Builder) -> isize {
        let Some(j) = find(self.raw.as_bytes(), b">", i + 2) else {
            return -1;
        };
        let data = self.raw[i + 2..j].to_string();
        t.special(&data, false);
        (j + 1) as isize
    }

    fn parse_html_declaration(&mut self, i: usize, t: &mut Builder) -> isize {
        let raw = self.raw.clone();
        let b = raw.as_bytes();
        if b[i..].starts_with(b"<!--") {
            return self.parse_comment(i, t);
        }
        if b[i..].starts_with(b"<![CDATA[") {
            let Some(j) = find(b, b"]]>", i + 9) else {
                return -1;
            };
            unknown_decl(&raw[i + 3..j], t);
            return (j + 3) as isize;
        }
        if raw
            .get(i..i + 9)
            .is_some_and(|s| s.eq_ignore_ascii_case("<!doctype"))
        {
            let Some(gt) = find(b, b">", i + 9) else {
                return -1;
            };
            handle_decl(&raw[i + 2..gt], t);
            return (gt + 1) as isize;
        }
        if b[i..].starts_with(b"<![") {
            let Some(j) = find(b, b">", i + 3) else {
                return -1;
            };
            if b[j - 1] == b']' {
                unknown_decl(&raw[i + 3..j - 1], t);
            } else {
                t.special(&raw[i + 2..j], true);
            }
            return (j + 1) as isize;
        }
        self.parse_bogus_comment(i, t)
    }
}

/// `charref`: `&#(?:[0-9]+|[xX][0-9a-fA-F]+)[^0-9a-fA-F]` at i; returns
/// (end of the number, end of the match).
fn charref(b: &[u8], i: usize) -> Option<(usize, usize)> {
    let mut j = i + 2;
    let hex = matches!(b.get(j), Some(b'x' | b'X'));
    if hex {
        j += 1;
    }
    let start = j;
    let digit = |c: u8| {
        if hex {
            c.is_ascii_hexdigit()
        } else {
            c.is_ascii_digit()
        }
    };
    while j < b.len() && digit(b[j]) {
        j += 1;
    }
    if j == start {
        return None;
    }
    // the terminator must not be a hex digit; for decimal, backtracking
    // cannot help because every shorter run is followed by a digit
    match b.get(j) {
        Some(c) if !c.is_ascii_hexdigit() => Some((j, j + 1)),
        _ => None,
    }
}

/// `entityref`: `&([a-zA-Z][-.a-zA-Z0-9]*)[^a-zA-Z0-9]` with backtracking.
fn entityref(b: &[u8], i: usize) -> Option<(usize, usize)> {
    if !b.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    let mut e = i + 2;
    while e < b.len() && (b[e].is_ascii_alphanumeric() || b[e] == b'-' || b[e] == b'.') {
        e += 1;
    }
    let mut p = e;
    while p > i + 1 {
        if b.get(p).is_some_and(|c| !c.is_ascii_alphanumeric()) {
            return Some((p, p + 1));
        }
        p -= 1;
    }
    None
}

fn handle_charref(name: &str, t: &mut Builder) {
    let (digits, base) = match name.strip_prefix(['x', 'X']) {
        Some(h) => (h, 16),
        None => (name, 10),
    };
    let value = u128::from_str_radix(digits, base).unwrap_or(u128::MAX);
    let s = match value {
        0 => "\u{fffd}".to_string(),
        v if v > 0x10ffff => "\u{fffd}".to_string(),
        v if (0xd800..=0xdfff).contains(&v) => "\u{fffd}".to_string(),
        v if (0x80..=0x9f).contains(&v) => cp1252(v as u8).to_string(),
        v => char::from_u32(v as u32)
            .map(String::from)
            .unwrap_or_else(|| "\u{fffd}".into()),
    };
    t.data(&s);
}

/// bs4's WINDOWS_1252_TO_UTF8 for 0x80..=0x9f (undefined bytes map to the
/// C1 control of the same value).
fn cp1252(b: u8) -> char {
    const TABLE: [u32; 32] = [
        0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039,
        0x152, 0x8d, 0x17d, 0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014,
        0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178,
    ];
    char::from_u32(TABLE[(b - 0x80) as usize]).unwrap_or('\u{fffd}')
}

fn handle_entityref(name: &str, t: &mut Builder) {
    match BS4.binary_search_by(|(k, _)| (*k).cmp(name)) {
        Ok(i) => t.data(BS4[i].1),
        Err(_) => t.data(&format!("&{name}")),
    }
}

fn handle_decl(decl: &str, t: &mut Builder) {
    let d = decl.get("DOCTYPE ".len()..).unwrap_or_default();
    t.special(d, true);
}

fn unknown_decl(data: &str, t: &mut Builder) {
    let is_cdata = data
        .get(..6)
        .is_some_and(|p| p.eq_ignore_ascii_case("CDATA["));
    let d = if is_cdata { &data[6..] } else { data };
    // CData and Declaration are both rendered as text by markdownify
    t.special(d, false);
}
