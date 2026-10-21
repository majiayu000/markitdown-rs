//! Document model -> HTML node tree, following mammoth's `conversion.py`
//! with markitdown's style map (`u => u` plus mammoth's defaults), then
//! mammoth's empty-element stripping and collapsing of adjacent elements.

use super::reader::{BreakKind, DocEl, NumLevel, Run};
use crate::markdownify::Dom;

pub struct Note {
    pub note_type: &'static str,
    pub id: String,
    pub body: Vec<DocEl>,
}

enum HNode {
    El(HEl),
    Text(String),
    ForceWrite,
}

struct HEl {
    /// the first name is the tag written; others only matter for collapsing
    names: &'static [&'static str],
    /// sorted by key, as mammoth writes them
    attrs: Vec<(String, String)>,
    children: Vec<HNode>,
    collapsible: bool,
}

#[derive(Clone, Copy)]
struct PathEl {
    names: &'static [&'static str],
    fresh: bool,
}

const fn el(names: &'static [&'static str], fresh: bool) -> PathEl {
    PathEl { names, fresh }
}

fn element(
    names: &'static [&'static str],
    mut attrs: Vec<(String, String)>,
    children: Vec<HNode>,
) -> HNode {
    attrs.sort();
    HNode::El(HEl {
        names,
        attrs,
        children,
        collapsible: false,
    })
}

fn wrap(path: &[PathEl], mut nodes: Vec<HNode>) -> Vec<HNode> {
    for p in path.iter().rev() {
        nodes = vec![HNode::El(HEl {
            names: p.names,
            attrs: Vec::new(),
            children: nodes,
            collapsible: !p.fresh,
        })];
    }
    nodes
}

const UL_OL: &[&str] = &["ul", "ol"];

fn list_path(level: usize, ordered: bool) -> Vec<PathEl> {
    let mut path = Vec::new();
    for _ in 0..level {
        path.push(el(UL_OL, false));
        path.push(el(&["li"], false));
    }
    path.push(el(if ordered { &["ol"] } else { &["ul"] }, false));
    path.push(el(&["li"], true));
    path
}

fn eq_ci(a: Option<&str>, b: &str) -> bool {
    a.is_some_and(|a| a.to_uppercase() == b.to_uppercase())
}

const HEADINGS: [&[&str]; 6] = [&["h1"], &["h2"], &["h3"], &["h4"], &["h5"], &["h6"]];

fn paragraph_path(
    style_id: Option<&str>,
    style_name: Option<&str>,
    numbering: Option<&NumLevel>,
) -> Vec<PathEl> {
    for (i, h) in HEADINGS.iter().enumerate() {
        if style_id == Some(format!("Heading{}", i + 1).as_str()) {
            return vec![el(h, true)];
        }
    }
    for (i, h) in HEADINGS.iter().enumerate() {
        if eq_ci(style_name, &format!("Heading {}", i + 1)) {
            return vec![el(h, true)];
        }
    }
    if style_id == Some("Heading") || eq_ci(style_name, "Heading") {
        return vec![el(HEADINGS[0], true)];
    }
    let note_styles = [
        "footnote text",
        "endnote text",
        "annotation text",
        "Footnote",
        "Endnote",
    ];
    if !note_styles.iter().any(|s| eq_ci(style_name, s))
        && let Some(n) = numbering
        && let Ok(level) = n.level_index.parse::<usize>()
        && level < 5
        && n.level_index == level.to_string()
    {
        return list_path(level, n.is_ordered);
    }
    vec![el(&["p"], true)]
}

pub struct Converter<'a> {
    notes: &'a [Note],
    note_refs: Vec<(&'static str, String)>,
    keep_data_uris: bool,
    image_data: &'a dyn Fn(&str) -> Option<Vec<u8>>,
}

impl<'a> Converter<'a> {
    pub fn new(
        notes: &'a [Note],
        keep_data_uris: bool,
        image_data: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> Self {
        Converter {
            notes,
            note_refs: Vec::new(),
            keep_data_uris,
            image_data,
        }
    }

    /// Convert the document body (plus referenced notes) into `dom`'s root.
    pub fn convert_document(mut self, body: &[DocEl], dom: &mut Dom) {
        let mut nodes = self.visit_all(body, false);
        let refs = self.note_refs.clone();
        let mut items = Vec::new();
        for (typ, id) in refs {
            if let Some(note) = self.notes.iter().find(|n| n.note_type == typ && n.id == id) {
                items.extend(self.visit_note(note));
            }
        }
        nodes.push(element(&["ol"], Vec::new(), items));
        let nodes = collapse(strip_empty(nodes));
        write_dom(&nodes, dom, crate::markdownify::ROOT);
    }

    fn visit_all(&mut self, els: &[DocEl], header: bool) -> Vec<HNode> {
        els.iter().flat_map(|e| self.visit(e, header)).collect()
    }

    fn visit(&mut self, e: &DocEl, header: bool) -> Vec<HNode> {
        match e {
            DocEl::Paragraph {
                children,
                style_id,
                style_name,
                numbering,
            } => {
                let path = paragraph_path(
                    style_id.as_deref(),
                    style_name.as_deref(),
                    numbering.as_ref(),
                );
                let content = self.visit_all(children, header);
                wrap(&path, content)
            }
            DocEl::Run(r) => self.visit_run(r, header),
            DocEl::Text(t) => vec![HNode::Text(t.clone())],
            DocEl::Tab => vec![HNode::Text("\t".into())],
            DocEl::Hyperlink {
                children,
                href,
                anchor,
                target_frame,
            } => {
                let href = match anchor {
                    Some(a) => format!("#{a}"),
                    None => href.clone().unwrap_or_default(),
                };
                let mut attrs = vec![("href".to_string(), href)];
                if let Some(t) = target_frame {
                    attrs.push(("target".into(), t.clone()));
                }
                attrs.sort();
                let children = self.visit_all(children, header);
                vec![HNode::El(HEl {
                    names: &["a"],
                    attrs,
                    children,
                    collapsible: true,
                })]
            }
            DocEl::Checkbox(checked) => {
                let mut attrs = vec![("type".to_string(), "checkbox".to_string())];
                if *checked {
                    attrs.push(("checked".into(), "checked".into()));
                }
                vec![element(&["input"], attrs, Vec::new())]
            }
            DocEl::Bookmark(name) => vec![HNode::El(HEl {
                names: &["a"],
                attrs: vec![("id".into(), name.clone())],
                children: vec![HNode::ForceWrite],
                collapsible: true,
            })],
            DocEl::Table { children, .. } => {
                let body_index = children
                    .iter()
                    .position(|c| {
                        !matches!(
                            c,
                            DocEl::TableRow {
                                is_header: true,
                                ..
                            }
                        )
                    })
                    .unwrap_or(children.len());
                let mut inner = vec![HNode::ForceWrite];
                if body_index == 0 {
                    inner.extend(self.visit_all(children, false));
                } else {
                    let head = self.visit_all(&children[..body_index], true);
                    let rows = self.visit_all(&children[body_index..], false);
                    inner.push(element(&["thead"], Vec::new(), head));
                    inner.push(element(&["tbody"], Vec::new(), rows));
                }
                wrap(&[el(&["table"], true)], inner)
            }
            DocEl::TableRow { children, .. } => {
                let mut inner = vec![HNode::ForceWrite];
                inner.extend(self.visit_all(children, header));
                vec![element(&["tr"], Vec::new(), inner)]
            }
            DocEl::TableCell {
                children,
                colspan,
                rowspan,
                ..
            } => {
                let mut attrs = Vec::new();
                if *colspan != 1 {
                    attrs.push(("colspan".to_string(), colspan.to_string()));
                }
                if *rowspan != 1 {
                    attrs.push(("rowspan".to_string(), rowspan.to_string()));
                }
                let mut inner = vec![HNode::ForceWrite];
                inner.extend(self.visit_all(children, header));
                vec![element(
                    if header { &["th"] } else { &["td"] },
                    attrs,
                    inner,
                )]
            }
            DocEl::Break(BreakKind::Line) => wrap(&[el(&["br"], true)], Vec::new()),
            DocEl::Break(_) => Vec::new(),
            DocEl::Image {
                alt,
                content_type,
                path,
            } => self.visit_image(alt.as_deref(), content_type.as_deref(), path),
            DocEl::NoteRef { note_type, id } => {
                self.note_refs.push((note_type, id.clone()));
                let n = self.note_refs.len();
                let a = element(
                    &["a"],
                    vec![
                        ("href".into(), format!("#{note_type}-{id}")),
                        ("id".into(), format!("{note_type}-ref-{id}")),
                    ],
                    vec![HNode::Text(format!("[{n}]"))],
                );
                vec![element(&["sup"], Vec::new(), vec![a])]
            }
        }
    }

    fn visit_run(&mut self, r: &Run, header: bool) -> Vec<HNode> {
        let mut nodes = self.visit_all(&r.children, header);
        // innermost first, as mammoth applies them
        let mut paths: Vec<PathEl> = Vec::new();
        if r.strike {
            paths.push(el(&["s"], false));
        }
        if r.underline {
            paths.push(el(&["u"], false));
        }
        match r.vert_align.as_deref() {
            Some("subscript") => paths.push(el(&["sub"], false)),
            Some("superscript") => paths.push(el(&["sup"], false)),
            _ => {}
        }
        if r.italic {
            paths.push(el(&["em"], false));
        }
        if r.bold {
            paths.push(el(&["strong"], false));
        }
        if eq_ci(r.style_name.as_deref(), "Strong") {
            paths.push(el(&["strong"], false));
        }
        for p in paths {
            nodes = wrap(&[p], nodes);
        }
        nodes
    }

    fn visit_image(&self, alt: Option<&str>, content_type: Option<&str>, path: &str) -> Vec<HNode> {
        let ct = content_type.unwrap_or("None");
        let data = if self.keep_data_uris {
            match (self.image_data)(path) {
                Some(bytes) => crate::base64::encode(&bytes),
                // mammoth fails on a missing image part; keep the reference
                None => String::new(),
            }
        } else {
            String::new()
        };
        let mut attrs = vec![("src".to_string(), format!("data:{ct};base64,{data}"))];
        if let Some(a) = alt.filter(|a| !a.is_empty()) {
            attrs.push(("alt".into(), a.to_string()));
        }
        vec![element(&["img"], attrs, Vec::new())]
    }

    fn visit_note(&mut self, note: &Note) -> Vec<HNode> {
        let mut body = self.visit_all(&note.body, false);
        let back = element(
            &["a"],
            vec![(
                "href".into(),
                format!("#{}-ref-{}", note.note_type, note.id),
            )],
            vec![HNode::Text("\u{2191}".into())],
        );
        body.push(HNode::El(HEl {
            names: &["p"],
            attrs: Vec::new(),
            children: vec![HNode::Text(" ".into()), back],
            collapsible: true,
        }));
        vec![element(
            &["li"],
            vec![("id".into(), format!("{}-{}", note.note_type, note.id))],
            body,
        )]
    }
}

fn is_void(e: &HEl) -> bool {
    e.children.is_empty() && matches!(e.names[0], "br" | "hr" | "img" | "input")
}

fn strip_empty(nodes: Vec<HNode>) -> Vec<HNode> {
    nodes
        .into_iter()
        .filter_map(|n| match n {
            HNode::Text(t) if t.is_empty() => None,
            HNode::El(mut e) => {
                e.children = strip_empty(e.children);
                if e.children.is_empty() && !matches!(e.names[0], "br" | "hr" | "img" | "input") {
                    None
                } else {
                    Some(HNode::El(e))
                }
            }
            other => Some(other),
        })
        .collect()
}

fn collapse(nodes: Vec<HNode>) -> Vec<HNode> {
    let mut out = Vec::new();
    for n in nodes {
        collapsing_add(&mut out, n);
    }
    out
}

fn collapsing_add(out: &mut Vec<HNode>, node: HNode) {
    let node = match node {
        HNode::El(mut e) => {
            e.children = collapse(e.children);
            HNode::El(e)
        }
        other => other,
    };
    if let (Some(HNode::El(last)), HNode::El(e)) = (out.last_mut(), &node)
        && e.collapsible
        && e.names.contains(&last.names[0])
        && last.attrs == e.attrs
    {
        let HNode::El(e) = node else { unreachable!() };
        for c in e.children {
            collapsing_add(&mut last.children, c);
        }
        return;
    }
    out.push(node);
}

/// What html.parser builds from mammoth's serialised HTML.
fn write_dom(nodes: &[HNode], dom: &mut Dom, parent: usize) {
    for n in nodes {
        match n {
            HNode::Text(t) => dom.text(parent, t),
            HNode::ForceWrite => {}
            HNode::El(e) => {
                debug_assert!(!is_void(e) || e.children.is_empty());
                let id = dom.element(parent, e.names[0], e.attrs.clone());
                write_dom(&e.children, dom, id);
            }
        }
    }
}
