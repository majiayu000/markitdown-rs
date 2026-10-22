//! pdfminer's PDFPage: the page tree walk, page attributes and the page
//! labels it pulls per page.

use std::collections::HashSet;
use std::rc::Rc;

use super::document::{Document, MAX_DEPTH};
use super::object::{Dict, Obj, PdfError, Result, other, py_float};

impl Document {
    /// `PDFPage.create_pages()`, with the page labels it pulls per page.
    pub fn pages(&self) -> Result<Vec<Page>> {
        let mut labels = Labels::new(self)?;
        let mut found = Vec::new();
        if let Some(root) = self.catalog.get("Pages") {
            let mut visited = HashSet::new();
            self.walk_pages(root, &self.catalog, &mut visited, &mut found, 0)?;
        }
        let mut pages = Vec::new();
        for props in found {
            labels.next(self)?;
            pages.push(Page::new(self, Rc::new(props))?);
        }
        if pages.is_empty() {
            for ids in self.xref_objids() {
                for objid in ids {
                    match self.getobj(objid) {
                        Ok(Obj::Dict(d)) if d.get("Type").is_some_and(|t| t.is_name("Page")) => {
                            labels.next(self)?;
                            pages.push(Page::new(self, d)?);
                        }
                        Ok(_) | Err(PdfError::NotFound(_)) => {}
                        Err(e) => return Err(e),
                    }
                }
            }
        }
        Ok(pages)
    }

    fn walk_pages(
        &self,
        obj: &Obj,
        parent: &Dict,
        visited: &mut HashSet<i64>,
        out: &mut Vec<Dict>,
        depth: u32,
    ) -> Result<()> {
        if depth > MAX_DEPTH {
            return other("maximum recursion depth exceeded");
        }
        let (id, props) = match obj {
            Obj::Int(_) | Obj::Bool(_) => {
                let id = obj.as_int().unwrap();
                (id, self.dict_value(&self.getobj(id)?)?)
            }
            Obj::Ref(id) => (*id, self.dict_value(obj)?),
            _ => return other("object has no attribute 'objid'"),
        };
        if !visited.insert(id) {
            return Ok(());
        }
        let mut props = (*props).clone();
        for (k, v) in &parent.0 {
            let inheritable = matches!(&**k, b"Resources" | b"MediaBox" | b"CropBox" | b"Rotate");
            if inheritable && !props.0.iter().any(|(pk, _)| pk == k) {
                props.0.push((k.clone(), v.clone()));
            }
        }
        let ty = props.get("Type").or_else(|| props.get("type")).cloned();
        if ty.as_ref().is_some_and(|t| t.is_name("Pages")) && props.contains("Kids") {
            let kids = self.list_value(props.get("Kids").unwrap())?;
            for child in kids.iter() {
                self.walk_pages(child, &props, visited, out, depth + 1)?;
            }
        } else if ty.as_ref().is_some_and(|t| t.is_name("Page")) {
            out.push(props);
        }
        Ok(())
    }
}

pub struct Page {
    pub attrs: Rc<Dict>,
    pub resources: Obj,
    pub mediabox: [f64; 4],
    pub contents: Vec<Obj>,
    pub rotate: i64,
}

impl Page {
    fn new(doc: &Document, attrs: Rc<Dict>) -> Result<Self> {
        let resources = match attrs.get("Resources") {
            Some(r) => doc.resolve(r)?,
            None => Obj::Dict(Rc::new(Dict::default())),
        };
        let letter = [0.0, 0.0, 612.0, 792.0];
        let mediabox = match attrs.get("MediaBox") {
            None => letter,
            Some(v) => parse_rect(doc, v)?.unwrap_or(letter),
        };
        if let Some(v) = attrs.get("CropBox") {
            parse_rect(doc, v)?;
        }
        let contents = match attrs.get("Contents") {
            None => Vec::new(),
            Some(v) => match doc.resolve(v)? {
                Obj::Array(a) => a.to_vec(),
                c => vec![c],
            },
        };
        let rotate =
            (doc.int_value(attrs.get("Rotate").unwrap_or(&Obj::Int(0)))? + 360).rem_euclid(360);
        Ok(Page {
            attrs,
            resources,
            mediabox,
            contents,
            rotate,
        })
    }
}

/// `parse_rect(resolve1(v) for v in resolve1(value))`: None stands for the
/// PDFValueError that falls back to the default box.
fn parse_rect(doc: &Document, value: &Obj) -> Result<Option<[f64; 4]>> {
    let items: Vec<Obj> = match doc.resolve(value)? {
        Obj::Array(a) => a.iter().map(|v| doc.resolve(v)).collect::<Result<_>>()?,
        Obj::Str(s) => s.iter().map(|&b| Obj::Int(b as i64)).collect(),
        Obj::Dict(d) => d.0.iter().map(|(k, _)| Obj::Str(k.clone())).collect(),
        _ => return other("object is not iterable"),
    };
    if items.len() != 4 {
        return Ok(None);
    }
    let mut r = [0.0; 4];
    for (slot, v) in r.iter_mut().zip(&items) {
        *slot = match v {
            Obj::Str(s) => match py_float(&String::from_utf8_lossy(s)) {
                Some(f) => f,
                None => return Ok(None),
            },
            v => match v.as_num() {
                Some(f) => f,
                None => return other("float() argument must be a string or a real number"),
            },
        };
    }
    Ok(Some(r))
}

/// The page-label generator `create_pages` advances once per page; only
/// its failures (roman numerals past 3999, letters below 1) matter here.
struct Labels {
    ranges: Option<Vec<(i64, Obj)>>,
    present: bool,
    idx: usize,
    value: i64,
    end: Option<i64>,
    style: Option<u8>,
}

impl Labels {
    fn new(doc: &Document) -> Result<Self> {
        Ok(Labels {
            ranges: None,
            present: doc.catalog.contains("PageLabels"),
            idx: 0,
            value: 0,
            end: Some(0),
            style: None,
        })
    }

    fn tree(doc: &Document, obj: &Obj, depth: u32, items: &mut Vec<(i64, Obj)>) -> Result<()> {
        if depth > MAX_DEPTH / 2 {
            return other("maximum recursion depth exceeded");
        }
        let d = doc.dict_value(obj)?;
        if let Some(n) = d.get("Nums") {
            let nums = doc.list_value(n)?;
            for p in nums.as_chunks::<2>().0 {
                items.push((doc.int_value(&p[0])?, p[1].clone()));
            }
        }
        if let Some(k) = d.get("Kids") {
            for kid in doc.list_value(k)?.iter() {
                Self::tree(doc, kid, depth + 1, items)?;
            }
        }
        Ok(())
    }

    fn next(&mut self, doc: &Document) -> Result<()> {
        if !self.present {
            return Ok(());
        }
        if self.ranges.is_none() {
            let mut items = Vec::new();
            Self::tree(doc, doc.catalog.get("PageLabels").unwrap(), 0, &mut items)?;
            items.sort_by_key(|t| t.0);
            if items.first().is_none_or(|t| t.0 != 0) {
                items.insert(0, (0, Obj::Dict(Rc::new(Dict::default()))));
            }
            self.ranges = Some(items);
            self.idx = 0;
            self.end = Some(0);
            self.value = 0;
        }
        let ranges = self.ranges.as_ref().unwrap();
        while self.end.is_some_and(|e| self.value >= e) {
            if self.idx >= ranges.len() {
                return Ok(());
            }
            let (start, dict) = &ranges[self.idx];
            let d = doc.dict_value(dict)?;
            self.style = d.get("S").and_then(|s| match s.name() {
                Some(b"D") => Some(b'D'),
                Some(b"R") => Some(b'R'),
                Some(b"r") => Some(b'r'),
                Some(b"A") => Some(b'A'),
                Some(b"a") => Some(b'a'),
                _ => None,
            });
            if let Some(p) = d.get("P") {
                doc.str_value(p)?;
            }
            let first = doc.int_value(d.get("St").unwrap_or(&Obj::Int(1)))?;
            self.value = first;
            self.end = ranges.get(self.idx + 1).map(|(e, _)| first + (e - start));
            self.idx += 1;
        }
        let v = self.value;
        self.value += 1;
        match self.style {
            Some(b'R' | b'r') if !(0 < v && v < 4000) => other("AssertionError: roman page label"),
            Some(b'A' | b'a') if v <= 0 => other("AssertionError: alphabetic page label"),
            _ => Ok(()),
        }
    }
}
