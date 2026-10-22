//! RSS 2.0 and Atom feeds, as markitdown's RssConverter: the feed is read
//! with minidom semantics, HTML carried in fields goes through the HTML
//! parser and markdownify, links are resolved against `xml:base`.

use crate::dom::{Dom, Kind, ROOT};
use crate::xml::{self, Element, Node, ParseOptions};
use crate::{Error, StreamInfo, html, markdownify, pystr, pyurl, text};

const ATOM_NS: &str = "http://www.w3.org/2005/Atom";
const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";
const CONTENT_NS: &str = "http://purl.org/rss/1.0/modules/content/";

const TEXT_BREAK: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "br",
    "dd",
    "div",
    "dl",
    "dt",
    "figcaption",
    "figure",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "td",
    "th",
    "tr",
    "ul",
];

#[derive(Clone, Copy, PartialEq)]
enum ContentKind {
    Text,
    Html,
    Xhtml,
    Binary,
}

fn parse(data: &[u8]) -> Result<Element, Error> {
    // expat decodes by the XML declaration; UTF-8 otherwise
    let decoded;
    let bytes = match declared_xml_encoding(data) {
        Some(enc) if !matches!(enc.as_str(), "utf-8" | "utf8") => {
            decoded = text::decode(data, &enc)
                .ok_or_else(|| Error::Xml(format!("cannot decode feed as {enc}")))?;
            decoded.as_bytes()
        }
        _ => data,
    };
    let opts = ParseOptions {
        namespaces: &[],
        cdata_as_text: true,
        normalize_after_unescape: false,
        raw_names: true,
    };
    xml::parse(bytes, &opts)
}

fn declared_xml_encoding(data: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(&data[..data.len().min(200)]);
    let decl = head
        .strip_prefix('\u{feff}')
        .unwrap_or(&head)
        .strip_prefix("<?xml")?;
    let decl = &decl[..decl.find("?>")?];
    let pos = decl.find("encoding")?;
    let rest = decl[pos + 8..].trim_start().strip_prefix('=')?.trim_start();
    let quote = rest.chars().next()?;
    let value = rest[1..].split(quote).next()?;
    Some(value.to_lowercase())
}

fn local(e: &Element) -> &str {
    e.name.rsplit(':').next().unwrap_or(&e.name)
}

fn ns(e: &Element) -> Option<&str> {
    e.ns.as_deref()
}

/// `_get_children`: same-namespace children, or content:encoded.
fn children<'a>(e: &'a Element, tag: &str) -> Vec<&'a Element> {
    let (want_ns, want_local) = if tag == "content:encoded" {
        (Some(CONTENT_NS), "encoded")
    } else {
        (ns(e), tag)
    };
    e.elements()
        .filter(|c| ns(c) == want_ns && local(c) == want_local)
        .collect()
}

fn child<'a>(e: &'a Element, tag: &str) -> Option<&'a Element> {
    children(e, tag).into_iter().next()
}

fn feed_type(root: &Element) -> Option<&'static str> {
    if &*root.name == "rss" {
        return Some("rss");
    }
    if local(root) == "feed"
        && ns(root).is_none_or(|n| n == ATOM_NS)
        && !children(root, "entry").is_empty()
    {
        return Some("atom");
    }
    None
}

pub fn accepts(data: &[u8], info: &StreamInfo) -> bool {
    let ext = info.extension.as_deref().unwrap_or_default().to_lowercase();
    let mime = info.mimetype.as_deref().unwrap_or_default().to_lowercase();
    if matches!(ext.as_str(), ".rss" | ".atom")
        || ["application/rss", "application/atom"]
            .iter()
            .any(|m| mime.starts_with(m))
    {
        return true;
    }
    if ext == ".xml" || mime.starts_with("text/xml") || mime.starts_with("application/xml") {
        return parse(data).is_ok_and(|r| feed_type(&r).is_some());
    }
    false
}

pub fn convert(data: &[u8], info: &StreamInfo, keep_data_uris: bool) -> Result<String, Error> {
    let root = parse(data)?;
    let doc_uri = info.url.clone().unwrap_or_default();
    let ctx = Ctx {
        root: &root,
        doc_uri,
        keep_data_uris,
    };
    match feed_type(&root) {
        Some("rss") => ctx.rss(),
        Some(_) => Ok(ctx.atom()),
        None => Err(Error::Conversion("Unknown feed type".into())),
    }
}

/// `html.escape(text, quote=False)`
fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `_read_content`: text, encoded HTML, or inline XML serialised for the
/// HTML parser, in document order.
fn read_content(e: &Element, kind: ContentKind) -> String {
    fn walk(e: &Element, kind: ContentKind, direct: bool, out: &mut String) {
        for c in &e.children {
            match c {
                Node::Text(t) => {
                    if kind == ContentKind::Text || (kind == ContentKind::Html && direct) {
                        out.push_str(t);
                    } else {
                        out.push_str(&escape_text(t));
                    }
                }
                Node::Element(c) => {
                    if kind == ContentKind::Text {
                        let brk = TEXT_BREAK.contains(&local(c).to_lowercase().as_str());
                        if brk {
                            out.push('\n');
                        }
                        walk(c, kind, false, out);
                        if brk {
                            out.push('\n');
                        }
                        continue;
                    }
                    let name = if ns(c) == Some(XHTML_NS) {
                        local(c)
                    } else {
                        &c.name
                    };
                    let attrs: String = c
                        .attrs
                        .iter()
                        .map(|(k, v)| format!(" {k}=\"{}\"", html::escape(v)))
                        .collect();
                    if c.children.is_empty() {
                        out.push_str(&format!("<{name}{attrs}/>"));
                    } else {
                        out.push_str(&format!("<{name}{attrs}>"));
                        walk(c, kind, false, out);
                        out.push_str(&format!("</{name}>"));
                    }
                }
            }
        }
    }
    let mut out = String::new();
    walk(e, kind, true, &mut out);
    out
}

fn atom_kind(t: &str, allow_media: bool) -> ContentKind {
    let t = pystr::strip(t).to_lowercase();
    match t.as_str() {
        "" | "text" => return ContentKind::Text,
        "html" => return ContentKind::Html,
        "xhtml" => return ContentKind::Xhtml,
        _ => {}
    }
    if !allow_media {
        return ContentKind::Text;
    }
    let media = pystr::strip(t.split(';').next().unwrap_or_default()).to_string();
    if media == "text/html" {
        ContentKind::Html
    } else if media.ends_with("+xml") || media.ends_with("/xml") {
        ContentKind::Xhtml
    } else if media.starts_with("text/") {
        ContentKind::Text
    } else {
        ContentKind::Binary
    }
}

struct Ctx<'a> {
    root: &'a Element,
    doc_uri: String,
    keep_data_uris: bool,
}

impl Ctx<'_> {
    fn data(&self, e: &Element, tag: &str, kind: ContentKind) -> Option<String> {
        child(e, tag)
            .map(|n| read_content(n, kind))
            .filter(|s| !s.is_empty())
    }

    /// `_get_atom_content`: (value, is_markup)
    fn atom_content(&self, entry: &Element, tag: &str) -> (Option<String>, bool) {
        let Some(node) = child(entry, tag) else {
            return (None, true);
        };
        let kind = atom_kind(node.attr("type").unwrap_or_default(), tag == "content");
        if kind == ContentKind::Binary {
            return (None, true);
        }
        let mut text = read_content(node, kind);
        if kind != ContentKind::Text {
            return ((!text.is_empty()).then_some(text), true);
        }
        let lines = pystr::splitlines(&text);
        if lines.len() > 1 {
            text = format!("{}\n{}", lines[0], pystr::dedent(&lines[1..].join("\n")));
        }
        (Some(pystr::strip(&text).to_string()), false)
    }

    /// `_get_flattened_text`: one line, layout whitespace dropped.
    fn flattened(&self, e: &Element, tag: &str, atom_text: bool) -> Option<String> {
        let value = if atom_text {
            let (value, markup) = self.atom_content(e, tag);
            match value {
                Some(v) if markup && !v.is_empty() => Some(html_text_with_breaks(&v)),
                v => v,
            }
        } else {
            self.data(e, tag, ContentKind::Text)
        }?;
        let parts: Vec<&str> = pystr::splitlines(&value)
            .into_iter()
            .map(pystr::strip)
            .filter(|p| !p.is_empty())
            .collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    }

    /// `_get_field_base_url`: apply xml:base from the root to the field.
    fn base_url(&self, e: &Element, tag: &str) -> String {
        let mut path = Vec::new();
        if let Some(field) = child(e, tag) {
            find_path(self.root, field, &mut path);
        }
        let mut base = self.doc_uri.clone();
        for node in path {
            if let Some(b) = node.attr("xml:base") {
                base = pyurl::urljoin(&base, b).unwrap_or_else(|_| b.to_string());
            }
        }
        base
    }

    fn parse_content(&self, content: &str, base: &str) -> String {
        let mut dom = html::parser::parse(content);
        resolve_links(&mut dom, base);
        markdownify::convert(
            &dom,
            ROOT,
            &markdownify::Options {
                keep_data_uris: self.keep_data_uris,
            },
        )
    }

    fn body(&self, parts: Vec<String>, md: &mut String) {
        let body = parts
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        if !body.is_empty() && !md.is_empty() && !md.ends_with('\n') {
            md.push_str("\n\n");
        }
        md.push_str(&body);
    }

    fn rss(&self) -> Result<String, Error> {
        let channel = child(self.root, "channel")
            .ok_or_else(|| Error::Conversion("No channel found in RSS feed".into()))?;
        let mut md = String::new();
        if let Some(t) = self.flattened(channel, "title", false) {
            md.push_str(&format!("# {t}\n"));
        }
        if let Some(d) = self.data(channel, "description", ContentKind::Text) {
            md.push_str(&format!("{d}\n"));
        }
        for item in children(channel, "item") {
            let title = self.flattened(item, "title", false);
            let description = self.data(item, "description", ContentKind::Html);
            let pub_date = self.flattened(item, "pubDate", false);
            let content = self.data(item, "content:encoded", ContentKind::Html);
            if let Some(t) = title {
                md.push_str(&format!("\n## {t}\n"));
            }
            if let Some(p) = pub_date {
                md.push_str(&format!("Published on: {p}\n"));
            }
            let mut parts = Vec::new();
            for (value, tag) in [(description, "description"), (content, "content:encoded")] {
                if let Some(v) = value {
                    parts.push(self.parse_content(&v, &self.base_url(item, tag)));
                }
            }
            self.body(parts, &mut md);
        }
        Ok(md)
    }

    fn atom(&self) -> String {
        let root = self.root;
        let title = self.flattened(root, "title", true);
        let subtitle = self.flattened(root, "subtitle", true);
        let mut md = title.map(|t| format!("# {t}\n")).unwrap_or_default();
        if let Some(s) = subtitle {
            md.push_str(&format!("{s}\n"));
        }
        for entry in children(root, "entry") {
            let entry_title = self.flattened(entry, "title", true);
            let summary = self.atom_content(entry, "summary");
            let updated = self.flattened(entry, "updated", false);
            let content = self.atom_content(entry, "content");
            if let Some(t) = entry_title {
                md.push_str(&format!("\n## {t}\n"));
            }
            if let Some(u) = updated {
                md.push_str(&format!("Updated on: {u}\n"));
            }
            let mut parts = Vec::new();
            for ((value, markup), tag) in [(summary, "summary"), (content, "content")] {
                let Some(v) = value.filter(|v| !v.is_empty()) else {
                    continue;
                };
                parts.push(if markup {
                    self.parse_content(&v, &self.base_url(entry, tag))
                } else {
                    v
                });
            }
            self.body(parts, &mut md);
        }
        md
    }
}

/// Ancestors of `target` from the root down to it, inclusive.
fn find_path<'a>(e: &'a Element, target: &Element, path: &mut Vec<&'a Element>) -> bool {
    path.push(e);
    if std::ptr::eq(e, target) {
        return true;
    }
    for c in e.elements() {
        if find_path(c, target, path) {
            return true;
        }
    }
    path.pop();
    false
}

/// `_resolve_content_links`: absolutise a/href and img/src, data-src.
fn resolve_links(dom: &mut Dom, base: &str) {
    let mut stack = vec![(ROOT, base.to_string())];
    while let Some((id, inherited)) = stack.pop() {
        let current = match dom.attr(id, "xml:base") {
            Some(o) => pyurl::urljoin(&inherited, o).unwrap_or_else(|_| o.to_string()),
            None => inherited,
        };
        let attrs: &[&str] = match dom.name(id) {
            Some("a") => &["href"],
            Some("img") => &["src", "data-src"],
            _ => &[],
        };
        for &attr in attrs {
            if let Some(r) = dom.attr(id, attr).map(str::to_string)
                && (!r.is_empty() || attr == "href")
            {
                let resolved = pyurl::urljoin(&current, &r).unwrap_or(r);
                dom.set_attr(id, attr, resolved);
            }
        }
        for &c in dom.children(id) {
            if matches!(dom.kind(c), Kind::Element { .. }) {
                stack.push((c, current.clone()));
            }
        }
    }
}

/// Atom text constructs in markup: line breaks around block elements, then
/// `soup.get_text()`.
fn html_text_with_breaks(value: &str) -> String {
    let mut dom = html::parser::parse(value);
    for b in dom.find_all(ROOT, TEXT_BREAK) {
        dom.insert_text_before(b, "\n".into());
        dom.append_text(b, "\n".into());
    }
    dom.get_text(ROOT, "", false)
}
