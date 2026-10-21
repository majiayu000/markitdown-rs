//! WordprocessingML body -> document model, following mammoth 1.11's
//! `docx/body_xml.py` element by element (including what it ignores).

use super::dingbats::DINGBATS;
use super::package::{ContentTypes, Numbering, Relationships, Styles};
use crate::pystr;
use crate::xml::{Element, Node};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct NumLevel {
    pub level_index: String,
    pub is_ordered: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BreakKind {
    Line,
    Page,
    Column,
}

#[derive(Debug, Clone)]
pub enum DocEl {
    Paragraph {
        children: Vec<DocEl>,
        style_id: Option<String>,
        style_name: Option<String>,
        numbering: Option<NumLevel>,
    },
    Run(Run),
    Text(String),
    Tab,
    Hyperlink {
        children: Vec<DocEl>,
        href: Option<String>,
        anchor: Option<String>,
        target_frame: Option<String>,
    },
    Checkbox(bool),
    Table {
        children: Vec<DocEl>,
    },
    TableRow {
        children: Vec<DocEl>,
        is_header: bool,
    },
    TableCell {
        children: Vec<DocEl>,
        colspan: u32,
        rowspan: u32,
        vmerge: bool,
    },
    Break(BreakKind),
    Image {
        alt: Option<String>,
        content_type: Option<String>,
        /// zip entry of the embedded image
        path: String,
    },
    Bookmark(String),
    NoteRef {
        note_type: &'static str,
        id: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct Run {
    pub children: Vec<DocEl>,
    pub style_id: Option<String>,
    pub style_name: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub vert_align: Option<String>,
}

#[derive(Default)]
struct ReadResult {
    elements: Vec<DocEl>,
    extra: Vec<DocEl>,
}

impl ReadResult {
    fn one(el: DocEl) -> Self {
        ReadResult {
            elements: vec![el],
            extra: Vec::new(),
        }
    }
}

enum Field<'a> {
    Begin(&'a Element),
    Hyperlink {
        href: Option<String>,
        anchor: Option<String>,
    },
    Checkbox(bool),
    Other,
}

pub struct Ctx<'p> {
    pub styles: &'p Styles,
    pub numbering: &'p Numbering,
    pub content_types: &'p ContentTypes,
    pub rels: &'p Relationships,
}

pub struct BodyReader<'a, 'p> {
    ctx: Ctx<'p>,
    instr_text: Vec<String>,
    fields: Vec<Field<'a>>,
    deleted_paragraph_contents: Vec<&'a Node>,
}

fn boolean_attr(v: Option<&str>) -> bool {
    !matches!(v, Some("false" | "0"))
}

fn boolean_element(e: Option<&Element>) -> bool {
    e.is_some_and(|e| boolean_attr(e.attr("w:val")))
}

impl<'a, 'p> BodyReader<'a, 'p> {
    pub fn new(ctx: Ctx<'p>) -> Self {
        BodyReader {
            ctx,
            instr_text: Vec::new(),
            fields: Vec::new(),
            deleted_paragraph_contents: Vec::new(),
        }
    }

    pub fn read_all(&mut self, nodes: &'a [Node]) -> Vec<DocEl> {
        let r = self.read_nodes(nodes.iter());
        let mut out = r.elements;
        out.extend(r.extra);
        out
    }

    fn read_nodes(&mut self, nodes: impl Iterator<Item = &'a Node>) -> ReadResult {
        let mut out = ReadResult::default();
        for n in nodes {
            if let Node::Element(e) = n {
                let r = self.read(e);
                out.elements.extend(r.elements);
                out.extra.extend(r.extra);
            }
        }
        out
    }

    fn read_children(&mut self, e: Option<&'a Element>) -> ReadResult {
        match e {
            Some(e) => self.read_nodes(e.children.iter()),
            None => ReadResult::default(),
        }
    }

    fn read(&mut self, e: &'a Element) -> ReadResult {
        match e.name.as_str() {
            "w:t" => ReadResult::one(DocEl::Text(e.inner_text())),
            "w:r" => self.run(e),
            "w:p" => self.paragraph(e),
            "w:fldChar" => self.fld_char(e),
            "w:instrText" => {
                self.instr_text.push(e.inner_text());
                ReadResult::default()
            }
            "w:tab" => ReadResult::one(DocEl::Tab),
            "w:noBreakHyphen" => ReadResult::one(DocEl::Text("\u{2011}".into())),
            "w:softHyphen" => ReadResult::one(DocEl::Text("\u{ad}".into())),
            "w:sym" => self.symbol(e),
            "w:tbl" => self.table(e),
            "w:tr" => self.table_row(e),
            "w:tc" => self.table_cell(e),
            "w:ins" | "w:object" | "w:smartTag" | "w:drawing" | "v:group" | "v:rect"
            | "v:roundrect" | "v:shape" | "v:textbox" | "w:txbxContent" => {
                self.read_children(Some(e))
            }
            "w:pict" => {
                let r = self.read_children(Some(e));
                let mut extra = r.extra;
                extra.extend(r.elements);
                ReadResult {
                    elements: Vec::new(),
                    extra,
                }
            }
            "w:hyperlink" => self.hyperlink(e),
            "w:bookmarkStart" => match e.attr("w:name") {
                Some("_GoBack") => ReadResult::default(),
                name => ReadResult::one(DocEl::Bookmark(name.unwrap_or("None").to_string())),
            },
            "w:br" => match e.attr("w:type") {
                None | Some("" | "textWrapping") => ReadResult::one(DocEl::Break(BreakKind::Line)),
                Some("page") => ReadResult::one(DocEl::Break(BreakKind::Page)),
                Some("column") => ReadResult::one(DocEl::Break(BreakKind::Column)),
                Some(_) => ReadResult::default(),
            },
            "wp:inline" | "wp:anchor" => self.inline_image(e),
            // mammoth looks up "o:title", but the o: namespace is not in its
            // table, so VML images never get alt text
            "v:imagedata" => match e.attr("r:id") {
                Some(id) => self.embedded_image(id, None),
                None => ReadResult::default(),
            },
            "w:footnoteReference" => self.note_ref("footnote", e),
            "w:endnoteReference" => self.note_ref("endnote", e),
            "mc:AlternateContent" => self.read_children(e.child("mc:Fallback")),
            "w:sdt" => self.sdt(e),
            // comment references render to nothing with the default style map
            _ => ReadResult::default(),
        }
    }

    fn style(
        &self,
        props: Option<&Element>,
        tag: &str,
        find: impl Fn(&str) -> Option<String>,
    ) -> (Option<String>, Option<String>) {
        let id = props
            .and_then(|p| p.child(tag))
            .and_then(|s| s.attr("w:val"))
            .map(str::to_string);
        let name = id.as_deref().and_then(find);
        (id, name)
    }

    fn run(&mut self, e: &'a Element) -> ReadResult {
        let props = e.child("w:rPr");
        let prop = |name: &str| props.and_then(|p| p.child(name));
        let mut run = Run {
            vert_align: prop("w:vertAlign")
                .and_then(|v| v.attr("w:val"))
                .map(str::to_string),
            bold: boolean_element(prop("w:b")),
            italic: boolean_element(prop("w:i")),
            underline: prop("w:u")
                .is_some_and(|u| !matches!(u.attr("w:val"), None | Some("false" | "0" | "none"))),
            strike: boolean_element(prop("w:strike")),
            ..Run::default()
        };
        let styles = self.ctx.styles;
        (run.style_id, run.style_name) =
            self.style(props, "w:rStyle", |id| styles.character_name(id));
        let mut r = self.read_children(Some(e));
        if let Some((href, anchor)) = self.current_hyperlink() {
            let children = std::mem::take(&mut r.elements);
            r.elements = vec![DocEl::Hyperlink {
                children,
                href,
                anchor,
                target_frame: None,
            }];
        }
        run.children = r.elements;
        ReadResult {
            elements: vec![DocEl::Run(run)],
            extra: r.extra,
        }
    }

    fn current_hyperlink(&self) -> Option<(Option<String>, Option<String>)> {
        self.fields.iter().rev().find_map(|f| match f {
            Field::Hyperlink { href, anchor } => Some((href.clone(), anchor.clone())),
            _ => None,
        })
    }

    fn paragraph(&mut self, e: &'a Element) -> ReadResult {
        let props = e.child("w:pPr");
        if props
            .and_then(|p| p.child("w:rPr"))
            .and_then(|r| r.child("w:del"))
            .is_some()
        {
            self.deleted_paragraph_contents.extend(e.children.iter());
            return ReadResult::default();
        }
        let styles = self.ctx.styles;
        let (style_id, style_name) = self.style(props, "w:pStyle", |id| styles.paragraph_name(id));
        let mut r = if self.deleted_paragraph_contents.is_empty() {
            self.read_nodes(e.children.iter())
        } else {
            let mut nodes = std::mem::take(&mut self.deleted_paragraph_contents);
            nodes.extend(e.children.iter());
            self.read_nodes(nodes.into_iter())
        };
        let numbering =
            self.numbering_properties(style_id.as_deref(), props.and_then(|p| p.child("w:numPr")));
        let p = DocEl::Paragraph {
            children: std::mem::take(&mut r.elements),
            style_id,
            style_name,
            numbering,
        };
        let mut elements = vec![p];
        elements.append(&mut r.extra);
        ReadResult {
            elements,
            extra: Vec::new(),
        }
    }

    fn numbering_properties(
        &self,
        style_id: Option<&str>,
        num_pr: Option<&Element>,
    ) -> Option<NumLevel> {
        let val = |name: &str| {
            num_pr
                .and_then(|n| n.child(name))
                .and_then(|c| c.attr("w:val"))
        };
        let num_id = val("w:numId");
        if let (Some(id), Some(level)) = (num_id, val("w:ilvl")) {
            return self.ctx.numbering.find_level(id, level);
        }
        if let Some(level) =
            style_id.and_then(|s| self.ctx.numbering.find_level_by_paragraph_style(s))
        {
            return Some(level);
        }
        num_id.and_then(|id| self.ctx.numbering.find_level(id, "0"))
    }

    fn fld_char(&mut self, e: &'a Element) -> ReadResult {
        match e.attr("w:fldCharType") {
            Some("begin") => {
                self.fields.push(Field::Begin(e));
                self.instr_text.clear();
            }
            Some("end") => {
                // mammoth 1.11 raises IndexError on an unmatched end; skip it instead
                let field = match self.fields.pop() {
                    Some(Field::Begin(fld)) => self.parse_instr_text(Some(fld)),
                    Some(f) => f,
                    None => Field::Other,
                };
                if let Field::Checkbox(checked) = field {
                    return ReadResult::one(DocEl::Checkbox(checked));
                }
            }
            Some("separate") => {
                let fld = match self.fields.pop() {
                    Some(Field::Begin(fld)) => Some(fld),
                    _ => None,
                };
                let field = self.parse_instr_text(fld);
                self.fields.push(field);
            }
            _ => {}
        }
        ReadResult::default()
    }

    fn parse_instr_text(&self, fld_char: Option<&'a Element>) -> Field<'a> {
        let instr = self.instr_text.concat();
        let ws = pystr::is_space;
        let rest = instr.trim_start_matches(ws);
        if let Some(r) = rest.strip_prefix("HYPERLINK") {
            // \s*HYPERLINK "(.*)"
            if let Some(href) = r.strip_prefix(' ').and_then(quoted_greedy) {
                return Field::Hyperlink {
                    href: Some(href),
                    anchor: None,
                };
            }
            // \s*HYPERLINK\s+\\l\s+"(.*)"
            let after_ws = |s: &'_ str| -> Option<String> {
                let t = s.trim_start_matches(ws);
                (t.len() < s.len()).then(|| t.to_string())
            };
            let anchor = after_ws(r)
                .and_then(|t| t.strip_prefix("\\l").map(str::to_string))
                .and_then(|t| after_ws(&t))
                .and_then(|t| quoted_greedy(&t));
            if let Some(anchor) = anchor {
                return Field::Hyperlink {
                    href: None,
                    anchor: Some(anchor),
                };
            }
        }
        if rest.starts_with("FORMCHECKBOX") {
            let cb = fld_char.and_then(|f| f.path(&["w:ffData", "w:checkBox"]));
            let checked = match cb.and_then(|c| c.child("w:checked")) {
                Some(c) => boolean_element(Some(c)),
                None => boolean_element(cb.and_then(|c| c.child("w:default"))),
            };
            return Field::Checkbox(checked);
        }
        Field::Other
    }

    fn symbol(&mut self, e: &Element) -> ReadResult {
        let font = e.attr("w:font").unwrap_or_default();
        let Some(chr) = e.attr("w:char") else {
            return ReadResult::default();
        };
        let lookup = |code: u32| {
            DINGBATS
                .binary_search_by(|(f, c, _)| (*f, *c).cmp(&(font, code)))
                .ok()
                .map(|i| DINGBATS[i].2)
        };
        let mut cp = u32::from_str_radix(chr, 16).ok().and_then(lookup);
        if cp.is_none() && chr.starts_with("F0") && chr.chars().count() >= 4 {
            cp = u32::from_str_radix(&chr[2..], 16).ok().and_then(lookup);
        }
        match cp.and_then(char::from_u32) {
            Some(c) => ReadResult::one(DocEl::Text(c.to_string())),
            None => ReadResult::default(),
        }
    }

    fn table(&mut self, e: &'a Element) -> ReadResult {
        // table styles only matter for custom style maps
        let mut r = self.read_children(Some(e));
        calculate_row_spans(&mut r.elements);
        let t = DocEl::Table {
            children: r.elements,
        };
        ReadResult {
            elements: vec![t],
            extra: r.extra,
        }
    }

    fn table_row(&mut self, e: &'a Element) -> ReadResult {
        let props = e.child("w:trPr");
        if props.and_then(|p| p.child("w:del")).is_some() {
            return ReadResult::default();
        }
        let is_header = props.and_then(|p| p.child("w:tblHeader")).is_some();
        let r = self.read_children(Some(e));
        ReadResult {
            elements: vec![DocEl::TableRow {
                children: r.elements,
                is_header,
            }],
            extra: r.extra,
        }
    }

    fn table_cell(&mut self, e: &'a Element) -> ReadResult {
        let props = e.child("w:tcPr");
        let colspan = props
            .and_then(|p| p.child("w:gridSpan"))
            .and_then(|g| g.attr("w:val"))
            .and_then(|v| pystr::strip(v).parse().ok())
            .unwrap_or(1);
        let vmerge = props
            .and_then(|p| p.child("w:vMerge"))
            .is_some_and(|v| matches!(v.attr("w:val"), None | Some("" | "continue")));
        let r = self.read_children(Some(e));
        let cell = DocEl::TableCell {
            children: r.elements,
            colspan,
            rowspan: 1,
            vmerge,
        };
        ReadResult {
            elements: vec![cell],
            extra: r.extra,
        }
    }

    fn hyperlink(&mut self, e: &'a Element) -> ReadResult {
        let target_frame = e
            .attr("w:tgtFrame")
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        let anchor = e.attr("w:anchor").map(str::to_string);
        let href = e.attr("r:id").and_then(|id| self.ctx.rels.target(id));
        let r = self.read_children(Some(e));
        let (href, anchor) = match (href, anchor) {
            (Some(h), Some(a)) => (Some(replace_fragment(h, &a)), None),
            (Some(h), None) => (Some(h.to_string()), None),
            (None, Some(a)) => (None, Some(a)),
            (None, None) => return r,
        };
        let link = DocEl::Hyperlink {
            children: r.elements,
            href,
            anchor,
            target_frame,
        };
        ReadResult {
            elements: vec![link],
            extra: r.extra,
        }
    }

    fn inline_image(&mut self, e: &Element) -> ReadResult {
        let doc_pr = e.child("wp:docPr");
        let descr = doc_pr.and_then(|d| d.attr("descr"));
        let alt = match descr {
            Some(d) if !pystr::strip(d).is_empty() => Some(d.to_string()),
            _ => doc_pr.and_then(|d| d.attr("title")).map(str::to_string),
        };
        let mut out = ReadResult::default();
        for graphic in e.children_named("a:graphic") {
            for data in graphic.children_named("a:graphicData") {
                for pic in data.children_named("pic:pic") {
                    for fill in pic.children_named("pic:blipFill") {
                        for blip in fill.children_named("a:blip") {
                            if let Some(id) = blip.attr("r:embed") {
                                out.elements
                                    .extend(self.embedded_image(id, alt.clone()).elements);
                            }
                            // r:link (external) images are dropped: mammoth runs with
                            // external file access disabled
                        }
                    }
                }
            }
        }
        out
    }

    fn embedded_image(&self, rel_id: &str, alt: Option<String>) -> ReadResult {
        let Some(target) = self.ctx.rels.target(rel_id) else {
            return ReadResult::default();
        };
        let path = match target.strip_prefix('/') {
            Some(abs) => abs.to_string(),
            None => format!("word/{target}"),
        };
        let content_type = self.ctx.content_types.find(&path);
        ReadResult::one(DocEl::Image {
            alt,
            content_type,
            path,
        })
    }

    fn note_ref(&self, note_type: &'static str, e: &Element) -> ReadResult {
        let id = e.attr("w:id").unwrap_or_default().to_string();
        ReadResult::one(DocEl::NoteRef { note_type, id })
    }

    fn sdt(&mut self, e: &'a Element) -> ReadResult {
        let mut r = self.read_children(e.child("w:sdtContent"));
        let Some(cb) = e.path(&["w:sdtPr", "wordml:checkbox"]) else {
            return r;
        };
        let checked = cb
            .child("wordml:checked")
            .is_some_and(|c| boolean_attr(c.attr("wordml:val")));
        let mut replaced = false;
        for el in &mut r.elements {
            replace_first_text(el, checked, &mut replaced);
        }
        if !replaced {
            r.elements = vec![DocEl::Checkbox(checked)];
        }
        r
    }
}

fn replace_first_text(el: &mut DocEl, checked: bool, done: &mut bool) {
    if let Some(children) = children_mut(el) {
        for c in children.iter_mut() {
            replace_first_text(c, checked, done);
        }
    }
    if let DocEl::Text(t) = el
        && !t.is_empty()
        && !*done
    {
        *done = true;
        *el = DocEl::Checkbox(checked);
    }
}

fn children_mut(el: &mut DocEl) -> Option<&mut Vec<DocEl>> {
    match el {
        DocEl::Paragraph { children, .. }
        | DocEl::Hyperlink { children, .. }
        | DocEl::Table { children, .. }
        | DocEl::TableRow { children, .. }
        | DocEl::TableCell { children, .. } => Some(children),
        DocEl::Run(r) => Some(&mut r.children),
        _ => None,
    }
}

/// `"(.*)"` matched at the start of `s`: greedy up to the last quote on the
/// first line.
fn quoted_greedy(s: &str) -> Option<String> {
    let rest = s.strip_prefix('"')?;
    let line = rest.split('\n').next().unwrap_or_default();
    let end = line.rfind('"')?;
    Some(line[..end].to_string())
}

fn replace_fragment(uri: &str, fragment: &str) -> String {
    let base = uri.split('#').next().unwrap_or_default();
    format!("{base}#{fragment}")
}

fn cell_mut(rows: &mut [DocEl], r: usize, c: usize) -> Option<(&mut u32, &mut bool, u32)> {
    let DocEl::TableRow { children, .. } = rows.get_mut(r)? else {
        return None;
    };
    let DocEl::TableCell {
        rowspan,
        vmerge,
        colspan,
        ..
    } = children.get_mut(c)?
    else {
        return None;
    };
    Some((rowspan, vmerge, *colspan))
}

/// mammoth's vertical-merge resolution: a `vMerge` continuation cell extends
/// the rowspan of the cell above it in the same grid column and is removed.
fn calculate_row_spans(rows: &mut [DocEl]) {
    let well_formed = rows.iter().all(|r| {
        matches!(r, DocEl::TableRow { children, .. }
            if children.iter().all(|c| matches!(c, DocEl::TableCell { .. })))
    });
    if !well_formed {
        // mammoth keeps every cell as-is (no merging) in this case
        return;
    }
    let mut columns: HashMap<u32, (usize, usize)> = HashMap::new();
    for ri in 0..rows.len() {
        let mut index = 0;
        let mut ci = 0;
        while let Some((_, vmerge, colspan)) = cell_mut(rows, ri, ci) {
            let merge = *vmerge;
            match columns.get(&index) {
                Some(&(r0, c0)) if merge => {
                    if let Some((rowspan, ..)) = cell_mut(rows, r0, c0) {
                        *rowspan += 1;
                    }
                }
                _ => {
                    *vmerge = false;
                    columns.insert(index, (ri, ci));
                }
            }
            index += colspan;
            ci += 1;
        }
    }
    for row in rows.iter_mut() {
        if let DocEl::TableRow { children, .. } = row {
            children.retain(|c| !matches!(c, DocEl::TableCell { vmerge: true, .. }));
        }
    }
}
