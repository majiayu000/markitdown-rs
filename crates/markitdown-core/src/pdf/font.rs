//! pdfminer's pdffont: what text extraction needs from each font kind
//! (code decoding, Unicode mapping, widths, descent and displacement).

use std::collections::HashMap;
use std::rc::Rc;

use super::cmap::{CMap, UnicodeMap, parse_tounicode};
use super::document::Document;
use super::encoding::{Cid2Unicode, NameError, get_encoding, name2unicode};
use super::lexer::{Keywords, Lexer, Stack};
use super::metrics::{FONT_ALIASES, FONT_METRICS};
use super::object::{Dict, Obj, PdfError, Result, literal_name, other, safe_float};
use crate::pystr;

/// AFM metrics of a standard-14 font (generated in metrics.rs).
pub struct Metrics {
    pub descent: f64,
    pub bbox: [f64; 4],
    pub widths: &'static [(&'static str, i64)],
}

fn metrics(name: &str) -> Option<&'static Metrics> {
    let name = FONT_ALIASES
        .iter()
        .find(|(a, _)| *a == name)
        .map_or(name, |(_, b)| b);
    FONT_METRICS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, m)| m)
}

pub enum Disp {
    Horizontal,
    /// (vx, vy); vx None means half the font size
    Vertical(Option<f64>, f64),
}

enum Kind {
    Simple {
        cid2unicode: Rc<Cid2Unicode>,
        unicode_map: Option<UnicodeMap>,
    },
    Cid {
        cmap: CMap,
        unicode_map: Option<UnicodeMap>,
        disps: HashMap<i64, (f64, f64)>,
        default_vy: Option<f64>,
    },
}

pub struct Font {
    /// widths by code, as `safe_float()` of each value
    widths: HashMap<i64, Option<f64>>,
    /// standard-14 widths, keyed by the character
    std_widths: Option<&'static [(&'static str, i64)]>,
    /// None when the default width is not a number (a TypeError if used)
    default_width: Option<f64>,
    descent: f64,
    hscale: f64,
    vscale: f64,
    kind: Kind,
}

impl Font {
    pub fn is_vertical(&self) -> bool {
        matches!(&self.kind, Kind::Cid { cmap, .. } if cmap.is_vertical())
    }

    pub fn is_multibyte(&self) -> bool {
        matches!(self.kind, Kind::Cid { .. })
    }

    pub fn decode(&self, s: &[u8]) -> Vec<u32> {
        match &self.kind {
            Kind::Simple { .. } => s.iter().map(|&b| b as u32).collect(),
            Kind::Cid { cmap, .. } => cmap.decode(s),
        }
    }

    pub fn get_descent(&self) -> f64 {
        self.descent * self.vscale
    }

    /// None is PDFUnicodeNotDefined.
    pub fn to_unichr(&self, cid: i64) -> Option<String> {
        match &self.kind {
            Kind::Simple {
                cid2unicode,
                unicode_map,
            } => unicode_map
                .as_ref()
                .and_then(|m| m.get(cid))
                .or_else(|| cid2unicode.get(&cid).cloned()),
            Kind::Cid { unicode_map, .. } => unicode_map.as_ref()?.get(cid),
        }
    }

    pub fn char_width(&self, cid: i64) -> Result<f64> {
        if let Some(Some(w)) = self.widths.get(&cid) {
            return Ok(w * self.hscale);
        }
        if let Some(std) = self.std_widths
            && let Some(text) = self.to_unichr(cid)
            && let Some((_, w)) = std.iter().find(|(c, _)| *c == text)
        {
            return Ok(*w as f64 * self.hscale);
        }
        match self.default_width {
            Some(d) => Ok(d * self.hscale),
            None => other("unsupported operand type(s) for *"),
        }
    }

    pub fn char_disp(&self, cid: i64) -> Disp {
        match &self.kind {
            Kind::Cid {
                cmap,
                disps,
                default_vy,
                ..
            } if cmap.is_vertical() => match disps.get(&cid) {
                Some(&(vx, vy)) => Disp::Vertical(Some(vx), vy),
                None => Disp::Vertical(None, default_vy.unwrap_or(0.0)),
            },
            _ => Disp::Horizontal,
        }
    }
}

/// PDFResourceManager's font cache (by object id).
#[derive(Default)]
pub struct Fonts {
    cache: HashMap<i64, Rc<Font>>,
}

fn key_err<T>(k: &str) -> Result<T> {
    other(format!("KeyError: '{k}'"))
}

/// What PDFFont.__init__ reads from the descriptor.
struct Desc {
    descent: f64,
    missing_width: f64,
    bbox: [f64; 4],
}

impl Desc {
    fn from_dict(doc: &Document, d: &Dict) -> Result<Self> {
        let num = |k: &str| d.get(k).map_or(Ok(0.0), |v| doc.num_value(v));
        if let Some(n) = d.get("FontName") {
            doc.resolve(n)?;
        }
        if let Some(f) = d.get("Flags") {
            doc.int_value(f)?;
        }
        num("Ascent")?;
        num("ItalicAngle")?;
        num("Leading")?;
        Ok(Desc {
            descent: num("Descent")?,
            missing_width: num("MissingWidth")?,
            bbox: parse_bbox(doc, d.get("FontBBox"))?,
        })
    }

    fn from_metrics(m: &Metrics) -> Self {
        Desc {
            descent: m.descent,
            missing_width: 0.0,
            bbox: m.bbox,
        }
    }
}

/// `safe_rect_list(resolve_all(FontBBox))`, zeros when unusable.
fn parse_bbox(doc: &Document, v: Option<&Obj>) -> Result<[f64; 4]> {
    let Some(v) = v else {
        return Ok([0.0; 4]);
    };
    let vals: Vec<Option<f64>> = match doc.resolve(v)? {
        Obj::Array(a) => a
            .iter()
            .take(4)
            .map(|x| Ok(safe_float(&doc.resolve(x)?)))
            .collect::<Result<_>>()?,
        Obj::Str(s) => s.iter().take(4).map(|&b| Some(b as f64)).collect(),
        Obj::Dict(d) => {
            d.0.iter()
                .take(4)
                .map(|(k, _)| safe_float(&Obj::Str(k.clone())))
                .collect()
        }
        _ => return Ok([0.0; 4]),
    };
    if vals.len() != 4 || vals.iter().any(Option::is_none) {
        return Ok([0.0; 4]);
    }
    Ok([
        vals[0].unwrap(),
        vals[1].unwrap(),
        vals[2].unwrap(),
        vals[3].unwrap(),
    ])
}

/// `resolve_all()` then `safe_float()` of each width.
fn width_value(doc: &Document, v: &Obj) -> Result<Option<f64>> {
    Ok(safe_float(&doc.resolve(v)?))
}

/// An int key, or a float key that equals an int (dict lookups by int
/// find those too).
fn int_key(v: f64) -> Option<i64> {
    (v.fract() == 0.0 && v.is_finite()).then_some(v as i64)
}

fn get_widths(doc: &Document, seq: &[Obj]) -> Result<HashMap<i64, Option<f64>>> {
    let mut widths = HashMap::new();
    let mut r: Vec<Obj> = Vec::new();
    for v in seq {
        let v = doc.resolve(v)?;
        match &v {
            Obj::Array(list) => {
                if let Some(char1) = r.last().and_then(Obj::as_num) {
                    for (i, w) in list.iter().enumerate() {
                        if let Some(k) = int_key(char1 + i as f64) {
                            widths.insert(k, width_value(doc, w)?);
                        }
                    }
                    r.clear();
                }
            }
            _ if v.as_num().is_some() => {
                r.push(v.clone());
                if r.len() == 3 {
                    if let (Some(c1), Some(c2)) = (r[0].as_int(), r[1].as_int()) {
                        let w = width_value(doc, &r[2])?;
                        for c in c1..=c2.min(c1.saturating_add(1 << 20)) {
                            widths.insert(c, w);
                        }
                    }
                    r.clear();
                }
            }
            _ => {}
        }
    }
    Ok(widths)
}

type Widths2 = (HashMap<i64, Option<f64>>, HashMap<i64, (f64, f64)>);

fn get_widths2(seq: &[Obj]) -> Widths2 {
    let (mut widths, mut disps) = (HashMap::new(), HashMap::new());
    let mut r: Vec<f64> = Vec::new();
    let num = |o: &Obj| o.as_num().unwrap_or(0.0);
    for v in seq {
        match v {
            Obj::Array(list) => {
                if let Some(&char1) = r.last() {
                    for (i, t) in list.as_chunks::<3>().0.iter().enumerate() {
                        if let Some(k) = int_key(char1 + i as f64) {
                            widths.insert(k, t[0].as_num());
                            disps.insert(k, (num(&t[1]), num(&t[2])));
                        }
                    }
                    r.clear();
                }
            }
            _ => {
                if let Some(n) = v.as_num() {
                    r.push(n);
                    if r.len() == 5 {
                        let (c1, c2) = (r[0] as i64, r[1] as i64);
                        for c in c1..=c2.min(c1.saturating_add(1 << 20)) {
                            widths.insert(c, Some(r[2]));
                            disps.insert(c, (r[3], r[4]));
                        }
                        r.clear();
                    }
                }
            }
        }
    }
    (widths, disps)
}

impl Fonts {
    /// `PDFResourceManager.get_font(objid, spec)`
    pub fn get(&mut self, doc: &Document, objid: Option<i64>, spec: &Dict) -> Result<Rc<Font>> {
        let objid = objid.filter(|&id| id != 0);
        if let Some(id) = objid
            && let Some(f) = self.cache.get(&id)
        {
            return Ok(f.clone());
        }
        let subtype = spec
            .get("Subtype")
            .map_or_else(|| "Type1".to_string(), literal_name);
        let font = match subtype.as_str() {
            "Type3" => Rc::new(type3(doc, spec)?),
            "CIDFontType0" | "CIDFontType2" => Rc::new(cid_font(doc, spec)?),
            "Type0" => {
                let Some(d) = spec.get("DescendantFonts") else {
                    return key_err("DescendantFonts");
                };
                let dfonts = doc.list_value(d)?;
                let Some(first) = dfonts.first() else {
                    return other("AssertionError: no descendant fonts");
                };
                let mut subspec = (*doc.dict_value(first)?).clone();
                for k in ["Encoding", "ToUnicode"] {
                    if let Some(v) = spec.get(k) {
                        subspec.set(Rc::from(k.as_bytes()), doc.resolve(v)?);
                    }
                }
                self.get(doc, None, &subspec)?
            }
            _ => Rc::new(type1(doc, spec)?),
        };
        if let Some(id) = objid {
            self.cache.insert(id, font.clone());
        }
        Ok(font)
    }
}

/// PDFSimpleFont's encoding and ToUnicode.
fn simple_kind(doc: &Document, spec: &Dict) -> Result<Kind> {
    let default = if spec
        .get("Subtype")
        .map_or_else(|| "None".to_string(), literal_name)
        == "TrueType"
    {
        "WinAnsiEncoding"
    } else {
        "StandardEncoding"
    };
    let cid2unicode = match spec.get("Encoding") {
        None => get_encoding(default, None),
        Some(e) => match doc.resolve(e)? {
            Obj::Dict(d) => {
                let name = d
                    .get("BaseEncoding")
                    .map_or_else(|| default.to_string(), literal_name);
                let diff = match d.get("Differences") {
                    Some(v) => doc.list_value(v)?.to_vec(),
                    None => Vec::new(),
                };
                get_encoding(&name, Some(&diff))
            }
            e => get_encoding(&literal_name(&e), None),
        },
    };
    let unicode_map = match spec.get("ToUnicode") {
        Some(t) => {
            let strm = doc.stream_value(t)?;
            Some(parse_tounicode(doc.stream_data(&strm)?)?)
        }
        None => None,
    };
    Ok(Kind::Simple {
        cid2unicode,
        unicode_map,
    })
}

fn finish(
    desc: Desc,
    widths: HashMap<i64, Option<f64>>,
    std_widths: Option<&'static [(&'static str, i64)]>,
    default_width: Option<f64>,
    kind: Kind,
) -> Font {
    let descent = if desc.descent > 0.0 {
        -desc.descent
    } else {
        desc.descent
    };
    Font {
        widths,
        std_widths,
        default_width,
        descent,
        hscale: 0.001,
        vscale: 0.001,
        kind,
    }
}

/// PDFType1Font (and PDFTrueTypeFont, which only differs in its repr).
fn type1(doc: &Document, spec: &Dict) -> Result<Font> {
    let basefont = spec
        .get("BaseFont")
        .map_or_else(|| "unknown".to_string(), literal_name);
    let m = metrics(&basefont);
    let (desc_dict, desc, widths) = match m {
        Some(m) => (None, Desc::from_metrics(m), HashMap::new()),
        None => {
            let d = match spec.get("FontDescriptor") {
                Some(d) => doc.dict_value(d)?,
                None => Rc::new(Dict::default()),
            };
            let firstchar = spec.get("FirstChar").map_or(Ok(0), |v| doc.int_value(v))?;
            let list = match spec.get("Widths") {
                Some(w) => doc.list_value(w)?.to_vec(),
                None => vec![Obj::Int(0); 256],
            };
            let mut widths = HashMap::new();
            for (i, w) in list.iter().enumerate() {
                widths.insert(i as i64 + firstchar, width_value(doc, w)?);
            }
            let desc = Desc::from_dict(doc, &d)?;
            (Some(d), desc, widths)
        }
    };
    let mut kind = simple_kind(doc, spec)?;
    if !spec.contains("Encoding")
        && let Some(d) = &desc_dict
        && let Some(ff) = d.get("FontFile")
    {
        let fontfile = doc.stream_value(ff)?;
        let Some(l1) = fontfile.attrs.get("Length1") else {
            return key_err("Length1");
        };
        let length1 = doc.int_value(l1)?;
        let data = doc.stream_data(&fontfile)?;
        let end = if length1 < 0 {
            data.len().saturating_sub(length1.unsigned_abs() as usize)
        } else {
            (length1 as usize).min(data.len())
        };
        if let Kind::Simple { cid2unicode, .. } = &mut kind {
            *cid2unicode = Rc::new(type1_encoding(Rc::from(&data[..end]))?);
        }
    }
    let default = Some(desc.missing_width);
    Ok(finish(desc, widths, m.map(|m| m.widths), default, kind))
}

fn type3(doc: &Document, spec: &Dict) -> Result<Font> {
    let firstchar = spec.get("FirstChar").map_or(Ok(0), |v| doc.int_value(v))?;
    let list = match spec.get("Widths") {
        Some(w) => doc.list_value(w)?.to_vec(),
        None => vec![Obj::Int(0); 256],
    };
    let mut widths = HashMap::new();
    for (i, w) in list.iter().enumerate() {
        widths.insert(i as i64 + firstchar, width_value(doc, w)?);
    }
    let desc = match spec.get("FontDescriptor") {
        Some(d) => Desc::from_dict(doc, &*doc.dict_value(d)?)?,
        None => {
            let Some(bbox) = spec.get("FontBBox") else {
                return key_err("FontBBox");
            };
            Desc {
                descent: 0.0,
                missing_width: 0.0,
                bbox: parse_bbox(doc, Some(bbox))?,
            }
        }
    };
    let kind = simple_kind(doc, spec)?;
    let matrix = doc.list_value(spec.get("FontMatrix").unwrap_or(&Obj::Null))?;
    if matrix.len() != 6 {
        return other(format!(
            "not enough values to unpack (expected 6, got {})",
            matrix.len()
        ));
    }
    let m: Vec<f64> = matrix
        .iter()
        .map(|v| {
            v.as_num()
                .ok_or_else(|| PdfError::Other("unsupported operand type(s) for *".into()))
        })
        .collect::<Result<_>>()?;
    let bbox = desc.bbox;
    let default = Some(desc.missing_width);
    let mut font = finish(desc, widths, None, default, kind);
    // the descent comes from the bbox, as given
    font.descent = bbox[1];
    font.hscale = m[0] + m[2];
    font.vscale = m[1] + m[3];
    Ok(font)
}

fn cid_font(doc: &Document, spec: &Dict) -> Result<Font> {
    let info = match spec.get("CIDSystemInfo") {
        Some(i) => doc.dict_value(i)?,
        None => Rc::new(Dict::default()),
    };
    let latin1 = |k: &str| -> Result<String> {
        match info.get(k) {
            None => Ok("unknown".into()),
            Some(v) => match doc.resolve(v)? {
                Obj::Str(s) => Ok(s.iter().map(|&b| b as char).collect()),
                _ => other("object has no attribute 'decode'"),
            },
        }
    };
    let (registry, ordering) = (latin1("Registry")?, latin1("Ordering")?);
    let cidcoding = format!("{}-{}", pystr::strip(&registry), pystr::strip(&ordering));
    let cmap_name = match spec.get("Encoding") {
        None => "unknown".to_string(),
        Some(e @ (Obj::Name(_) | Obj::Kw(_))) => literal_name(e),
        Some(Obj::Dict(d)) => d
            .get("CMapName")
            .map_or_else(|| "unknown".to_string(), literal_name),
        Some(Obj::Stream(s)) => s
            .attrs
            .get("CMapName")
            .map_or_else(|| "unknown".to_string(), literal_name),
        Some(_) => return other("object is not subscriptable"),
    };
    let cmap_name = match cmap_name.as_str() {
        "DLIdent-H" => "Identity-H".to_string(),
        "DLIdent-V" => "Identity-V".to_string(),
        _ => cmap_name,
    };
    let cmap = CMap::get(&cmap_name).unwrap_or(CMap::Empty);
    let descriptor = match spec.get("FontDescriptor") {
        Some(d) => doc.dict_value(d)?,
        None => Rc::new(Dict::default()),
    };
    let ttf = match descriptor.get("FontFile2") {
        Some(ff) => Some(doc.stream_data(&*doc.stream_value(ff)?)?),
        None => None,
    };
    let unicode_map = match spec.get("ToUnicode") {
        Some(Obj::Stream(s)) => Some(parse_tounicode(doc.stream_data(s)?)?),
        Some(t) => {
            let name = literal_name(t);
            let Some(enc) = spec.get("Encoding") else {
                return key_err("Encoding");
            };
            let enc = literal_name(enc);
            (ordering.contains("Identity") || name.contains("Identity") || enc.contains("Identity"))
                .then_some(UnicodeMap::Identity)
        }
        None if cidcoding == "Adobe-Identity" || cidcoding == "Adobe-UCS" => match &ttf {
            Some(data) => super::truetype::unicode_map(data)?,
            None => None,
        },
        None => UnicodeMap::predefined(&cidcoding, cmap.is_vertical()),
    };
    let desc = Desc::from_dict(doc, &descriptor)?;
    let (widths, disps, default_width, default_vy) = if cmap.is_vertical() {
        let (widths, disps) = get_widths2(&doc.list_value(spec.get("W2").unwrap_or(&Obj::Null))?);
        let dw2 = match spec.get("DW2") {
            Some(v) => doc.resolve(v)?,
            None => Obj::Array(Rc::new(vec![Obj::Int(880), Obj::Int(-1000)])),
        };
        let Obj::Array(dw2) = dw2 else {
            return other("cannot unpack DW2");
        };
        if dw2.len() != 2 {
            return other("cannot unpack DW2");
        }
        (widths, disps, dw2[1].as_num(), dw2[0].as_num())
    } else {
        let widths = get_widths(doc, &doc.list_value(spec.get("W").unwrap_or(&Obj::Null))?)?;
        let dw = match spec.get("DW") {
            Some(v) => doc.resolve(v)?.as_num(),
            None => Some(1000.0),
        };
        (widths, HashMap::new(), dw, None)
    };
    let kind = Kind::Cid {
        cmap,
        unicode_map,
        disps,
        default_vy,
    };
    Ok(finish(desc, widths, None, default_width, kind))
}

/// Type1FontHeaderParser: `dup <code> /<name> put` pairs.
struct Type1Kw(Vec<(i64, String)>);

impl Keywords for Type1Kw {
    fn do_keyword(&mut self, st: &mut Stack, _pos: usize, kw: Rc<[u8]>) -> Result<()> {
        if &*kw == b"put" {
            let v = st.pop(2);
            if v.len() < 2 {
                return other("not enough values to unpack");
            }
            if let (Some(key), Obj::Name(_)) = (v[0].1.as_int(), &v[1].1) {
                self.0.push((key, literal_name(&v[1].1)));
            }
        }
        Ok(())
    }
}

fn type1_encoding(data: Rc<[u8]>) -> Result<Cid2Unicode> {
    let mut st = Stack::new(Lexer::new(data));
    let mut kw = Type1Kw(Vec::new());
    match st.nextobject(&mut kw) {
        Ok(_) | Err(PdfError::Eof) => {}
        Err(e) => return Err(e),
    }
    let mut map = HashMap::new();
    for (cid, name) in kw.0 {
        match name2unicode(&name) {
            Ok(u) => {
                map.insert(cid, u);
            }
            Err(NameError::Key) => {}
            Err(NameError::Value) => return other("invalid glyph name"),
        }
    }
    Ok(map)
}
