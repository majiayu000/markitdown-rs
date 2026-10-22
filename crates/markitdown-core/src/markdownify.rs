//! HTML tree -> Markdown, ported from markdownify 1.2.3 with markitdown's
//! `_CustomMarkdownify` overrides (ATX headings, truncated data URIs,
//! scheme-filtered and path-quoted links, `<u>` kept as HTML, checkboxes,
//! `<strike>` as `<s>`). Options are markitdown's defaults.

use crate::dom::{Dom, Kind};
use crate::{pystr, pyurl};

pub use crate::dom::ROOT;

/// markdownify's `parent_tags` set, reduced to the members it tests.
#[derive(Clone, Copy, Default)]
struct Ctx {
    inline: bool,
    noformat: bool,
    pre: bool,
    li: bool,
}

fn is_heading(name: &str) -> bool {
    // re.match(r'h(\d+)')
    name.strip_prefix('h')
        .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
}

fn heading_level(name: &str) -> usize {
    let digits: String = name[1..].chars().take_while(char::is_ascii_digit).collect();
    // int() of a long digit run is large: max(1, min(6, n))
    digits.parse::<u64>().map_or(6, |n| n.clamp(1, 6) as usize)
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

/// `convert_soup(node)`
pub fn convert(dom: &Dom, root: usize, opts: &Options) -> String {
    Converter { dom, opts }.process_tag(root, Ctx::default())
}

struct Converter<'a> {
    dom: &'a Dom,
    opts: &'a Options,
}

impl Converter<'_> {
    fn name_of(&self, id: Option<usize>) -> Option<&str> {
        id.and_then(|i| self.dom.name(i))
    }

    /// whitespace-only text next to block boundaries, comments and doctypes
    fn can_ignore(&self, id: usize, remove_inside: bool) -> bool {
        match self.dom.kind(id) {
            Kind::Element { .. } => false,
            Kind::Comment(_) => true,
            Kind::Text(t) => {
                if !pystr::strip(t).is_empty() {
                    return false;
                }
                let prev = self.dom.prev(id);
                let next = self.dom.next(id);
                (remove_inside && (prev.is_none() || next.is_none()))
                    || remove_ws_outside(self.name_of(prev))
                    || remove_ws_outside(self.name_of(next))
            }
        }
    }

    fn process_tag(&self, id: usize, ctx: Ctx) -> String {
        let name = self.dom.name(id).unwrap_or_default();
        let remove_inside = remove_ws_inside(Some(name));
        let child_ctx = Ctx {
            inline: ctx.inline || is_heading(name) || name == "td" || name == "th",
            noformat: ctx.noformat || matches!(name, "pre" | "code" | "kbd" | "samp"),
            pre: ctx.pre || name == "pre",
            li: ctx.li || name == "li",
        };
        let mut parts: Vec<String> = Vec::new();
        for &c in self.dom.children(id) {
            if self.can_ignore(c, remove_inside) {
                continue;
            }
            let s = match self.dom.kind(c) {
                Kind::Text(_) => self.process_text(c, child_ctx),
                _ => self.process_tag(c, child_ctx),
            };
            if !s.is_empty() {
                parts.push(s);
            }
        }
        let text = if name == "pre" || ctx.pre {
            parts.concat()
        } else {
            collapse_child_newlines(&parts)
        };
        self.convert_tag(id, name, text, ctx)
    }

    fn process_text(&self, id: usize, ctx: Ctx) -> String {
        let Kind::Text(raw) = self.dom.kind(id) else {
            unreachable!()
        };
        let mut text = if ctx.pre {
            raw.clone()
        } else {
            normalize_ws(raw)
        };
        if !ctx.noformat && text.contains(['*', '_']) {
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
        let inline = |open: &str, close: &str| {
            if ctx.noformat {
                text.clone()
            } else {
                inline_markup(&text, open, close)
            }
        };
        match name {
            "[document]" => text.trim_matches('\n').to_string(),
            "a" => self.convert_a(id, &text),
            "b" | "strong" => inline("**", "**"),
            "em" | "i" => inline("*", "*"),
            "s" | "del" | "strike" => inline("~~", "~~"),
            "sub" | "sup" => inline("", ""),
            "u" => {
                if pystr::strip(&text).is_empty() {
                    text
                } else {
                    inline_markup(&text, "<u>", "</u>")
                }
            }
            "code" | "kbd" | "samp" => {
                if ctx.noformat {
                    return text;
                }
                let (prefix, suffix, t) = chomp(&text);
                if t.is_empty() {
                    return String::new();
                }
                let max_ticks = t.split(|c| c != '`').map(str::len).max().unwrap_or(0);
                let delim = "`".repeat(max_ticks + 1);
                let t = if max_ticks > 0 {
                    format!(" {t} ")
                } else {
                    t.to_string()
                };
                format!("{prefix}{delim}{t}{delim}{suffix}")
            }
            "blockquote" => {
                let t = pystr::strip_ascii_ws(&text);
                if ctx.inline {
                    return format!(" {t} ");
                }
                if t.is_empty() {
                    return "\n".into();
                }
                let quoted: Vec<String> = t
                    .split('\n')
                    .map(|l| {
                        if l.is_empty() {
                            ">".to_string()
                        } else {
                            format!("> {l}")
                        }
                    })
                    .collect();
                format!("\n{}\n\n", quoted.join("\n"))
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
            "div" | "article" | "section" | "dl" => {
                let t = pystr::strip(&text);
                if ctx.inline {
                    format!(" {t} ")
                } else if t.is_empty() {
                    String::new()
                } else {
                    format!("\n\n{t}\n\n")
                }
            }
            "dt" => {
                let t = squeeze_all_ws(pystr::strip(&text));
                if ctx.inline {
                    format!(" {t} ")
                } else if t.is_empty() {
                    "\n".into()
                } else {
                    format!("\n\n{t}\n")
                }
            }
            "dd" => {
                let t = pystr::strip(&text);
                if ctx.inline {
                    return format!(" {t} ");
                }
                if t.is_empty() {
                    return "\n".into();
                }
                let indented = indent_lines(t, "    ");
                format!(":{}\n", &indented[1..])
            }
            "hr" => "\n\n---\n\n".into(),
            "img" => self.convert_img(id),
            "video" => self.convert_video(id, text, ctx),
            "input" => match self.dom.attr(id, "type") {
                Some("checkbox") if self.dom.attr(id, "checked").is_some() => "[x] ".into(),
                Some("checkbox") => "[ ] ".into(),
                _ => String::new(),
            },
            "ul" | "ol" => self.convert_list(id, &text, ctx),
            "li" => self.convert_li(id, &text),
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
            "pre" => {
                if text.is_empty() {
                    return String::new();
                }
                // strip_pre: r'^[ \n]*\n' and r'[ \n]*$'
                let lead = text.find(|c| c != ' ' && c != '\n').unwrap_or(text.len());
                let start = text[..lead].rfind('\n').map_or(0, |i| i + 1);
                let body = text[start..].trim_end_matches([' ', '\n']);
                format!("\n\n```\n{body}\n```\n\n")
            }
            "q" => format!("\"{text}\""),
            "script" | "style" => String::new(),
            "table" => format!("\n\n{}\n\n", pystr::strip(&text)),
            "caption" => format!("{}\n\n", pystr::strip(&text)),
            "figcaption" => format!("\n\n{}\n\n", pystr::strip(&text)),
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
                let t = squeeze_all_ws(pystr::strip(&text));
                format!("\n\n{} {}\n\n", "#".repeat(heading_level(n)), t)
            }
            _ => text,
        }
    }

    fn convert_a(&self, id: usize, text: &str) -> String {
        let (prefix, suffix, text) = chomp(text);
        if text.is_empty() {
            return String::new();
        }
        if self.dom.has_ancestor(id, "pre") {
            return text.to_string();
        }
        let title = self.dom.attr(id, "title").filter(|t| !t.is_empty());
        let mut href = self.dom.attr(id, "href").unwrap_or_default().to_string();
        if !href.is_empty() {
            match pyurl::requote(&href) {
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

    fn convert_video(&self, id: usize, text: String, ctx: Ctx) -> String {
        let parent_is_inline_keeper = false; // keep_inline_images_in is empty
        if ctx.inline && !parent_is_inline_keeper {
            return text;
        }
        let mut src = self.dom.attr(id, "src").unwrap_or_default().to_string();
        if src.is_empty() {
            let mut sources = Vec::new();
            self.dom.find_all_by(
                id,
                &|d, c| d.name(c) == Some("source") && d.attr(c, "src").is_some(),
                &mut sources,
            );
            if let Some(&s) = sources.first() {
                src = self.dom.attr(s, "src").unwrap_or_default().to_string();
            }
        }
        let poster = self.dom.attr(id, "poster").unwrap_or_default();
        match (src.is_empty(), poster.is_empty()) {
            (false, false) => format!("[![{text}]({poster})]({src})"),
            (false, true) => format!("[{text}]({src})"),
            (true, false) => format!("![{text}]({poster})"),
            (true, true) => text,
        }
    }

    fn convert_list(&self, id: usize, text: &str, ctx: Ctx) -> String {
        // _next_block_content_sibling: skip comments and whitespace-only text
        let mut next = self.dom.next(id);
        while let Some(n) = next {
            match self.dom.kind(n) {
                Kind::Text(t) if pystr::strip(t).is_empty() => next = self.dom.next(n),
                Kind::Comment(_) => next = self.dom.next(n),
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
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(1);
            let pos = self
                .dom
                .children(p)
                .iter()
                .position(|&c| c == id)
                .unwrap_or(0);
            let before = self.dom.children(p)[..pos]
                .iter()
                .filter(|&&c| self.dom.is_element_named(c, &["li"]))
                .count();
            format!("{}. ", start + before as i64)
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
        let indented = indent_lines(text, &" ".repeat(width));
        format!("{bullet}{}\n", &indented[width..])
    }

    fn colspan(&self, id: usize) -> usize {
        match self.dom.attr(id, "colspan") {
            Some(v) if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) => {
                v.parse::<usize>().unwrap_or(1000).clamp(1, 1000)
            }
            _ => 1,
        }
    }

    fn convert_tr(&self, id: usize, text: &str) -> String {
        let cells = self.dom.find_all(id, &["td", "th"]);
        let parent = self.dom.parent(id).unwrap_or(ROOT);
        let parent_name = self.dom.name(parent).unwrap_or_default();
        let is_first_row = self.dom.prev_element_sibling(id).is_none();
        let is_headrow = cells.iter().all(|&c| self.dom.name(c) == Some("th"))
            || (parent_name == "thead" && self.dom.find_all(parent, &["tr"]).len() == 1);
        let is_head_row_missing = (is_first_row && parent_name != "tbody")
            || (is_first_row
                && parent_name == "tbody"
                && self
                    .dom
                    .parent(parent)
                    .is_none_or(|gp| self.dom.find_all(gp, &["thead"]).is_empty()));
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

/// `re_line_with_content.sub`: prefix every non-empty line.
fn indent_lines(text: &str, indent: &str) -> String {
    text.split('\n')
        .map(|l| {
            if l.is_empty() {
                String::new()
            } else {
                format!("{indent}{l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    let mut run_has_newline = false;
    let mut in_run = false;
    for c in s.chars() {
        if matches!(c, '\t' | ' ' | '\r' | '\n') {
            in_run = true;
            run_has_newline |= c == '\r' || c == '\n';
            continue;
        }
        if in_run {
            out.push(if run_has_newline { '\n' } else { ' ' });
            in_run = false;
            run_has_newline = false;
        }
        out.push(c);
    }
    if in_run {
        out.push(if run_has_newline { '\n' } else { ' ' });
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
