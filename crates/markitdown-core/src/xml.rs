//! A small namespace-aware XML tree.
//!
//! Element and attribute names are rewritten to a canonical `prefix:local`
//! form from a namespace table (as mammoth does), so documents that bind the
//! same namespace to another prefix read the same. Names in namespaces that
//! are not in the table become `{uri}local`.

use crate::Error;
use quick_xml::NsReader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{QName, ResolveResult};
use std::collections::HashSet;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum Node {
    Element(Element),
    Text(String),
}

#[derive(Debug, Clone, Default)]
pub struct Element {
    /// interned: documents repeat a few dozen names hundreds of thousands of times
    pub name: Rc<str>,
    pub attrs: Vec<(Rc<str>, String)>,
    pub children: Vec<Node>,
}

impl Element {
    pub fn new(name: &str, children: Vec<Node>) -> Self {
        Element {
            name: name.into(),
            attrs: Vec::new(),
            children,
        }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| &**k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| &*e.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.elements().filter(move |e| &*e.name == name)
    }

    /// `child(a)?.child(b)?...` along a path of names.
    pub fn path(&self, names: &[&str]) -> Option<&Element> {
        names.iter().try_fold(self, |e, n| e.child(n))
    }

    pub fn inner_text(&self) -> String {
        let mut out = String::new();
        fn walk(e: &Element, out: &mut String) {
            for c in &e.children {
                match c {
                    Node::Text(t) => out.push_str(t),
                    Node::Element(e) => walk(e, out),
                }
            }
        }
        walk(self, &mut out);
        out
    }
}

pub struct ParseOptions<'a> {
    /// (prefix, namespace URI) pairs used to canonicalise names.
    pub namespaces: &'a [(&'a str, &'a str)],
    /// Keep CDATA sections as text (lxml) instead of dropping them (minidom).
    pub cdata_as_text: bool,
    /// Apply XML whitespace normalisation after entity expansion instead of
    /// before, as happens when a part has been re-serialised by lxml first.
    pub normalize_after_unescape: bool,
}

/// Converters walk the tree recursively; Python markitdown hits its recursion
/// limit on such documents too.
const MAX_DEPTH: usize = 512;

pub fn parse(xml: &[u8], opts: &ParseOptions) -> Result<Element, Error> {
    let xml = xml.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(xml);
    let mut reader = NsReader::from_reader(xml);
    let mut buf = Vec::new();
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    let mut names = HashSet::new();
    let err = |e: &dyn std::fmt::Display| Error::Xml(e.to_string());
    loop {
        let ev = reader.read_event_into(&mut buf).map_err(|e| err(&e))?;
        match ev {
            Event::Start(e) => {
                if stack.len() >= MAX_DEPTH {
                    return Err(Error::Xml(format!(
                        "elements nested deeper than {MAX_DEPTH} levels"
                    )));
                }
                let el = start_element(&reader, &e, opts, &mut names)?;
                stack.push(el);
            }
            Event::Empty(e) => {
                let el = start_element(&reader, &e, opts, &mut names)?;
                attach(&mut stack, &mut root, el);
            }
            Event::End(_) => {
                let mut el = stack
                    .pop()
                    .ok_or_else(|| Error::Xml("unbalanced end tag".into()))?;
                el.children.shrink_to_fit();
                attach(&mut stack, &mut root, el);
            }
            Event::Text(t) => {
                if let Some(parent) = stack.last_mut() {
                    let raw = std::str::from_utf8(t.as_ref()).map_err(|e| err(&e))?;
                    push_text(parent, &text_value(raw, opts)?);
                }
            }
            Event::CData(t) => {
                if opts.cdata_as_text
                    && let Some(parent) = stack.last_mut()
                {
                    let s = std::str::from_utf8(t.as_ref()).map_err(|e| err(&e))?;
                    push_text(parent, &normalize_eol(s));
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if !stack.is_empty() {
        return Err(Error::Xml("unexpected end of document".into()));
    }
    root.ok_or_else(|| Error::Xml("no root element".into()))
}

fn attach(stack: &mut [Element], root: &mut Option<Element>, el: Element) {
    match stack.last_mut() {
        Some(parent) => parent.children.push(Node::Element(el)),
        None => *root = Some(el),
    }
}

fn push_text(parent: &mut Element, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(Node::Text(prev)) = parent.children.last_mut() {
        prev.push_str(s);
    } else {
        parent.children.push(Node::Text(s.to_string()));
    }
}

fn canonical(opts: &ParseOptions, ns: ResolveResult, local: &[u8]) -> Result<String, Error> {
    let local = std::str::from_utf8(local).map_err(|e| Error::Xml(e.to_string()))?;
    match ns {
        ResolveResult::Bound(uri) => {
            let uri = std::str::from_utf8(uri.as_ref()).map_err(|e| Error::Xml(e.to_string()))?;
            Ok(match opts.namespaces.iter().find(|(_, u)| *u == uri) {
                Some((prefix, _)) => format!("{prefix}:{local}"),
                None => format!("{{{uri}}}{local}"),
            })
        }
        ResolveResult::Unbound => Ok(local.to_string()),
        ResolveResult::Unknown(p) => Err(Error::Xml(format!(
            "unbound prefix {}",
            String::from_utf8_lossy(&p)
        ))),
    }
}

fn intern(names: &mut HashSet<Rc<str>>, name: String) -> Rc<str> {
    if let Some(n) = names.get(name.as_str()) {
        return n.clone();
    }
    let n: Rc<str> = name.into();
    names.insert(n.clone());
    n
}

fn start_element(
    reader: &NsReader<&[u8]>,
    e: &BytesStart,
    opts: &ParseOptions,
    names: &mut HashSet<Rc<str>>,
) -> Result<Element, Error> {
    let (ns, local) = reader.resolve_element(e.name());
    let name = intern(names, canonical(opts, ns, local.as_ref())?);
    let mut attrs = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|e| Error::Xml(e.to_string()))?;
        let key = a.key.as_ref();
        if key == b"xmlns" || key.starts_with(b"xmlns:") {
            continue;
        }
        let (ans, local) = reader.resolve_attribute(QName(key));
        let k = intern(names, canonical(opts, ans, local.as_ref())?);
        let raw = std::str::from_utf8(&a.value).map_err(|e| Error::Xml(e.to_string()))?;
        attrs.push((k, attr_value(raw, opts)?));
    }
    attrs.shrink_to_fit();
    Ok(Element {
        name,
        attrs,
        children: Vec::new(),
    })
}

fn unescape(s: &str) -> Result<String, Error> {
    quick_xml::escape::unescape(s)
        .map(|c| c.into_owned())
        .map_err(|e| Error::Xml(e.to_string()))
}

fn normalize_eol(s: &str) -> String {
    if s.contains('\r') {
        s.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        s.to_string()
    }
}

fn normalize_attr(s: &str) -> String {
    normalize_eol(s).replace(['\n', '\t'], " ")
}

fn text_value(raw: &str, opts: &ParseOptions) -> Result<String, Error> {
    Ok(if opts.normalize_after_unescape {
        normalize_eol(&unescape(raw)?)
    } else {
        unescape(&normalize_eol(raw))?
    })
}

fn attr_value(raw: &str, opts: &ParseOptions) -> Result<String, Error> {
    Ok(if opts.normalize_after_unescape {
        normalize_attr(&unescape(raw)?)
    } else {
        unescape(&normalize_attr(raw))?
    })
}
