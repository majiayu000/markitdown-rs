//! PPTX -> Markdown with markitdown 0.1.8's output, reading the package the
//! way python-pptx 1.0.2 does: shape classes from the shape factory,
//! placeholder positions inherited from the layout and master for ordering,
//! `\v` for soft line breaks, chart data from the chart part's caches.

use crate::markdownify::{self, Dom};
use crate::xml::{self, Element, Node, ParseOptions};
use crate::zip::Archive;
use crate::{Error, Options, pystr};
use std::collections::HashMap;

const NAMESPACES: &[(&str, &str)] = &[
    (
        "p",
        "http://schemas.openxmlformats.org/presentationml/2006/main",
    ),
    ("a", "http://schemas.openxmlformats.org/drawingml/2006/main"),
    (
        "r",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    ),
    (
        "c",
        "http://schemas.openxmlformats.org/drawingml/2006/chart",
    ),
    (
        "asvg",
        "http://schemas.microsoft.com/office/drawing/2016/SVG/main",
    ),
    (
        "relationships",
        "http://schemas.openxmlformats.org/package/2006/relationships",
    ),
    (
        "content-types",
        "http://schemas.openxmlformats.org/package/2006/content-types",
    ),
];

const RT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const URI_CHART: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const URI_TABLE: &str = "http://schemas.openxmlformats.org/drawingml/2006/table";
const SHAPE_TAGS: &[&str] = &[
    "p:sp",
    "p:grpSp",
    "p:graphicFrame",
    "p:cxnSp",
    "p:pic",
    "p:contentPart",
];

struct Rel {
    id: String,
    typ: String,
    /// resolved part name without the leading '/'; None for external targets
    part: Option<String>,
}

struct Package<'a> {
    zip: Archive<'a>,
    xml_cache: HashMap<String, Option<Element>>,
}

impl Package<'_> {
    fn xml(&mut self, name: &str) -> Result<Option<&Element>, Error> {
        if !self.xml_cache.contains_key(name) {
            let parsed = match self.zip.read(name)? {
                Some(bytes) => {
                    let opts = ParseOptions {
                        namespaces: NAMESPACES,
                        cdata_as_text: true,
                        normalize_after_unescape: false,
                    };
                    Some(xml::parse(&bytes, &opts)?)
                }
                None => None,
            };
            self.xml_cache.insert(name.to_string(), parsed);
        }
        Ok(self.xml_cache[name].as_ref())
    }

    fn rels(&mut self, part: &str) -> Result<Vec<Rel>, Error> {
        let (dir, base) = part.rsplit_once('/').unwrap_or(("", part));
        let path = if dir.is_empty() {
            format!("_rels/{base}.rels")
        } else {
            format!("{dir}/_rels/{base}.rels")
        };
        let Some(root) = self.xml(&path)? else {
            return Ok(Vec::new());
        };
        Ok(root
            .children_named("relationships:Relationship")
            .filter_map(|r| {
                let external = r.attr("TargetMode") == Some("External");
                let target = r.attr("Target")?;
                Some(Rel {
                    id: r.attr("Id")?.to_string(),
                    typ: r.attr("Type")?.to_string(),
                    part: (!external).then(|| resolve(dir, target)),
                })
            })
            .collect())
    }
}

/// posixpath.normpath(posixpath.join("/" + base_dir, target)) without the '/'.
fn resolve(base_dir: &str, target: &str) -> String {
    let joined = if target.starts_with('/') {
        target.to_string()
    } else {
        format!("{base_dir}/{target}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn rel_of_type<'r>(rels: &'r [Rel], typ: &str) -> Option<&'r str> {
    rels.iter()
        .find(|r| r.typ == format!("{RT}{typ}"))
        .and_then(|r| r.part.as_deref())
}

fn rel_by_id<'r>(rels: &'r [Rel], id: &str) -> Option<&'r str> {
    rels.iter()
        .find(|r| r.id == id)
        .and_then(|r| r.part.as_deref())
}

fn shape_elems(tree: &Element) -> impl Iterator<Item = &Element> {
    tree.elements()
        .filter(|e| SHAPE_TAGS.contains(&e.name.as_ref()))
}

/// `./*[1]/p:nvPr/p:ph`
fn ph(shape: &Element) -> Option<&Element> {
    shape.elements().next()?.path(&["p:nvPr", "p:ph"])
}

fn ph_idx(ph: &Element) -> i64 {
    ph.attr("idx")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

fn ph_type(ph: &Element) -> &str {
    ph.attr("type").unwrap_or("obj")
}

/// ST_Coordinate: EMU integer or a universal measure such as "1.5in".
fn coordinate(v: &str) -> Option<i64> {
    if v.contains(['i', 'm', 'p']) {
        let split = v.len().checked_sub(2)?;
        let (num, unit) = v.split_at(split);
        let mult = match unit {
            "mm" => 36000.0,
            "cm" => 360000.0,
            "in" => 914400.0,
            "pt" => 12700.0,
            "pc" | "pi" => 152400.0,
            _ => return None,
        };
        return Some((num.parse::<f64>().ok()? * mult) as i64);
    }
    v.parse().ok()
}

/// The shape's own (top, left) from its xfrm, as `BaseShape.top/.left`.
fn own_offset(shape: &Element) -> (Option<i64>, Option<i64>) {
    let xfrm = match shape.name.as_ref() {
        "p:graphicFrame" => shape.child("p:xfrm"),
        "p:grpSp" => shape.path(&["p:grpSpPr", "a:xfrm"]),
        "p:sp" | "p:pic" | "p:cxnSp" => shape.path(&["p:spPr", "a:xfrm"]),
        _ => None,
    };
    let off = xfrm.and_then(|x| x.child("a:off"));
    let get = |k| off.and_then(|o| o.attr(k)).and_then(coordinate);
    (get("y"), get("x"))
}

struct Layout<'a> {
    layout: Option<&'a Element>,
    master: Option<&'a Element>,
}

impl Layout<'_> {
    /// `_InheritsDimensions`: own value, else the layout placeholder with the
    /// same idx, which in turn falls back to the master placeholder by type.
    fn offset(&self, shape: &Element) -> (Option<i64>, Option<i64>) {
        let own = own_offset(shape);
        let inherits = matches!(shape.name.as_ref(), "p:sp" | "p:pic") && ph(shape).is_some();
        if !inherits || (own.0.is_some() && own.1.is_some()) {
            return own;
        }
        let idx = ph(shape).map(ph_idx).unwrap_or(0);
        let base = self
            .layout
            .and_then(|l| sp_tree(l))
            .and_then(|t| shape_elems(t).find(|e| ph(e).is_some_and(|p| ph_idx(p) == idx)));
        let inherited = base.map(|b| self.layout_offset(b)).unwrap_or((None, None));
        (own.0.or(inherited.0), own.1.or(inherited.1))
    }

    fn layout_offset(&self, base: &Element) -> (Option<i64>, Option<i64>) {
        let own = own_offset(base);
        if &*base.name != "p:sp" {
            return own;
        }
        let master_type = match ph(base).map(ph_type) {
            Some(
                "body" | "chart" | "clipArt" | "dgm" | "media" | "obj" | "pic" | "subTitle" | "tbl",
            ) => "body",
            Some("ctrTitle" | "title") => "title",
            Some(t @ ("dt" | "ftr" | "sldNum")) => t,
            _ => return own,
        };
        let master_ph = self.master.and_then(|m| sp_tree(m)).and_then(|t| {
            shape_elems(t).find(|e| ph(e).is_some_and(|p| ph_type(p) == master_type))
        });
        let inherited = master_ph.map(own_offset).unwrap_or((None, None));
        (own.0.or(inherited.0), own.1.or(inherited.1))
    }
}

fn sp_tree(part_root: &Element) -> Option<&Element> {
    part_root.path(&["p:cSld", "p:spTree"])
}

/// lxml `.text`: the text before the first child element.
fn leading_text(e: &Element) -> String {
    e.children
        .iter()
        .map_while(|n| {
            if let Node::Text(t) = n {
                Some(t.as_str())
            } else {
                None
            }
        })
        .collect()
}

fn paragraph_text(p: &Element) -> String {
    p.elements()
        .filter_map(|c| match c.name.as_ref() {
            "a:r" | "a:fld" => Some(c.child("a:t").map(leading_text).unwrap_or_default()),
            "a:br" => Some("\u{b}".to_string()),
            _ => None,
        })
        .collect()
}

/// `TextFrame.text`: paragraphs joined with '\n'.
fn frame_text(body: Option<&Element>) -> String {
    let Some(body) = body else {
        return String::new();
    };
    body.children_named("a:p")
        .map(paragraph_text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// `str(float)` as CPython prints it.
fn py_float(v: f64) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let sci = format!("{v:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let (sign, mantissa) = mantissa
        .strip_prefix('-')
        .map_or(("", mantissa), |m| ("-", m));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let decpt = exp + 1;
    if (-4 < decpt) && (decpt <= 16) {
        let d = digits.len() as i32;
        let s = if decpt <= 0 {
            format!("0.{}{digits}", "0".repeat((-decpt) as usize))
        } else if decpt >= d {
            format!("{digits}{}.0", "0".repeat((decpt - d) as usize))
        } else {
            format!(
                "{}.{}",
                &digits[..decpt as usize],
                &digits[decpt as usize..]
            )
        };
        format!("{sign}{s}")
    } else {
        let m = if digits.len() > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits
        };
        format!(
            "{sign}{m}e{}{:02}",
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        )
    }
}

struct Slide<'a> {
    rels: Vec<Rel>,
    layout: Layout<'a>,
    title: Option<&'a Element>,
}

pub fn convert(data: &[u8], opts: &Options) -> Result<String, Error> {
    let mut pkg = Package {
        zip: Archive::new(data)?,
        xml_cache: HashMap::new(),
    };
    let root_rels = pkg.rels("")?;
    let pres_path = rel_of_type(&root_rels, "officeDocument")
        .ok_or_else(|| Error::MissingPart("presentation".into()))?
        .to_string();
    let pres_rels = pkg.rels(&pres_path)?;
    let presentation = pkg
        .xml(&pres_path)?
        .ok_or_else(|| Error::MissingPart(pres_path.clone()))?;
    let slide_paths: Vec<String> = presentation
        .path(&["p:sldIdLst"])
        .map(|l| {
            l.children_named("p:sldId")
                .filter_map(|s| s.attr("r:id"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .map(|id| {
            rel_by_id(&pres_rels, id)
                .map(str::to_string)
                .ok_or_else(|| Error::MissingPart(format!("slide {id}")))
        })
        .collect::<Result<_, _>>()?;

    // load every part a slide needs first, so the borrows below are shared
    let mut slide_parts = Vec::new();
    for path in &slide_paths {
        let rels = pkg.rels(path)?;
        let layout = rel_of_type(&rels, "slideLayout").map(str::to_string);
        let master = match &layout {
            Some(l) => rel_of_type(&pkg.rels(l)?, "slideMaster").map(str::to_string),
            None => None,
        };
        let notes = rel_of_type(&rels, "notesSlide").map(str::to_string);
        for part in [
            Some(path.clone()),
            layout.clone(),
            master.clone(),
            notes.clone(),
        ]
        .into_iter()
        .flatten()
        {
            pkg.xml(&part)?;
        }
        for r in &rels {
            if r.typ == format!("{RT}chart")
                && let Some(p) = &r.part
            {
                pkg.xml(p)?;
            }
        }
        slide_parts.push((path.clone(), rels, layout, master, notes));
    }
    let image_bytes: HashMap<String, Vec<u8>> = if opts.keep_data_uris {
        let mut m = HashMap::new();
        for (_, rels, ..) in &slide_parts {
            for r in rels.iter().filter(|r| r.typ == format!("{RT}image")) {
                if let Some(p) = &r.part
                    && let Some(b) = pkg.zip.read(p)?
                {
                    m.insert(p.clone(), b);
                }
            }
        }
        m
    } else {
        HashMap::new()
    };
    let content_types = pkg.zip.read("[Content_Types].xml")?;
    let content_types = match content_types {
        Some(b) => Some(xml::parse(
            &b,
            &ParseOptions {
                namespaces: NAMESPACES,
                cdata_as_text: true,
                normalize_after_unescape: false,
            },
        )?),
        None => None,
    };
    let cache = &pkg.xml_cache;
    let get = |p: &Option<String>| {
        p.as_ref()
            .and_then(|p| cache.get(p))
            .and_then(Option::as_ref)
    };

    let mut md = String::new();
    for (n, (path, rels, layout, master, notes)) in slide_parts.into_iter().enumerate() {
        md.push_str(&format!("\n\n<!-- Slide number: {} -->\n", n + 1));
        let slide_el = get(&Some(path.clone())).ok_or_else(|| Error::MissingPart(path.clone()))?;
        let tree =
            sp_tree(slide_el).ok_or_else(|| Error::MissingPart(format!("{path} p:spTree")))?;
        let title = shape_elems(tree).find(|e| ph(e).is_some_and(|p| ph_idx(p) == 0));
        let slide = Slide {
            rels,
            layout: Layout {
                layout: get(&layout),
                master: get(&master),
            },
            title,
        };
        let ctx = Ctx {
            slide: &slide,
            parts: cache,
            opts,
            images: &image_bytes,
            content_types: content_types.as_ref(),
        };
        for shape in sorted(shape_elems(tree), |s| slide.layout.offset(s)) {
            ctx.shape_content(shape, &mut md)?;
        }
        md = pystr::strip(&md).to_string();
        if notes.is_some() {
            let body = get(&notes)
                .and_then(sp_tree)
                .and_then(|t| shape_elems(t).find(|e| ph(e).is_some_and(|p| ph_type(p) == "body")));
            let text = body
                .map(|b| frame_text(b.child("p:txBody")))
                .unwrap_or_default();
            if !pystr::strip(&text).is_empty() {
                md.push_str("\n\n### Notes:\n");
                md.push_str(&text);
                md = pystr::strip(&md).to_string();
            }
        }
    }
    Ok(pystr::strip(&md).to_string())
}

/// Stable sort by (top, left), missing values first.
fn sorted<'e>(
    shapes: impl Iterator<Item = &'e Element>,
    key: impl Fn(&Element) -> (Option<i64>, Option<i64>),
) -> Vec<&'e Element> {
    let mut v: Vec<_> = shapes.collect();
    v.sort_by_key(|s| key(s));
    v
}

struct Ctx<'c, 'a> {
    slide: &'c Slide<'a>,
    parts: &'c HashMap<String, Option<Element>>,
    opts: &'c Options,
    images: &'c HashMap<String, Vec<u8>>,
    content_types: Option<&'c Element>,
}

impl Ctx<'_, '_> {
    fn related_part(&self, rid: &str) -> Option<&str> {
        rel_by_id(&self.slide.rels, rid)
    }

    fn shape_content(&self, shape: &Element, md: &mut String) -> Result<(), Error> {
        let name = shape.name.as_ref();
        if self.is_picture(shape) {
            md.push_str(&self.picture(shape));
        }
        let graphic_uri = shape
            .path(&["a:graphic", "a:graphicData"])
            .and_then(|g| g.attr("uri"));
        if name == "p:graphicFrame" && graphic_uri == Some(URI_TABLE) {
            md.push_str(&table(shape));
        }
        if name == "p:graphicFrame" && graphic_uri == Some(URI_CHART) {
            md.push_str(&self.chart(shape)?);
        } else if name == "p:sp" {
            let text = frame_text(shape.child("p:txBody"));
            if self.slide.title.is_some_and(|t| std::ptr::eq(t, shape)) {
                if !pystr::strip(&text).is_empty() {
                    md.push_str("# ");
                    md.push_str(text.trim_start_matches(pystr::is_space));
                    md.push('\n');
                }
            } else {
                md.push_str(&text);
                md.push('\n');
            }
        }
        if name == "p:grpSp" {
            for child in sorted(shape_elems(shape), own_offset) {
                self.shape_content(child, md)?;
            }
        }
        Ok(())
    }

    fn svg_blip_part(&self, shape: &Element) -> Option<&str> {
        fn find<'e>(e: &'e Element, out: &mut Vec<&'e Element>) {
            for c in e.elements() {
                if &*c.name == "asvg:svgBlip" {
                    out.push(c);
                }
                find(c, out);
            }
        }
        let mut blips = Vec::new();
        find(shape, &mut blips);
        blips
            .into_iter()
            .filter_map(|b| b.attr("r:embed").filter(|r| !r.is_empty()))
            .find_map(|r| self.related_part(r))
    }

    fn blip_part(&self, shape: &Element) -> Option<&str> {
        let rid = shape
            .path(&["p:blipFill", "a:blip"])
            .and_then(|b| b.attr("r:embed"))?;
        self.related_part(rid)
    }

    fn is_picture(&self, shape: &Element) -> bool {
        let is_placeholder = ph(shape).is_some();
        match shape.name.as_ref() {
            "p:pic" if !is_placeholder => shape
                .path(&["p:nvPicPr", "p:nvPr", "a:videoFile"])
                .is_none(),
            "p:pic" => self.blip_part(shape).is_some() || self.svg_blip_part(shape).is_some(),
            "p:sp" if is_placeholder => self.svg_blip_part(shape).is_some(),
            _ => false,
        }
    }

    fn picture(&self, shape: &Element) -> String {
        let c_nv_pr = shape.elements().next().and_then(|nv| nv.child("p:cNvPr"));
        let shape_name = c_nv_pr.and_then(|c| c.attr("name")).unwrap_or_default();
        let descr = c_nv_pr.and_then(|c| c.attr("descr")).unwrap_or_default();
        let alt = if pystr::strip(descr).is_empty() {
            shape_name
        } else {
            descr
        };
        let alt: String = alt
            .chars()
            .map(|c| {
                if matches!(c, '\r' | '\n' | '[' | ']') {
                    ' '
                } else {
                    c
                }
            })
            .collect();
        let mut squeezed = String::new();
        let mut in_ws = false;
        for c in alt.chars() {
            if pystr::is_space(c) {
                if !in_ws {
                    squeezed.push(' ');
                }
                in_ws = true;
            } else {
                squeezed.push(c);
                in_ws = false;
            }
        }
        let alt = pystr::strip(&squeezed);
        if self.opts.keep_data_uris {
            let part = self.blip_part(shape).or_else(|| self.svg_blip_part(shape));
            if let Some(bytes) = part.and_then(|p| self.images.get(p)) {
                let ct = part
                    .and_then(|p| self.content_type(p))
                    .unwrap_or_else(|| "image/png".into());
                return format!(
                    "\n![{alt}](data:{ct};base64,{})\n",
                    crate::base64::encode(bytes)
                );
            }
        }
        let filename: String = shape_name
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        format!("\n![{alt}]({filename}.jpg)\n")
    }

    fn content_type(&self, part: &str) -> Option<String> {
        let ct = self.content_types?;
        let over = ct
            .children_named("content-types:Override")
            .find(|o| o.attr("PartName").map(|p| p.trim_start_matches('/')) == Some(part));
        if let Some(t) = over.and_then(|o| o.attr("ContentType")) {
            return Some(t.to_string());
        }
        let ext = part.rsplit('.').next()?.to_ascii_lowercase();
        ct.children_named("content-types:Default")
            .find(|d| {
                d.attr("Extension").map(str::to_ascii_lowercase).as_deref() == Some(ext.as_str())
            })
            .and_then(|d| d.attr("ContentType"))
            .map(str::to_string)
    }

    fn chart(&self, shape: &Element) -> Result<String, Error> {
        let part = shape
            .path(&["a:graphic", "a:graphicData", "c:chart"])
            .and_then(|c| c.attr("r:id"))
            .and_then(|id| self.related_part(id));
        let chart = part
            .and_then(|p| self.parts.get(p))
            .and_then(Option::as_ref)
            .and_then(|cs| cs.child("c:chart"));
        let Some(chart) = chart else {
            return Ok("\n\n[unsupported chart]\n\n".into());
        };
        match chart_markdown(chart) {
            Ok(md) => Ok(md),
            Err(ChartError::Unsupported) => Ok("\n\n[unsupported chart]\n\n".into()),
            Err(ChartError::Value(e)) => Err(Error::Xml(format!("chart: {e}"))),
        }
    }
}

fn table(frame: &Element) -> String {
    let Some(tbl) = frame.path(&["a:graphic", "a:graphicData", "a:tbl"]) else {
        return "\n".into();
    };
    let mut dom = Dom::new("[document]");
    let html = dom.element(markdownify::ROOT, "html", Vec::new());
    let body = dom.element(html, "body", Vec::new());
    let table = dom.element(body, "table", Vec::new());
    for (i, row) in tbl.children_named("a:tr").enumerate() {
        let tr = dom.element(table, "tr", Vec::new());
        for cell in row.children_named("a:tc") {
            let td = dom.element(tr, if i == 0 { "th" } else { "td" }, Vec::new());
            dom.text(td, frame_text(cell.child("a:txBody")));
        }
    }
    let md = markdownify::convert(
        &dom,
        body,
        &markdownify::Options {
            keep_data_uris: false,
        },
    );
    format!("{}\n", pystr::strip(&md))
}

enum ChartError {
    /// what markitdown prints as "[unsupported chart]"
    Unsupported,
    /// a ValueError that escapes markitdown's handler and fails the file
    Value(String),
}

const PLOT_TAGS: &[&str] = &[
    "c:area3DChart",
    "c:areaChart",
    "c:bar3DChart",
    "c:barChart",
    "c:bubbleChart",
    "c:doughnutChart",
    "c:line3DChart",
    "c:lineChart",
    "c:ofPieChart",
    "c:pie3DChart",
    "c:pieChart",
    "c:radarChart",
    "c:scatterChart",
    "c:stockChart",
    "c:surface3DChart",
    "c:surfaceChart",
];

fn descendants<'e>(e: &'e Element, name: &str, out: &mut Vec<&'e Element>) {
    for c in e.elements() {
        if &*c.name == name {
            out.push(c);
        }
        descendants(c, name, out);
    }
}

fn first_descendant<'e>(e: &'e Element, name: &str) -> Option<&'e Element> {
    let mut v = Vec::new();
    descendants(e, name, &mut v);
    v.into_iter().next()
}

fn parse_int(v: Option<&str>) -> Result<i64, ChartError> {
    let v = v.unwrap_or_default();
    v.trim()
        .parse()
        .map_err(|_| ChartError::Value(format!("invalid integer {v:?}")))
}

fn chart_markdown(chart: &Element) -> Result<String, ChartError> {
    let mut md = "\n\n### Chart".to_string();
    if let Some(rich) = chart.path(&["c:title", "c:tx", "c:rich"]) {
        md.push_str(": ");
        md.push_str(&frame_text(Some(rich)));
    }
    md.push_str("\n\n");
    let plot_area = chart.child("c:plotArea").ok_or(ChartError::Unsupported)?;
    let x_charts: Vec<&Element> = plot_area
        .elements()
        .filter(|e| PLOT_TAGS.contains(&e.name.as_ref()))
        .collect();
    let first = x_charts.first().ok_or(ChartError::Unsupported)?;
    let supported_plots = [
        "c:areaChart",
        "c:area3DChart",
        "c:barChart",
        "c:bubbleChart",
        "c:doughnutChart",
        "c:lineChart",
        "c:pieChart",
        "c:radarChart",
        "c:scatterChart",
    ];
    if !supported_plots.contains(&first.name.as_ref()) {
        return Err(ChartError::Unsupported);
    }

    // categories of the first plot
    let mut counts = Vec::new();
    for ser in first.children_named("c:ser") {
        if let Some(cat) = ser.child("c:cat") {
            descendants(cat, "c:ptCount", &mut counts);
        }
    }
    let cat_count = match counts.first() {
        Some(c) => parse_int(c.attr("val"))?,
        None => 0,
    };
    let mut cat_pts = Vec::new();
    if let Some(cat) = first.child("c:ser").and_then(|s| s.child("c:cat")) {
        let mut lvls = Vec::new();
        first_lvls(cat, &mut lvls);
        for l in lvls {
            cat_pts.extend(l.children_named("c:pt"));
        }
        if cat_pts.is_empty() {
            descendants(cat, "c:pt", &mut cat_pts);
        }
    }
    let mut by_idx: HashMap<i64, &Element> = HashMap::new();
    for pt in cat_pts {
        by_idx.insert(parse_int(pt.attr("idx"))?, pt);
    }
    let categories: Vec<String> = (0..cat_count)
        .map(|i| match by_idx.get(&i) {
            None => String::new(),
            Some(pt) => pt
                .child("c:v")
                .map(|v| {
                    if v.children.is_empty() {
                        "None".into()
                    } else {
                        leading_text(v)
                    }
                })
                .unwrap_or_default(),
        })
        .collect();

    // every series of every plot, by c:order within each plot
    let mut names = Vec::new();
    let mut values: Vec<Vec<String>> = Vec::new();
    for xc in &x_charts {
        let value_tag = match xc.name.as_ref() {
            "c:areaChart" | "c:barChart" | "c:doughnutChart" | "c:lineChart" | "c:pieChart"
            | "c:radarChart" => "c:val",
            "c:bubbleChart" | "c:scatterChart" => "c:yVal",
            _ => return Err(ChartError::Unsupported), // NotImplementedError
        };
        let mut sers: Vec<(i64, &Element)> = Vec::new();
        for s in xc.children_named("c:ser") {
            let order = s.child("c:order").ok_or(ChartError::Unsupported)?;
            sers.push((parse_int(order.attr("val"))?, s));
        }
        sers.sort_by_key(|(o, _)| *o);
        for (_, ser) in sers {
            names.push(series_name(ser));
            values.push(series_values(ser.child(value_tag))?);
        }
    }

    let mut rows = vec![format!("| Category | {} |", names.join(" | "))];
    if names.is_empty() {
        rows[0] = "| Category |".into();
    }
    for (i, cat) in categories.iter().enumerate() {
        let mut row = vec![cat.clone()];
        for v in &values {
            row.push(v.get(i).cloned().unwrap_or_else(|| "None".into()));
        }
        rows.push(format!("| {} |", row.join(" | ")));
    }
    let separator = format!("|{}|", vec!["---"; names.len() + 1].join("|"));
    rows.insert(1, separator);
    Ok(md + &rows.join("\n"))
}

/// `c:lvl[1]` descendants: the first `c:lvl` child of any element.
fn first_lvls<'e>(e: &'e Element, out: &mut Vec<&'e Element>) {
    if let Some(l) = e.child("c:lvl") {
        out.push(l);
    }
    for c in e.elements() {
        first_lvls(c, out);
    }
}

/// `./c:tx//c:pt/c:v/text()`, first match.
fn series_name(ser: &Element) -> String {
    let Some(tx) = ser.child("c:tx") else {
        return String::new();
    };
    let mut pts = Vec::new();
    descendants(tx, "c:pt", &mut pts);
    pts.iter()
        .flat_map(|pt| pt.children_named("c:v"))
        .flat_map(|v| v.children.iter())
        .find_map(|n| {
            if let Node::Text(t) = n {
                Some(t.clone())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn series_values(val: Option<&Element>) -> Result<Vec<String>, ChartError> {
    let Some(val) = val else {
        return Ok(Vec::new());
    };
    let count = match first_descendant(val, "c:ptCount") {
        Some(c) => parse_int(c.attr("val"))?,
        None => 0,
    };
    let mut pts = Vec::new();
    descendants(val, "c:pt", &mut pts);
    let mut out = Vec::new();
    for i in 0..count {
        let mut found = None;
        for pt in &pts {
            if parse_int(pt.attr("idx"))? == i {
                found = Some(*pt);
                break;
            }
        }
        out.push(match found {
            None => "None".to_string(),
            Some(pt) => {
                let text = pt.child("c:v").map(leading_text).unwrap_or_default();
                let f: f64 = pystr::strip(&text).replace('_', "").parse().map_err(|_| {
                    ChartError::Value(format!("could not convert string to float: {text:?}"))
                })?;
                py_float(f)
            }
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::py_float;

    #[test]
    fn float_repr_matches_python() {
        let cases = [
            (1.0, "1.0"),
            (4.3, "4.3"),
            (-2.5, "-2.5"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1e16, "1e+16"),
            (123456789012345680.0, "1.2345678901234568e+17"),
            (1234567890123456.0, "1234567890123456.0"),
            (0.1 + 0.2, "0.30000000000000004"),
            (100.0, "100.0"),
        ];
        for (v, want) in cases {
            assert_eq!(py_float(v), want, "{v}");
        }
    }
}
