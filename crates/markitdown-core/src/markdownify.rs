//! HTML tree -> Markdown, ported from markdownify 1.2.3 with markitdown's
//! `_CustomMarkdownify` overrides (ATX headings, truncated data URIs,
//! scheme-filtered and path-quoted links, `<u>` kept as HTML, checkboxes).
//!
//! Only the tags the docx and pptx converters emit have conversion rules;
//! any other tag passes its children's text through, as in markdownify.

use crate::pystr;

enum Kind {
    Element {
        name: String,
        attrs: Vec<(String, String)>,
    },
    Text(String),
}

struct DNode {
    kind: Kind,
    parent: Option<usize>,
    children: Vec<usize>,
    /// index within the parent's children
    pos: usize,
}

/// An HTML tree as BeautifulSoup's html.parser would build it.
pub struct Dom {
    nodes: Vec<DNode>,
}

pub const ROOT: usize = 0;

impl Dom {
    /// `root_name` is "[document]" for a whole parsed document.
    pub fn new(root_name: &str) -> Self {
        let root = DNode {
            kind: Kind::Element {
                name: root_name.to_string(),
                attrs: Vec::new(),
            },
            parent: None,
            children: Vec::new(),
            pos: 0,
        };
        Dom { nodes: vec![root] }
    }

    fn push(&mut self, parent: usize, kind: Kind) -> usize {
        let id = self.nodes.len();
        let pos = self.nodes[parent].children.len();
        self.nodes.push(DNode {
            kind,
            parent: Some(parent),
            children: Vec::new(),
            pos,
        });
        self.nodes[parent].children.push(id);
        id
    }

    pub fn element(&mut self, parent: usize, name: &str, attrs: Vec<(String, String)>) -> usize {
        self.push(
            parent,
            Kind::Element {
                name: name.to_string(),
                attrs,
            },
        )
    }

    /// Append text, merging with a preceding text node like the HTML parser.
    pub fn text(&mut self, parent: usize, s: &str) {
        if s.is_empty() {
            return;
        }
        if let Some(&last) = self.nodes[parent].children.last()
            && let Kind::Text(t) = &mut self.nodes[last].kind
        {
            t.push_str(s);
            return;
        }
        self.push(parent, Kind::Text(s.to_string()));
    }

    fn name(&self, id: usize) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Element { name, .. } => Some(name),
            Kind::Text(_) => None,
        }
    }

    fn attr(&self, id: usize, key: &str) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Element { attrs, .. } => attrs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str()),
            Kind::Text(_) => None,
        }
    }

    fn parent(&self, id: usize) -> Option<usize> {
        self.nodes[id].parent
    }

    fn sibling(&self, id: usize, offset: isize) -> Option<usize> {
        let n = &self.nodes[id];
        let p = n.parent?;
        let idx = n.pos.checked_add_signed(offset)?;
        self.nodes[p].children.get(idx).copied()
    }

    fn prev(&self, id: usize) -> Option<usize> {
        self.sibling(id, -1)
    }

    fn next(&self, id: usize) -> Option<usize> {
        self.sibling(id, 1)
    }

    fn is_element_named(&self, id: usize, names: &[&str]) -> bool {
        self.name(id).is_some_and(|n| names.contains(&n))
    }

    fn find_all(&self, id: usize, names: &[&str], out: &mut Vec<usize>) {
        for &c in &self.nodes[id].children {
            if self.is_element_named(c, names) {
                out.push(c);
            }
            self.find_all(c, names, out);
        }
    }

    fn count_all(&self, id: usize, names: &[&str]) -> usize {
        let mut v = Vec::new();
        self.find_all(id, names, &mut v);
        v.len()
    }

    fn prev_element_sibling(&self, id: usize) -> Option<usize> {
        let n = &self.nodes[id];
        let p = n.parent?;
        self.nodes[p].children[..n.pos]
            .iter()
            .rev()
            .copied()
            .find(|&c| self.name(c).is_some())
    }
}

#[derive(Clone, Copy, Default)]
struct Ctx {
    inline: bool,
    li: bool,
}

fn is_heading(name: &str) -> bool {
    // re.match(r'h(\d+)')
    name.strip_prefix('h')
        .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
}

fn heading_level(name: &str) -> Option<usize> {
    let digits: String = name[1..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn remove_ws_inside(name: Option<&str>) -> bool {
    let Some(name) = name else { return false };
    is_heading(name)
        || matches!(
            name,
            "p" | "blockquote"
                | "article"
                | "div"
                | "section"
                | "ol"
                | "ul"
                | "li"
                | "dl"
                | "dt"
                | "dd"
                | "table"
                | "thead"
                | "tbody"
                | "tfoot"
                | "tr"
                | "td"
                | "th"
        )
}

fn remove_ws_outside(name: Option<&str>) -> bool {
    remove_ws_inside(name) || name == Some("pre")
}

pub struct Options {
    pub keep_data_uris: bool,
}

pub fn convert(dom: &Dom, root: usize, opts: &Options) -> String {
    let c = Converter { dom, opts };
    c.process_tag(root, Ctx::default())
}

struct Converter<'a> {
    dom: &'a Dom,
    opts: &'a Options,
}

impl Converter<'_> {
    fn name_of(&self, id: Option<usize>) -> Option<&str> {
        id.and_then(|i| self.dom.name(i))
    }

    fn can_ignore(&self, id: usize, remove_inside: bool) -> bool {
        let Kind::Text(t) = &self.dom.nodes[id].kind else {
            return false;
        };
        if !pystr::strip(t).is_empty() {
            return false;
        }
        let prev = self.dom.prev(id);
        let next = self.dom.next(id);
        (remove_inside && (prev.is_none() || next.is_none()))
            || remove_ws_outside(self.name_of(prev))
            || remove_ws_outside(self.name_of(next))
    }

    fn process_tag(&self, id: usize, ctx: Ctx) -> String {
        let name = self.dom.name(id).unwrap_or_default();
        let remove_inside = remove_ws_inside(Some(name));
        let child_ctx = Ctx {
            inline: ctx.inline || is_heading(name) || name == "td" || name == "th",
            li: ctx.li || name == "li",
        };
        let mut parts: Vec<String> = Vec::new();
        for &c in &self.dom.nodes[id].children {
            if self.can_ignore(c, remove_inside) {
                continue;
            }
            let s = match self.dom.nodes[c].kind {
                Kind::Text(_) => self.process_text(c),
                Kind::Element { .. } => self.process_tag(c, child_ctx),
            };
            if !s.is_empty() {
                parts.push(s);
            }
        }
        let text = collapse_child_newlines(&parts);
        self.convert_tag(id, name, text, ctx)
    }

    fn process_text(&self, id: usize) -> String {
        let Kind::Text(raw) = &self.dom.nodes[id].kind else {
            unreachable!()
        };
        let mut text = normalize_ws(raw);
        // escape_asterisks / escape_underscores
        if text.contains(['*', '_']) {
            text = text.replace('*', r"\*").replace('_', r"\_");
        }
        let parent_inside = remove_ws_inside(self.name_of(self.dom.parent(id)));
        let prev = self.dom.prev(id);
        let next = self.dom.next(id);
        if remove_ws_outside(self.name_of(prev)) || (parent_inside && prev.is_none()) {
            text = text.trim_start_matches([' ', '\t', '\r', '\n']).to_string();
        }
        if remove_ws_outside(self.name_of(next)) || (parent_inside && next.is_none()) {
            text.truncate(pystr::rstrip(&text).len());
        }
        text
    }

    fn convert_tag(&self, id: usize, name: &str, text: String, ctx: Ctx) -> String {
        match name {
            "[document]" => text.trim_matches('\n').to_string(),
            "a" => self.convert_a(id, &text),
            "b" | "strong" => inline_markup(&text, "**", "**"),
            "em" | "i" => inline_markup(&text, "*", "*"),
            "s" | "del" | "strike" => inline_markup(&text, "~~", "~~"),
            "sub" | "sup" => inline_markup(&text, "", ""),
            "u" => {
                if pystr::strip(&text).is_empty() {
                    text
                } else {
                    inline_markup(&text, "<u>", "</u>")
                }
            }
            "br" => {
                if ctx.inline {
                    if text.is_empty() {
                        " ".to_string()
                    } else {
                        text + " "
                    }
                } else {
                    "  \n".to_string() + &text
                }
            }
            "p" => {
                if ctx.inline {
                    format!(" {} ", pystr::strip_ascii_ws(&text))
                } else {
                    let t = pystr::strip_ascii_ws(&text);
                    if t.is_empty() {
                        String::new()
                    } else {
                        format!("\n\n{t}\n\n")
                    }
                }
            }
            "img" => self.convert_img(id),
            "input" => match self.dom.attr(id, "type") {
                Some("checkbox") if self.dom.attr(id, "checked").is_some() => "[x] ".into(),
                Some("checkbox") => "[ ] ".into(),
                _ => String::new(),
            },
            "ul" | "ol" => self.convert_list(id, &text, ctx),
            "li" => self.convert_li(id, &text),
            "table" => format!("\n\n{}\n\n", pystr::strip(&text)),
            "td" | "th" => {
                let colspan = self.colspan(id);
                format!(
                    " {}{}",
                    pystr::strip(&text).replace('\n', " "),
                    " |".repeat(colspan)
                )
            }
            "tr" => self.convert_tr(id, &text),
            n if is_heading(n) => {
                if ctx.inline {
                    return text;
                }
                let level = heading_level(n).unwrap_or(1).clamp(1, 6);
                let t = squeeze_all_ws(pystr::strip(&text));
                format!("\n\n{} {}\n\n", "#".repeat(level), t)
            }
            _ => text,
        }
    }

    fn convert_a(&self, id: usize, text: &str) -> String {
        let (prefix, suffix, text) = chomp(text);
        if text.is_empty() {
            return String::new();
        }
        let title = self.dom.attr(id, "title").filter(|t| !t.is_empty());
        let mut href = self.dom.attr(id, "href").unwrap_or_default().to_string();
        if !href.is_empty() {
            match url::requote(&href) {
                Some(h) => href = h,
                None => return format!("{prefix}{text}{suffix}"),
            }
        }
        if text.replace(r"\_", "_") == href && title.is_none() {
            return format!("<{href}>");
        }
        if href.is_empty() {
            return text.to_string();
        }
        let title_part = title
            .map(|t| format!(" \"{}\"", t.replace('"', "\\\"")))
            .unwrap_or_default();
        format!("{prefix}[{text}]({href}{title_part}){suffix}")
    }

    fn convert_img(&self, id: usize) -> String {
        let attr = |k| self.dom.attr(id, k).unwrap_or_default();
        let alt = attr("alt").replace('\n', " ");
        let mut src = attr("src").to_string();
        let data_src = attr("data-src");
        let is_data = |s: &str| s.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("data:"));
        if !data_src.is_empty() && (src.is_empty() || (is_data(&src) && !self.opts.keep_data_uris))
        {
            src = data_src.to_string();
        }
        let title = attr("title");
        let title_part = if title.is_empty() {
            String::new()
        } else {
            format!(" \"{}\"", title.replace('"', "\\\""))
        };
        if is_data(&src) && !self.opts.keep_data_uris {
            src = format!("{}...", src.split(',').next().unwrap_or_default());
        }
        format!("![{alt}]({src}{title_part})")
    }

    fn convert_list(&self, id: usize, text: &str, ctx: Ctx) -> String {
        // _next_block_content_sibling
        let mut next = self.dom.next(id);
        while let Some(n) = next {
            match &self.dom.nodes[n].kind {
                Kind::Text(t) if pystr::strip(t).is_empty() => next = self.dom.next(n),
                _ => break,
            }
        }
        let before_paragraph = next.is_some_and(|n| !self.dom.is_element_named(n, &["ul", "ol"]));
        if ctx.li {
            return format!("\n{}", pystr::rstrip(text));
        }
        format!("\n\n{text}{}", if before_paragraph { "\n" } else { "" })
    }

    fn convert_li(&self, id: usize, text: &str) -> String {
        let text = pystr::strip(text);
        if text.is_empty() {
            return "\n".to_string();
        }
        let parent = self.dom.parent(id);
        let bullet = if self.name_of(parent) == Some("ol") {
            let p = parent.unwrap_or(ROOT);
            let start = self
                .dom
                .attr(p, "start")
                .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(1);
            let before = self.dom.nodes[p].children[..self.dom.nodes[id].pos]
                .iter()
                .filter(|&&c| self.dom.is_element_named(c, &["li"]))
                .count();
            format!("{}. ", start + before)
        } else {
            let mut depth: isize = -1;
            let mut cur = Some(id);
            while let Some(c) = cur {
                if self.dom.name(c) == Some("ul") {
                    depth += 1;
                }
                cur = self.dom.parent(c);
            }
            let bullets = ['*', '+', '-'];
            format!("{} ", bullets[depth.rem_euclid(3) as usize])
        };
        let width = bullet.len();
        let indent = " ".repeat(width);
        let indented: Vec<String> = text
            .split('\n')
            .map(|line| {
                if line.is_empty() {
                    String::new()
                } else {
                    format!("{indent}{line}")
                }
            })
            .collect();
        let indented = indented.join("\n");
        format!("{bullet}{}\n", &indented[width..])
    }

    fn colspan(&self, id: usize) -> usize {
        match self.dom.attr(id, "colspan") {
            Some(v) if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) => {
                v.parse::<usize>().unwrap_or(1).clamp(1, 1000)
            }
            _ => 1,
        }
    }

    fn convert_tr(&self, id: usize, text: &str) -> String {
        let mut cells = Vec::new();
        self.dom.find_all(id, &["td", "th"], &mut cells);
        let parent = self.dom.parent(id).unwrap_or(ROOT);
        let parent_name = self.dom.name(parent).unwrap_or_default();
        let is_first_row = self.dom.prev_element_sibling(id).is_none();
        let is_headrow = cells.iter().all(|&c| self.dom.name(c) == Some("th"))
            || (parent_name == "thead" && self.dom.count_all(parent, &["tr"]) == 1);
        let is_head_row_missing = (is_first_row && parent_name != "tbody")
            || (is_first_row
                && parent_name == "tbody"
                && self
                    .dom
                    .parent(parent)
                    .is_none_or(|gp| self.dom.count_all(gp, &["thead"]) < 1));
        let full_colspan: usize = cells.iter().map(|&c| self.colspan(c)).sum();
        let mut overline = String::new();
        let mut underline = String::new();
        if is_headrow && is_first_row {
            underline = format!("| {} |\n", vec!["---"; full_colspan].join(" | "));
        } else if is_head_row_missing
            || (is_first_row
                && (parent_name == "table"
                    || (parent_name == "tbody" && self.dom.prev_element_sibling(parent).is_none())))
        {
            overline = format!("| {} |\n", vec![""; full_colspan].join(" | "));
            overline += &format!("| {} |\n", vec!["---"; full_colspan].join(" | "));
        }
        format!("{overline}|{text}\n{underline}")
    }
}

/// markdownify's newline collapsing at child boundaries.
fn collapse_child_newlines(parts: &[String]) -> String {
    let mut out = String::new();
    // trailing newline count of the previous child, kept out of `out` until
    // the next child decides how many to emit
    let mut prev_trailing = 0usize;
    for s in parts {
        let leading = s.len() - s.trim_start_matches('\n').len();
        let (content, trailing) = if leading == s.len() {
            ("", 0)
        } else {
            let body = &s[leading..];
            let t = body.len() - body.trim_end_matches('\n').len();
            (&body[..body.len() - t], t)
        };
        let lead = if prev_trailing > 0 && leading > 0 {
            prev_trailing.max(leading).min(2)
        } else {
            out.extend(std::iter::repeat_n('\n', prev_trailing));
            leading
        };
        out.extend(std::iter::repeat_n('\n', lead));
        out.push_str(content);
        prev_trailing = trailing;
    }
    out.extend(std::iter::repeat_n('\n', prev_trailing));
    out
}

/// `re_newline_whitespace.sub('\n')` then `re_whitespace.sub(' ')`.
fn normalize_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '\t' | ' ' | '\r' | '\n') {
            let start = i;
            while i < chars.len() && matches!(chars[i], '\t' | ' ' | '\r' | '\n') {
                i += 1;
            }
            let run = &chars[start..i];
            if run.iter().any(|&c| c == '\r' || c == '\n') {
                out.push('\n');
            } else {
                out.push(' ');
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// `re_all_whitespace.sub(' ')`
fn squeeze_all_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if matches!(c, '\t' | ' ' | '\r' | '\n') {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            in_ws = false;
            out.push(c);
        }
    }
    out
}

fn chomp(text: &str) -> (&'static str, &'static str, &str) {
    let prefix = if text.starts_with(' ') { " " } else { "" };
    let suffix = if text.ends_with(' ') { " " } else { "" };
    (prefix, suffix, pystr::strip(text))
}

fn inline_markup(text: &str, open: &str, close: &str) -> String {
    let (prefix, suffix, text) = chomp(text);
    if text.is_empty() {
        return String::new();
    }
    format!("{prefix}{open}{text}{close}{suffix}")
}

/// The subset of `urllib.parse` (CPython 3.12) that markitdown's link
/// rewriting goes through: `urlparse`, path quoting, `urlunparse`.
mod url {
    const USES_PARAMS: &[&str] = &[
        "", "ftp", "hdl", "prospero", "http", "imap", "https", "shttp", "rtsp", "rtsps", "rtspu",
        "sip", "sips", "mms", "sftp", "tel",
    ];
    const USES_NETLOC: &[&str] = &[
        "",
        "ftp",
        "http",
        "gopher",
        "nntp",
        "telnet",
        "imap",
        "wais",
        "file",
        "mms",
        "https",
        "shttp",
        "snews",
        "prospero",
        "rtsp",
        "rtsps",
        "rtspu",
        "rsync",
        "svn",
        "svn+ssh",
        "sftp",
        "nfs",
        "git",
        "git+ssh",
        "ws",
        "wss",
        "itms-services",
    ];

    /// Returns None when the link must be dropped (non-web scheme, bad IPv6).
    pub fn requote(href: &str) -> Option<String> {
        let mut url: String = href.trim_start_matches(|c: char| c <= ' ').to_string();
        url.retain(|c| !matches!(c, '\t' | '\r' | '\n'));
        let mut scheme = String::new();
        if let Some(i) = url.find(':') {
            let cand = &url[..i];
            if i > 0
                && cand.starts_with(|c: char| c.is_ascii_alphabetic())
                && cand
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
            {
                scheme = cand.to_ascii_lowercase();
                url = url[i + 1..].to_string();
            }
        }
        let mut netloc = String::new();
        if url.starts_with("//") {
            let delim = ['/', '?', '#']
                .iter()
                .filter_map(|&c| url[2..].find(c).map(|i| i + 2))
                .min();
            let delim = delim.unwrap_or(url.len());
            netloc = url[2..delim].to_string();
            url = url[delim..].to_string();
            if netloc.contains('[') != netloc.contains(']') {
                return None;
            }
        }
        let mut fragment = String::new();
        if let Some(i) = url.find('#') {
            fragment = url[i + 1..].to_string();
            url.truncate(i);
        }
        let mut query = String::new();
        if let Some(i) = url.find('?') {
            query = url[i + 1..].to_string();
            url.truncate(i);
        }
        if !scheme.is_empty() && !matches!(scheme.as_str(), "http" | "https" | "file") {
            return None;
        }
        let mut params = String::new();
        if USES_PARAMS.contains(&scheme.as_str()) && url.contains(';') {
            let i = match url.rfind('/') {
                Some(slash) => url[slash..].find(';').map(|j| j + slash),
                None => url.find(';'),
            };
            if let Some(i) = i {
                params = url[i + 1..].to_string();
                url.truncate(i);
            }
        }
        let mut path = quote_preserving_escapes(&url);
        if !params.is_empty() {
            path = format!("{path};{params}");
        }
        // urlunsplit
        let mut out = if !netloc.is_empty() {
            if !path.is_empty() && !path.starts_with('/') {
                path.insert(0, '/');
            }
            format!("//{netloc}{path}")
        } else if path.starts_with("//")
            || (!scheme.is_empty()
                && USES_NETLOC.contains(&scheme.as_str())
                && (path.is_empty() || path.starts_with('/')))
        {
            format!("//{path}")
        } else {
            path
        };
        if !scheme.is_empty() {
            out = format!("{scheme}:{out}");
        }
        if !query.is_empty() {
            out = format!("{out}?{query}");
        }
        if !fragment.is_empty() {
            out = format!("{out}#{fragment}");
        }
        Some(out)
    }

    /// `quote(path)` on the stretches between existing `%HH` escapes.
    fn quote_preserving_escapes(path: &str) -> String {
        let b = path.as_bytes();
        let mut out = String::with_capacity(path.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%'
                && i + 2 < b.len()
                && b[i + 1].is_ascii_hexdigit()
                && b[i + 2].is_ascii_hexdigit()
            {
                out.push_str(&path[i..i + 3]);
                i += 3;
                continue;
            }
            let c = b[i];
            if c.is_ascii_alphanumeric() || b"_.-~/".contains(&c) {
                out.push(c as char);
            } else {
                out.push_str(&format!("%{c:02X}"));
            }
            i += 1;
        }
        out
    }
}
