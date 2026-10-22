//! pdfminer's PDFPageInterpreter driving a PDFLayoutAnalyzer: runs a
//! page's content streams and collects the LTChar items, nested in the
//! LTFigures of form XObjects. Paths and images make no text and are
//! skipped.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::document::Document;
use super::font::{Disp, Font, Fonts};
use super::lexer::{Keywords, Lexer, Stack, Ty};
use super::object::{
    Dict, Obj, PdfError, Result, Stream, literal_name, other, safe_float, safe_int,
};
use super::page::Page;

pub type Matrix = [f64; 6];
const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

pub fn mult_matrix(m1: &Matrix, m0: &Matrix) -> Matrix {
    let [a1, b1, c1, d1, e1, f1] = *m1;
    let [a0, b0, c0, d0, e0, f0] = *m0;
    [
        a0 * a1 + c0 * b1,
        b0 * a1 + d0 * b1,
        a0 * c1 + c0 * d1,
        b0 * c1 + d0 * d1,
        a0 * e1 + c0 * f1 + e0,
        b0 * e1 + d0 * f1 + f0,
    ]
}

fn apply_pt(m: &Matrix, x: f64, y: f64) -> (f64, f64) {
    let [a, b, c, d, e, f] = *m;
    (a * x + c * y + e, b * x + d * y + f)
}

/// Python's min()/max(): the first of equal values wins.
fn py_min(v: [f64; 4]) -> f64 {
    v.into_iter().fold(v[0], |m, x| if x < m { x } else { m })
}

fn py_max(v: [f64; 4]) -> f64 {
    v.into_iter().fold(v[0], |m, x| if x > m { x } else { m })
}

pub fn apply_rect(m: &Matrix, r: [f64; 4]) -> [f64; 4] {
    let [x0, y0, x1, y1] = r;
    let (l1, b1) = apply_pt(m, x0, y0);
    let (r1, b2) = apply_pt(m, x1, y0);
    let (r2, t1) = apply_pt(m, x1, y1);
    let (l2, t2) = apply_pt(m, x0, y1);
    [
        py_min([l1, l2, r1, r2]),
        py_min([b1, b2, t1, t2]),
        py_max([l1, l2, r1, r2]),
        py_max([b1, b2, t1, t2]),
    ]
}

/// The LTChar fields text extraction reads.
#[derive(Clone, Debug)]
pub struct Char {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub width: f64,
    pub height: f64,
    pub text: String,
    pub upright: bool,
}

#[derive(Debug)]
pub enum Item {
    Char(Char),
    Figure(Vec<Item>),
}

/// An LTPage: its bbox and items in drawing order.
pub struct PageItems {
    pub bbox: [f64; 4],
    pub items: Vec<Item>,
}

/// PDFLayoutAnalyzer state shared by nested interpreters.
struct Device {
    ctm: Matrix,
    stack: Vec<Vec<Item>>,
    cur: Vec<Item>,
}

#[derive(Clone)]
struct TextState {
    font: Option<Rc<Font>>,
    fontsize: f64,
    charspace: f64,
    wordspace: f64,
    scaling: f64,
    leading: f64,
    rise: f64,
    matrix: Matrix,
    linematrix: (f64, f64),
}

impl TextState {
    fn new() -> Self {
        TextState {
            font: None,
            fontsize: 0.0,
            charspace: 0.0,
            wordspace: 0.0,
            scaling: 100.0,
            leading: 0.0,
            rise: 0.0,
            matrix: IDENTITY,
            linematrix: (0.0, 0.0),
        }
    }
}

#[derive(Clone)]
struct ColorSpace {
    name: String,
    /// raw /N; only ints can size the operand pop
    ncomponents: Option<i64>,
}

fn predefined_colorspaces() -> HashMap<String, ColorSpace> {
    [
        ("DeviceGray", 1),
        ("CalRGB", 3),
        ("CalGray", 1),
        ("Lab", 3),
        ("DeviceRGB", 3),
        ("DeviceCMYK", 4),
        ("Separation", 1),
        ("Indexed", 1),
        ("Pattern", 1),
    ]
    .into_iter()
    .map(|(n, c)| {
        (
            n.to_string(),
            ColorSpace {
                name: n.to_string(),
                ncomponents: Some(c),
            },
        )
    })
    .collect()
}

/// PDFContentParser's keyword handling: inline images.
struct ContentKw;

impl Keywords for ContentKw {
    fn do_keyword(&mut self, st: &mut Stack, pos: usize, kw: Rc<[u8]>) -> Result<()> {
        match &*kw {
            b"BI" => st.start_type(pos, Ty::Inline),
            b"ID" => {
                let Some((_, objs)) = st.end_type(Ty::Inline) else {
                    return Ok(());
                };
                if objs.len() % 2 != 0 {
                    return Ok(());
                }
                let mut d = Dict::default();
                for p in objs.as_chunks::<2>().0 {
                    d.set(Rc::from(literal_name(&p[0]).into_bytes()), p[1].clone());
                }
                let mut eos: &[u8] = b"EI";
                if let Some(f) = d.get("F") {
                    let first = match f {
                        Obj::Name(_) => f.clone(),
                        Obj::Array(a) => match a.first() {
                            Some(x) => x.clone(),
                            None => return other("list index out of range"),
                        },
                        Obj::Str(s) if !s.is_empty() => Obj::Int(s[0] as i64),
                        _ => return other("object is not subscriptable"),
                    };
                    if first.is_name("ASCII85Decode") || first.is_name("A85") {
                        eos = b"~>";
                    }
                }
                st.reset();
                let (pos, mut data) = st.lex.get_inline_data(pos + 3, eos)?;
                if eos != b"EI" {
                    data.extend_from_slice(eos);
                }
                st.push((pos, Obj::Stream(Rc::new(Stream::new(d, data, false)))));
                if eos == b"EI" {
                    st.push((pos, Obj::Kw(Rc::from(&b"EI"[..]))));
                }
            }
            _ => st.push((pos, Obj::Kw(kw))),
        }
        Ok(())
    }

    fn flushes(&self) -> bool {
        true
    }
}

/// Operator name (after pdfminer's `*`, `"`, `'` renaming) to arity.
fn arity(name: &str) -> Option<usize> {
    Some(match name {
        "q" | "Q" | "h" | "S" | "s" | "f" | "F" | "f_a" | "B" | "B_a" | "b" | "b_a" | "n" | "W"
        | "W_a" | "SCN" | "scn" | "SC" | "sc" | "BT" | "ET" | "BX" | "EX" | "EMC" | "T_a"
        | "BI" | "ID" => 0,
        "w" | "J" | "j" | "M" | "ri" | "i" | "gs" | "CS" | "cs" | "G" | "g" | "sh" | "MP"
        | "BMC" | "Tc" | "Tw" | "Tz" | "TL" | "Tr" | "Ts" | "TJ" | "Tj" | "_q" | "EI" | "Do" => 1,
        "d" | "m" | "l" | "DP" | "BDC" | "Tf" | "Td" | "TD" => 2,
        "RG" | "rg" | "_w" => 3,
        "v" | "y" | "re" | "K" | "k" => 4,
        "cm" | "c" | "Tm" => 6,
        _ => return None,
    })
}

fn safe_matrix(args: &[Obj]) -> Option<Matrix> {
    let mut m = [0.0; 6];
    for (slot, a) in m.iter_mut().zip(args) {
        *slot = safe_float(a)?;
    }
    Some(m)
}

fn num_list<const N: usize>(doc: &Document, o: &Obj, what: &str) -> Result<[f64; N]> {
    let list = doc.list_value(o)?;
    if list.len() != N {
        return other(format!("cannot unpack {what}"));
    }
    let mut out = [0.0; N];
    for (slot, v) in out.iter_mut().zip(list.iter()) {
        *slot = v
            .as_num()
            .ok_or_else(|| PdfError::Other(format!("non-numeric {what}")))?;
    }
    Ok(out)
}

struct Interp<'a> {
    doc: &'a Document,
    fonts: &'a mut Fonts,
    dev: &'a mut Device,
    resources: Obj,
    fontmap: HashMap<String, Rc<Font>>,
    xobjmap: HashMap<String, Obj>,
    csmap: HashMap<String, ColorSpace>,
    gstack: Vec<(Matrix, TextState, ColorSpace, ColorSpace)>,
    ctm: Matrix,
    ts: TextState,
    scs: ColorSpace,
    ncs: ColorSpace,
    argstack: Vec<Obj>,
    parent_ids: HashSet<i64>,
    stream_ids: HashSet<i64>,
    depth: u32,
}

impl<'a> Interp<'a> {
    fn new(
        doc: &'a Document,
        fonts: &'a mut Fonts,
        dev: &'a mut Device,
        parent_ids: HashSet<i64>,
        depth: u32,
    ) -> Self {
        let csmap = predefined_colorspaces();
        let gray = csmap["DeviceGray"].clone();
        Interp {
            doc,
            fonts,
            dev,
            resources: Obj::Null,
            fontmap: HashMap::new(),
            xobjmap: HashMap::new(),
            csmap,
            gstack: Vec::new(),
            ctm: IDENTITY,
            ts: TextState::new(),
            scs: gray.clone(),
            ncs: gray,
            argstack: Vec::new(),
            parent_ids,
            stream_ids: HashSet::new(),
            depth,
        }
    }

    fn init_resources(&mut self, resources: Obj) -> Result<()> {
        self.resources = resources.clone();
        if !resources.truthy() {
            return Ok(());
        }
        let doc = self.doc;
        for (k, v) in &doc.dict_value(&resources)?.0 {
            match &**k {
                b"Font" => {
                    for (fontid, spec) in &doc.dict_value(v)?.0 {
                        let objid = match spec {
                            Obj::Ref(id) => Some(*id),
                            _ => None,
                        };
                        let spec = doc.dict_value(spec)?;
                        let font = self.fonts.get(doc, objid, &spec)?;
                        self.fontmap
                            .insert(String::from_utf8_lossy(fontid).into_owned(), font);
                    }
                }
                b"ColorSpace" => {
                    for (csid, spec) in &doc.dict_value(v)?.0 {
                        if let Some(cs) = self.colorspace(&doc.resolve(spec)?)? {
                            self.csmap
                                .insert(String::from_utf8_lossy(csid).into_owned(), cs);
                        }
                    }
                }
                b"ProcSet" => {
                    doc.list_value(v)?;
                }
                b"XObject" => {
                    for (id, x) in &doc.dict_value(v)?.0 {
                        self.xobjmap
                            .insert(String::from_utf8_lossy(id).into_owned(), x.clone());
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn colorspace(&self, spec: &Obj) -> Result<Option<ColorSpace>> {
        let (name, list) = match spec {
            Obj::Array(a) => match a.first() {
                Some(n) => (literal_name(n), Some(a.clone())),
                None => return other("list index out of range"),
            },
            s => (literal_name(s), None),
        };
        if let Some(list) = list.filter(|l| l.len() >= 2) {
            if name == "ICCBased" {
                let s = self.doc.stream_value(&list[1])?;
                let Some(n) = s.attrs.get("N") else {
                    return other("PDF stream object does not have attribute 'N'");
                };
                return Ok(Some(ColorSpace {
                    name,
                    ncomponents: n.as_int(),
                }));
            }
            if name == "DeviceN" {
                let n = self.doc.list_value(&list[1])?.len() as i64;
                return Ok(Some(ColorSpace {
                    name,
                    ncomponents: Some(n),
                }));
            }
        }
        Ok(predefined_colorspaces().remove(&name))
    }

    /// `self.pop(n)` with Python's slicing for any n.
    fn pop(&mut self, n: i64) -> Vec<Obj> {
        if n == 0 {
            return Vec::new();
        }
        let len = self.argstack.len();
        let k = if n > 0 {
            len.saturating_sub(n as usize)
        } else {
            ((-n) as usize).min(len)
        };
        self.argstack.split_off(k)
    }

    fn set_ctm(&mut self, ctm: Matrix) {
        self.ctm = ctm;
        self.dev.ctm = ctm;
    }

    fn render_contents(&mut self, resources: Obj, streams: Vec<Obj>, ctm: Matrix) -> Result<()> {
        self.init_resources(resources)?;
        self.set_ctm(ctm);
        self.execute(streams)
    }

    fn execute(&mut self, streams: Vec<Obj>) -> Result<()> {
        self.stream_ids.clear();
        let mut srcs = Vec::new();
        for obj in &streams {
            let s = self.doc.stream_value(obj)?;
            let Some(id) = s.objid() else {
                continue;
            };
            if self.parent_ids.contains(&id) {
                continue;
            }
            self.stream_ids.insert(id);
            srcs.push(self.doc.stream_data(&s));
        }
        let mut st = match Lexer::content(srcs) {
            Ok(lex) => Stack::new(lex),
            Err(PdfError::Eof) => return Ok(()),
            Err(e) => return Err(e),
        };
        loop {
            let obj = match st.nextobject(&mut ContentKw) {
                Ok((_, o)) => o,
                Err(PdfError::Eof) => return Ok(()),
                Err(e) => return Err(e),
            };
            let Obj::Kw(k) = &obj else {
                self.argstack.push(obj);
                continue;
            };
            let name = String::from_utf8_lossy(k).replace('\u{fffd}', "");
            let name = name
                .replace('*', "_a")
                .replace('"', "_w")
                .replace('\'', "_q");
            let Some(n) = arity(&name) else {
                continue;
            };
            let args = if n > 0 {
                self.pop(n as i64)
            } else {
                Vec::new()
            };
            if args.len() == n {
                self.op(&name, args)?;
            }
        }
    }

    fn op(&mut self, name: &str, a: Vec<Obj>) -> Result<()> {
        let f = |i: usize| safe_float(&a[i]);
        match name {
            "q" => self.gstack.push((
                self.ctm,
                self.ts.clone(),
                self.scs.clone(),
                self.ncs.clone(),
            )),
            "Q" => {
                if let Some((ctm, ts, scs, ncs)) = self.gstack.pop() {
                    self.ts = ts;
                    self.scs = scs;
                    self.ncs = ncs;
                    self.set_ctm(ctm);
                }
            }
            "cm" => {
                if let Some(m) = safe_matrix(&a) {
                    let ctm = mult_matrix(&m, &self.ctm);
                    self.set_ctm(ctm);
                }
            }
            "CS" | "cs" => {
                if let Some(cs) = self.csmap.get(&literal_name(&a[0])).cloned() {
                    if name == "CS" {
                        self.scs = cs
                    } else {
                        self.ncs = cs
                    }
                }
            }
            "G" | "g" | "RG" | "rg" | "K" | "k" => {
                if a.iter().all(|x| safe_float(x).is_some()) {
                    let key = match name {
                        "G" | "g" => "DeviceGray",
                        "RG" | "rg" => "DeviceRGB",
                        _ => "DeviceCMYK",
                    };
                    let Some(cs) = self.csmap.get(key).cloned() else {
                        return other(format!("KeyError: '{key}'"));
                    };
                    if name.chars().next().unwrap().is_ascii_uppercase() {
                        self.scs = cs
                    } else {
                        self.ncs = cs
                    }
                }
            }
            "SCN" | "SC" | "scn" | "sc" => {
                let cs = if name.starts_with('S') {
                    &self.scs
                } else {
                    &self.ncs
                };
                let Some(n) = cs.ncomponents else {
                    return other("slice indices must be integers");
                };
                let _ = &cs.name;
                self.pop(n);
            }
            "BT" => {
                self.ts.matrix = IDENTITY;
                self.ts.linematrix = (0.0, 0.0);
            }
            "Tc" => {
                if let Some(v) = f(0) {
                    self.ts.charspace = v;
                }
            }
            "Tw" => {
                if let Some(v) = f(0) {
                    self.ts.wordspace = v;
                }
            }
            "Tz" => {
                if let Some(v) = f(0) {
                    self.ts.scaling = v;
                }
            }
            "TL" => {
                if let Some(v) = f(0) {
                    self.ts.leading = -v;
                }
            }
            "Tf" => {
                self.ts.font = Some(match self.fontmap.get(&literal_name(&a[0])) {
                    Some(font) => font.clone(),
                    None => self.fonts.get(self.doc, None, &Dict::default())?,
                });
                if let Some(v) = f(1) {
                    self.ts.fontsize = v;
                }
            }
            "Tr" => {
                safe_int(&a[0])?;
            }
            "Ts" => {
                if let Some(v) = f(0) {
                    self.ts.rise = v;
                }
            }
            "Td" | "TD" => {
                let (tx, ty) = (f(0), f(1));
                if let (Some(tx), Some(ty)) = (tx, ty) {
                    let [a_, b, c, d, e, f_] = self.ts.matrix;
                    self.ts.matrix = [a_, b, c, d, tx * a_ + ty * c + e, tx * b + ty * d + f_];
                }
                if name == "TD"
                    && let Some(ty) = ty
                {
                    self.ts.leading = ty;
                }
                self.ts.linematrix = (0.0, 0.0);
            }
            "Tm" => {
                if let Some(m) = safe_matrix(&a) {
                    self.ts.matrix = m;
                    self.ts.linematrix = (0.0, 0.0);
                }
            }
            "T_a" => self.next_line(),
            "TJ" => self.show(&a[0])?,
            "Tj" => self.show(&Obj::Array(Rc::new(a)))?,
            "_q" => {
                self.next_line();
                self.show(&Obj::Array(Rc::new(a)))?;
            }
            "_w" => {
                if let Some(v) = f(0) {
                    self.ts.wordspace = v;
                }
                if let Some(v) = f(1) {
                    self.ts.charspace = v;
                }
                self.show(&Obj::Array(Rc::new(vec![a[2].clone()])))?;
            }
            "Do" => self.do_xobject(&a[0])?,
            _ => {}
        }
        Ok(())
    }

    fn next_line(&mut self) {
        let [a, b, c, d, e, f] = self.ts.matrix;
        let l = self.ts.leading;
        self.ts.matrix = [a, b, c, d, l * c + e, l * d + f];
        self.ts.linematrix = (0.0, 0.0);
    }

    fn do_xobject(&mut self, id: &Obj) -> Result<()> {
        let id = literal_name(id);
        let Some(x) = self.xobjmap.get(&id).cloned() else {
            return Ok(());
        };
        let xobj = self.doc.stream_value(&x)?;
        let is_form = xobj.attrs.get("Subtype").is_some_and(|s| s.is_name("Form"));
        if !is_form || !xobj.attrs.contains("BBox") {
            return Ok(());
        }
        let doc = self.doc;
        let bbox: [f64; 4] = num_list(doc, xobj.attrs.get("BBox").unwrap(), "BBox")?;
        let matrix: Matrix = match xobj.attrs.get("Matrix") {
            Some(m) => num_list(doc, m, "Matrix")?,
            None => IDENTITY,
        };
        let resources = match xobj.attrs.get("Resources") {
            Some(r) if r.truthy() => Obj::Dict(doc.dict_value(r)?),
            _ => self.resources.clone(),
        };
        // LTFigure(name, bbox, mult_matrix(matrix, device.ctm))
        let _ = (bbox, mult_matrix(&matrix, &self.dev.ctm));
        let parent = std::mem::take(&mut self.dev.cur);
        self.dev.stack.push(parent);
        if self.depth > 100 {
            return other("maximum recursion depth exceeded");
        }
        let mut ids = self.parent_ids.clone();
        ids.extend(self.stream_ids.iter().copied());
        let ctm = mult_matrix(&matrix, &self.ctm);
        let mut sub = Interp::new(doc, self.fonts, self.dev, ids, self.depth + 1);
        sub.render_contents(resources, vec![Obj::Stream(xobj)], ctm)?;
        let fig = std::mem::take(&mut self.dev.cur);
        self.dev.cur = self.dev.stack.pop().unwrap_or_default();
        self.dev.cur.push(Item::Figure(fig));
        Ok(())
    }

    /// do_TJ: PDFTextDevice.render_string over the operand sequence.
    fn show(&mut self, seq: &Obj) -> Result<()> {
        let Some(font) = self.ts.font.clone() else {
            return Ok(());
        };
        let items: Vec<Obj> = match seq {
            Obj::Array(a) => a.to_vec(),
            Obj::Str(s) => s.iter().map(|&b| Obj::Int(b as i64)).collect(),
            Obj::Dict(_) => Vec::new(),
            _ => return other("object is not iterable"),
        };
        let ts = &self.ts;
        let matrix = mult_matrix(&ts.matrix, &self.dev.ctm);
        let fontsize = ts.fontsize;
        let scaling = ts.scaling * 0.01;
        let charspace = ts.charspace * scaling;
        let mut wordspace = ts.wordspace * scaling;
        let rise = ts.rise;
        if font.is_multibyte() {
            wordspace = 0.0;
        }
        let dxscale = 0.001 * fontsize * scaling;
        let vertical = font.is_vertical();
        let (mut x, mut y) = ts.linematrix;
        let mut needcharspace = false;
        for obj in &items {
            if let Some(n) = obj.as_num() {
                if vertical {
                    y -= n * dxscale
                } else {
                    x -= n * dxscale
                }
                needcharspace = true;
            } else if let Obj::Str(s) = obj {
                for cid in font.decode(s) {
                    if needcharspace {
                        if vertical {
                            y += charspace
                        } else {
                            x += charspace
                        }
                    }
                    let [a, b, c, d, e, f] = matrix;
                    let m = [a, b, c, d, x * a + y * c + e, x * b + y * d + f];
                    let adv = self.render_char(&m, &font, fontsize, scaling, rise, cid as i64)?;
                    if vertical {
                        y += adv
                    } else {
                        x += adv
                    }
                    if cid == 32 && wordspace != 0.0 {
                        if vertical {
                            y += wordspace
                        } else {
                            x += wordspace
                        }
                    }
                    needcharspace = true;
                }
            }
        }
        self.ts.linematrix = (x, y);
        Ok(())
    }

    /// PDFLayoutAnalyzer.render_char: one LTChar.
    fn render_char(
        &mut self,
        m: &Matrix,
        font: &Font,
        fontsize: f64,
        scaling: f64,
        rise: f64,
        cid: i64,
    ) -> Result<f64> {
        let text = font
            .to_unichr(cid)
            .unwrap_or_else(|| format!("(cid:{cid})"));
        let textwidth = font.char_width(cid)?;
        let adv = textwidth * fontsize * scaling;
        let bbox = match font.char_disp(cid) {
            Disp::Vertical(vx, vy) => {
                let vx = match vx {
                    None => fontsize * 0.5,
                    Some(v) => v * fontsize * 0.001,
                };
                let vy = (1000.0 - vy) * fontsize * 0.001;
                [-vx, vy + rise + adv, -vx + fontsize, vy + rise]
            }
            Disp::Horizontal => {
                let descent = font.get_descent() * fontsize;
                [0.0, descent + rise, adv, descent + rise + fontsize]
            }
        };
        let [a, b, c, d, _, _] = *m;
        let upright = a * d * scaling > 0.0 && b * c <= 0.0;
        let [mut x0, mut y0, mut x1, mut y1] = apply_rect(m, bbox);
        if x1 < x0 {
            std::mem::swap(&mut x0, &mut x1);
        }
        if y1 < y0 {
            std::mem::swap(&mut y0, &mut y1);
        }
        let (width, height) = (x1 - x0, y1 - y0);
        self.dev.cur.push(Item::Char(Char {
            x0,
            y0,
            x1,
            y1,
            width,
            height,
            text,
            upright,
        }));
        Ok(adv)
    }
}

/// `PDFPageInterpreter.process_page()`
pub fn process_page(doc: &Document, fonts: &mut Fonts, page: &Page) -> Result<PageItems> {
    let [x0, y0, x1, y1] = page.mediabox;
    let ctm = match page.rotate {
        90 => [0.0, -1.0, 1.0, 0.0, -y0, x1],
        180 => [-1.0, 0.0, 0.0, -1.0, x1, y1],
        270 => [0.0, 1.0, -1.0, 0.0, y1, -x0],
        _ => [1.0, 0.0, 0.0, 1.0, -x0, -y0],
    };
    let [bx0, by0, bx1, by1] = apply_rect(&ctm, page.mediabox);
    let bbox = [0.0, 0.0, (bx0 - bx1).abs(), (by0 - by1).abs()];
    let mut dev = Device {
        ctm,
        stack: Vec::new(),
        cur: Vec::new(),
    };
    let mut interp = Interp::new(doc, fonts, &mut dev, HashSet::new(), 0);
    interp.render_contents(page.resources.clone(), page.contents.clone(), ctm)?;
    Ok(PageItems {
        bbox,
        items: dev.cur,
    })
}
