//! DOCX -> Markdown with markitdown 0.1.8's output: its pre-processing
//! (equations to LaTeX, double strike, style repairs), mammoth 1.11's
//! docx-to-HTML conversion and markdownify, without the HTML round trip.

mod dingbats;
mod html;
mod math;
mod package;
mod reader;

use crate::markdownify::{self, Dom};
use crate::xml::{self, Element, Node, ParseOptions};
use crate::zip::Archive;
use crate::{Error, Options, pystr};
use package::{ContentTypes, Numbering, Relationships, Styles};
use reader::{BodyReader, Ctx};

const NAMESPACES: &[(&str, &str)] = &[
    // mammoth's table (transitional, then strict)
    (
        "w",
        "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    ),
    (
        "r",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    ),
    (
        "wp",
        "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
    ),
    ("a", "http://schemas.openxmlformats.org/drawingml/2006/main"),
    (
        "pic",
        "http://schemas.openxmlformats.org/drawingml/2006/picture",
    ),
    ("w", "http://purl.oclc.org/ooxml/wordprocessingml/main"),
    (
        "r",
        "http://purl.oclc.org/ooxml/officeDocument/relationships",
    ),
    (
        "wp",
        "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing",
    ),
    ("a", "http://purl.oclc.org/ooxml/drawingml/main"),
    ("pic", "http://purl.oclc.org/ooxml/drawingml/picture"),
    (
        "content-types",
        "http://schemas.openxmlformats.org/package/2006/content-types",
    ),
    (
        "relationships",
        "http://schemas.openxmlformats.org/package/2006/relationships",
    ),
    (
        "mc",
        "http://schemas.openxmlformats.org/markup-compatibility/2006",
    ),
    ("v", "urn:schemas-microsoft-com:vml"),
    ("office-word", "urn:schemas-microsoft-com:office:word"),
    (
        "wordml",
        "http://schemas.microsoft.com/office/word/2010/wordml",
    ),
    // namespaces the equation pre-processing knows about
    (
        "m",
        "http://schemas.openxmlformats.org/officeDocument/2006/math",
    ),
    ("m", "http://purl.oclc.org/ooxml/officeDocument/math"),
    ("o", "urn:schemas-microsoft-com:office:office"),
    (
        "wpc",
        "http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas",
    ),
    (
        "wp14",
        "http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing",
    ),
    (
        "wpg",
        "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup",
    ),
    (
        "wpi",
        "http://schemas.microsoft.com/office/word/2010/wordprocessingInk",
    ),
    (
        "wne",
        "http://schemas.microsoft.com/office/word/2006/wordml",
    ),
    (
        "wps",
        "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
    ),
    ("xml", "http://www.w3.org/XML/1998/namespace"),
];

const REL_BASE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";

/// Parts markitdown rewrites (strike + math) before mammoth reads them.
const PREPROCESSED: &[&str] = &[
    "word/document.xml",
    "word/footnotes.xml",
    "word/endnotes.xml",
];

struct Package<'a> {
    zip: Archive<'a>,
}

impl Package<'_> {
    fn read_xml(&mut self, name: &str) -> Result<Option<Element>, Error> {
        let Some(bytes) = self.zip.read(name)? else {
            return Ok(None);
        };
        let minidom = ParseOptions {
            namespaces: NAMESPACES,
            cdata_as_text: false,
            normalize_after_unescape: false,
        };
        if !PREPROCESSED.contains(&name) {
            let mut root = xml::parse(&bytes, &minidom)?;
            collapse_alternate_content(&mut root);
            return Ok(Some(root));
        }
        // markitdown re-serialises these parts with BeautifulSoup/lxml first
        let lxml = ParseOptions {
            cdata_as_text: true,
            normalize_after_unescape: true,
            ..minidom
        };
        let mut root = xml::parse(&bytes, &lxml)?;
        let has_dstrike = contains(&bytes, b"dstrike");
        if has_dstrike {
            rename_dstrike(&mut root);
        }
        if contains(&bytes, b"oMath") {
            match math::replace_equations(&root) {
                Some(replaced) => root = replaced,
                // the whole math step failed: the part keeps its OMML, and is only
                // re-serialised if the strike step did so
                None if !has_dstrike => root = xml::parse(&bytes, &minidom)?,
                None => {}
            }
        }
        drop(bytes);
        collapse_alternate_content(&mut root);
        Ok(Some(root))
    }

    fn rels_for(&mut self, part: &str) -> Result<Relationships, Error> {
        let (dir, base) = split_path(part);
        let path = join_path(&[dir, "_rels", &format!("{base}.rels")]);
        Ok(self
            .read_xml(&path)?
            .map(|r| Relationships::parse(&r))
            .unwrap_or_default())
    }

    fn find_part(&self, rels: &Relationships, typ: &str, base: &str, fallback: &str) -> String {
        rels.targets_of_type(typ)
            .map(|t| join_path(&[base, t]).trim_start_matches('/').to_string())
            .find(|t| self.zip.contains(t))
            .unwrap_or_else(|| fallback.to_string())
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn rename_dstrike(el: &mut Element) {
    if let Some(prefix) = el.name.strip_suffix("dstrike")
        && (prefix.is_empty() || prefix.ends_with([':', '}']))
    {
        el.name = format!("{prefix}strike").into();
    }
    for c in &mut el.children {
        if let Node::Element(c) = c {
            rename_dstrike(c);
        }
    }
}

/// mammoth's office_xml: `mc:AlternateContent` is replaced by the children of
/// its `mc:Fallback`.
fn collapse_alternate_content(el: &mut Element) {
    let has_ac = el.elements().any(|e| &*e.name == "mc:AlternateContent");
    if !has_ac {
        for c in &mut el.children {
            if let Node::Element(e) = c {
                collapse_alternate_content(e);
            }
        }
        return;
    }
    let children = std::mem::take(&mut el.children);
    for c in children {
        match c {
            Node::Element(e) if &*e.name == "mc:AlternateContent" => {
                if let Some(fb) = e.children.into_iter().find_map(|n| match n {
                    Node::Element(f) if &*f.name == "mc:Fallback" => Some(f),
                    _ => None,
                }) {
                    el.children.extend(fb.children);
                }
            }
            Node::Element(mut e) => {
                collapse_alternate_content(&mut e);
                el.children.push(Node::Element(e));
            }
            text => el.children.push(text),
        }
    }
}

fn split_path(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

fn join_path(parts: &[&str]) -> String {
    let mut relevant: Vec<&str> = Vec::new();
    for p in parts.iter().filter(|p| !p.is_empty()) {
        if p.starts_with('/') {
            relevant = vec![p];
        } else {
            relevant.push(p);
        }
    }
    relevant.join("/")
}

pub fn convert(data: &[u8], opts: &Options) -> Result<String, Error> {
    let mut pkg = Package {
        zip: Archive::new(data)?,
    };

    let package_rels = pkg.rels_for("")?;
    let document_path = pkg.find_part(
        &package_rels,
        &format!("{REL_BASE}officeDocument"),
        "",
        "word/document.xml",
    );
    if !pkg.zip.contains(&document_path) {
        return Err(Error::MissingPart(document_path));
    }
    let document_rels = pkg.rels_for(&document_path)?;
    let base = split_path(&document_path).0.to_string();
    let part = |pkg: &Package, name: &str| {
        pkg.find_part(
            &document_rels,
            &format!("{REL_BASE}{name}"),
            &base,
            &format!("word/{name}.xml"),
        )
    };
    let (styles_path, numbering_path) = (part(&pkg, "styles"), part(&pkg, "numbering"));
    let (footnotes_path, endnotes_path) = (part(&pkg, "footnotes"), part(&pkg, "endnotes"));

    let content_types = pkg
        .read_xml("[Content_Types].xml")?
        .map(|r| ContentTypes::parse(&r))
        .unwrap_or_default();
    let styles = pkg
        .read_xml(&styles_path)?
        .map(|r| Styles::parse(&r))
        .unwrap_or_default();
    let numbering = pkg
        .read_xml(&numbering_path)?
        .map(|r| Numbering::parse(&r, &styles))
        .unwrap_or_default();

    let mut notes = Vec::new();
    for (path, note_type) in [(&footnotes_path, "footnote"), (&endnotes_path, "endnote")] {
        let Some(root) = pkg.read_xml(path)? else {
            continue;
        };
        let rels = pkg.rels_for(path)?;
        for n in root.children_named(&format!("w:{note_type}")) {
            if matches!(
                n.attr("w:type"),
                Some("continuationSeparator" | "separator")
            ) {
                continue;
            }
            let ctx = Ctx {
                styles: &styles,
                numbering: &numbering,
                content_types: &content_types,
                rels: &rels,
            };
            let body = BodyReader::new(ctx).read_all(&n.children);
            notes.push(html::Note {
                note_type,
                id: n.attr("w:id").unwrap_or_default().to_string(),
                body,
            });
        }
    }
    let document = pkg
        .read_xml(&document_path)?
        .ok_or_else(|| Error::MissingPart(document_path.clone()))?;
    let body_el = document
        .child("w:body")
        .ok_or_else(|| Error::MissingPart(format!("{document_path} w:body")))?;
    let ctx = Ctx {
        styles: &styles,
        numbering: &numbering,
        content_types: &content_types,
        rels: &document_rels,
    };
    let body = BodyReader::new(ctx).read_all(&body_el.children);
    // each stage is freed as soon as the next one is built to keep peak memory low
    drop(document);

    let zip = std::cell::RefCell::new(pkg.zip);
    let image_data = |path: &str| zip.borrow_mut().read(path).ok().flatten();
    let mut dom = Dom::new("[document]");
    html::Converter::new(&notes, opts.keep_data_uris, &image_data).convert_document(body, &mut dom);
    let md = markdownify::convert(
        &dom,
        markdownify::ROOT,
        &markdownify::Options {
            keep_data_uris: opts.keep_data_uris,
        },
    );
    Ok(pystr::strip(&md).to_string())
}
