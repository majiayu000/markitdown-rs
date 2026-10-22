//! EPUB books, as markitdown's EpubConverter: metadata from the OPF
//! package document, then every spine item through the HTML converter.

use crate::xml::{self, Element, Node, ParseOptions};
use crate::zip::Archive;
use crate::{Error, html, pystr, pyurl};

fn parse(bytes: &[u8]) -> Result<Element, Error> {
    let opts = ParseOptions {
        namespaces: &[],
        cdata_as_text: true,
        normalize_after_unescape: false,
        raw_names: true,
    };
    xml::parse(bytes, &opts)
}

/// minidom `getElementsByTagName`: descendants (and self) by qualified name.
fn by_tag<'a>(e: &'a Element, tag: &str, out: &mut Vec<&'a Element>) {
    if &*e.name == tag {
        out.push(e);
    }
    for c in e.elements() {
        by_tag(c, tag, out);
    }
}

fn all_texts(root: &Element, tag: &str) -> Vec<String> {
    fn collect(e: &Element, out: &mut String) {
        for c in &e.children {
            match c {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => collect(e, out),
            }
        }
    }
    let mut found = Vec::new();
    by_tag(root, tag, &mut found);
    found
        .into_iter()
        .filter_map(|n| {
            let mut s = String::new();
            collect(n, &mut s);
            let s = pystr::strip(&s).to_string();
            (!s.is_empty()).then_some(s)
        })
        .collect()
}

/// posixpath.normpath (no leading slash handling needed for zip names)
fn normpath(p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." if parts.last().is_some_and(|l| *l != "..") => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let joined = parts.join("/");
    if p.starts_with('/') {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".into()
    } else {
        joined
    }
}

pub fn convert(data: &[u8], keep_data_uris: bool) -> Result<String, Error> {
    let mut zip = Archive::new(data)?;
    let missing = |p: &str| Error::MissingPart(p.to_string());
    let container = parse(
        &zip.read("META-INF/container.xml")?
            .ok_or_else(|| missing("META-INF/container.xml"))?,
    )?;
    let mut rootfiles = Vec::new();
    by_tag(&container, "rootfile", &mut rootfiles);
    let opf_path = rootfiles
        .first()
        .ok_or_else(|| missing("rootfile"))?
        .attr("full-path")
        .unwrap_or_default()
        .to_string();
    let opf = parse(&zip.read(&opf_path)?.ok_or_else(|| missing(&opf_path))?)?;
    let first = |tag: &str| all_texts(&opf, tag).into_iter().next();
    let metadata: Vec<(&str, Option<String>)> = vec![
        ("Title", first("dc:title")),
        ("Authors", Some(all_texts(&opf, "dc:creator").join(", "))),
        ("Language", first("dc:language")),
        ("Publisher", first("dc:publisher")),
        ("Date", first("dc:date")),
        ("Description", first("dc:description")),
        ("Identifier", first("dc:identifier")),
    ];
    let mut items = Vec::new();
    by_tag(&opf, "item", &mut items);
    let mut manifest: Vec<(String, String)> = Vec::new();
    for item in items {
        let (id, href) = (
            item.attr("id").unwrap_or_default().to_string(),
            item.attr("href").unwrap_or_default().to_string(),
        );
        match manifest.iter_mut().find(|(k, _)| *k == id) {
            Some(slot) => slot.1 = href,
            None => manifest.push((id, href)),
        }
    }
    let mut refs = Vec::new();
    by_tag(&opf, "itemref", &mut refs);
    let base = opf_path.rsplit_once('/').map_or("", |(d, _)| d);
    let names = zip.names();
    let spine: Vec<String> = refs
        .iter()
        .filter_map(|r| {
            manifest
                .iter()
                .find(|(k, _)| k == r.attr("idref").unwrap_or_default())
        })
        .map(|(_, href)| {
            let mut candidates: Vec<String> = Vec::new();
            for cand in [pyurl::unquote(href, false), href.clone()] {
                let joined = if base.is_empty() {
                    cand
                } else {
                    format!("{base}/{cand}")
                };
                let resolved = normpath(&joined);
                if !candidates.contains(&resolved) {
                    candidates.push(resolved);
                }
            }
            candidates
                .iter()
                .find(|c| names.contains(c))
                .cloned()
                .unwrap_or_else(|| candidates[0].clone())
        })
        .collect();
    let mut parts = Vec::new();
    for file in spine {
        if let Some(bytes) = zip.read(&file)? {
            parts.push(pystr::strip(&html::convert(&bytes, None, keep_data_uris)).to_string());
        }
    }
    let meta: Vec<String> = metadata
        .into_iter()
        .filter_map(|(k, v)| v.filter(|v| !v.is_empty()).map(|v| format!("**{k}:** {v}")))
        .collect();
    parts.insert(0, meta.join("\n"));
    Ok(parts.join("\n\n"))
}
