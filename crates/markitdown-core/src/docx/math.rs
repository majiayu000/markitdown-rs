//! OMML equations -> LaTeX text runs, as markitdown's docx pre-processing
//! does (`converter_utils/docx/math/omml.py`, adapted from dwml).
//!
//! markitdown converts every equation of a part in one pass and keeps the
//! part untouched if any equation fails, so a single unsupported construct
//! makes every equation of that part disappear. That is reproduced here.

use crate::xml::{Element, Node};

/// Error marker: the Python code raised, so the whole part keeps its OMML.
struct Fail;
type R<T> = Result<T, Fail>;

const BRK: &str = "\\\\";
const FUNC_PLACE: &str = "{fe}";

/// Replace `m:oMathPara` with a block-equation paragraph and the remaining
/// `m:oMath` with inline-equation runs. None if any equation fails.
pub fn replace_equations(root: &Element) -> Option<Element> {
    let mut el = root.clone();
    replace(&mut el, "oMathPara").ok()?;
    replace(&mut el, "oMath").ok()?;
    Some(el)
}

fn local_name(name: &str) -> &str {
    name.rsplit([':', '}']).next().unwrap_or(name)
}

fn replace(el: &mut Element, target: &str) -> R<()> {
    for child in &mut el.children {
        let Node::Element(c) = child else { continue };
        if local_name(&c.name) != target {
            replace(c, target)?;
            continue;
        }
        *child = if target == "oMathPara" {
            let mut maths = Vec::new();
            find_all(c, "oMath", &mut maths);
            let runs = maths
                .into_iter()
                .map(|m| Ok(text_run(format!("$${}$$", to_latex(m)?))))
                .collect::<R<Vec<_>>>()?;
            Node::Element(Element::new("w:p", runs))
        } else {
            text_run(format!("${}$", to_latex(c)?))
        };
    }
    Ok(())
}

fn find_all<'a>(el: &'a Element, local: &str, out: &mut Vec<&'a Element>) {
    for c in el.elements() {
        if local_name(&c.name) == local {
            out.push(c);
        }
        find_all(c, local, out);
    }
}

fn text_run(text: String) -> Node {
    let t = Element::new("w:t", vec![Node::Text(text)]);
    Node::Element(Element::new("w:r", vec![Node::Element(t)]))
}

/// `_convert_omath_to_latex`: the equation is re-parsed inside a fixed
/// namespace template, so markup from other namespaces makes it fail.
fn to_latex(math: &Element) -> R<String> {
    check_namespaces(math)?;
    if &*math.name != "m:oMath" {
        return Ok(String::new());
    }
    process_children(math, None)
}

fn check_namespaces(el: &Element) -> R<()> {
    let bad = |n: &str| n.starts_with('{') || n.starts_with("a:") || n.starts_with("pic:");
    if bad(&el.name) || el.attrs.iter().any(|(k, _)| bad(k)) {
        return Err(Fail);
    }
    el.elements().try_for_each(check_namespaces)
}

#[derive(Default)]
struct Pr {
    text: String,
    chr: Option<String>,
    pos: Option<String>,
    beg_chr: Option<String>,
    end_chr: Option<String>,
    typ: Option<String>,
}

enum Val {
    S(String),
    Pr(Pr),
}

impl Val {
    fn text(&self) -> &str {
        match self {
            Val::S(s) => s,
            Val::Pr(p) => &p.text,
        }
    }
    fn into_string(self) -> R<String> {
        match self {
            Val::S(s) => Ok(s),
            // "".join([...]) with a Pr object in the list raises TypeError
            Val::Pr(_) => Err(Fail),
        }
    }
    fn pr(&self) -> R<&Pr> {
        match self {
            Val::Pr(p) => Ok(p),
            // attribute access on a str (`.chr` etc.) raises
            Val::S(_) => Err(Fail),
        }
    }
}

fn m_children(el: &Element) -> impl Iterator<Item = (&str, &Element)> {
    el.elements()
        .filter_map(|c| c.name.strip_prefix("m:").map(|stag| (stag, c)))
}

fn read_pr(el: &Element) -> Pr {
    let mut pr = Pr::default();
    for (stag, c) in m_children(el) {
        let val = || c.attr("m:val").map(str::to_string);
        match stag {
            "brk" => pr.text.push_str(BRK),
            "chr" => pr.chr = val(),
            "pos" => pr.pos = val(),
            "begChr" => pr.beg_chr = val(),
            "endChr" => pr.end_chr = val(),
            "type" => pr.typ = val(),
            _ => {}
        }
    }
    pr
}

fn children_list<'a>(el: &'a Element, include: Option<&[&str]>) -> R<Vec<(&'a str, Val)>> {
    let mut out = Vec::new();
    for (stag, c) in m_children(el) {
        if include.is_some_and(|inc| !inc.contains(&stag)) {
            continue;
        }
        let v = match call(stag, c)? {
            Some(v) => Some(v),
            None => unknown(stag, c)?,
        };
        if let Some(v) = v {
            out.push((stag, v));
        }
    }
    Ok(out)
}

fn process_children(el: &Element, include: Option<&[&str]>) -> R<String> {
    Ok(children_list(el, include)?
        .iter()
        .map(|(_, v)| v.text())
        .collect())
}

/// `process_children_dict`: later children with the same tag win.
fn children_dict<'a>(el: &'a Element, include: Option<&[&str]>) -> R<Vec<(&'a str, Val)>> {
    let mut out: Vec<(&str, Val)> = Vec::new();
    for (k, v) in children_list(el, include)? {
        match out.iter_mut().find(|(ok, _)| *ok == k) {
            Some(slot) => slot.1 = v,
            None => out.push((k, v)),
        }
    }
    Ok(out)
}

fn get<'v>(d: &'v [(&str, Val)], key: &str) -> Option<&'v Val> {
    d.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
}

/// `c_dict["key"]` (KeyError when absent)
fn req<'v>(d: &'v [(&str, Val)], key: &str) -> R<&'v Val> {
    get(d, key).ok_or(Fail)
}

/// `str.format` of an optional value: None prints as "None".
fn or_none(v: Option<&Val>) -> &str {
    v.map_or("None", Val::text)
}

fn unknown(stag: &str, el: &Element) -> R<Option<Val>> {
    if matches!(
        stag,
        "box" | "sSub" | "sSup" | "sSubSup" | "num" | "den" | "deg" | "e"
    ) {
        Ok(Some(Val::S(process_children(el, None)?)))
    } else if stag.ends_with("Pr") {
        Ok(Some(Val::Pr(read_pr(el))))
    } else {
        Ok(None)
    }
}

fn call(stag: &str, el: &Element) -> R<Option<Val>> {
    let s = |s: String| Ok(Some(Val::S(s)));
    match stag {
        "acc" => {
            let d = children_dict(el, None)?;
            let pr = req(&d, "accPr")?.pr()?;
            let cmd = pr.chr.as_deref().and_then(chr_accent).unwrap_or("\\hat");
            s(format!("{cmd}{{{}}}", req(&d, "e")?.text()))
        }
        "r" => s(do_r(el)),
        "bar" => {
            let d = children_dict(el, None)?;
            let pr = req(&d, "barPr")?.pr()?;
            let cmd = match pr.pos.as_deref() {
                Some("bot") => "\\underline",
                _ => "\\overline",
            };
            s(format!("{}{cmd}{{{}}}", pr.text, req(&d, "e")?.text()))
        }
        "sub" => s(format!("_{{{}}}", process_children(el, None)?)),
        "sup" => s(format!("^{{{}}}", process_children(el, None)?)),
        "f" => {
            let d = children_dict(el, None)?;
            let pr = req(&d, "fPr")?.pr()?;
            let (num, den) = (or_none(get(&d, "num")), or_none(get(&d, "den")));
            let body = match pr.typ.as_deref() {
                Some("skw") => format!("^{{{num}}}/_{{{den}}}"),
                Some("noBar") => format!("\\genfrac{{}}{{}}{{0pt}}{{}}{{{num}}}{{{den}}}"),
                Some("lin") => format!("{{{num}}}/{{{den}}}"),
                _ => format!("\\frac{{{num}}}{{{den}}}"),
            };
            s(format!("{}{body}", pr.text))
        }
        "func" => {
            let d = children_dict(el, None)?;
            let name = get(&d, "fName").ok_or(Fail)?.text().to_string();
            let e = get(&d, "e").ok_or(Fail)?.text();
            s(name.replace(FUNC_PLACE, e))
        }
        "fName" => {
            let mut chars: Vec<String> = Vec::new();
            let mut parts: Vec<String> = Vec::new();
            let flush = |parts: &mut Vec<String>, chars: &mut Vec<String>| {
                if parts.is_empty() {
                    return;
                }
                let name = parts.concat();
                parts.clear();
                chars.push(match func(&name) {
                    Some(f) => f.to_string(),
                    None => format!("\\operatorname{{{name}}}({FUNC_PLACE})"),
                });
            };
            for (stag, v) in children_list(el, None)? {
                if stag == "r" {
                    parts.push(v.into_string()?);
                } else {
                    flush(&mut parts, &mut chars);
                    chars.push(v.into_string()?);
                }
            }
            flush(&mut parts, &mut chars);
            let t = chars.concat();
            s(if t.contains(FUNC_PLACE) {
                t
            } else {
                t + FUNC_PLACE
            })
        }
        "groupChr" => {
            let d = children_dict(el, None)?;
            let pr = req(&d, "groupChrPr")?.pr()?;
            let cmd = pr
                .chr
                .as_deref()
                .and_then(chr_accent)
                .unwrap_or("\\underbrace");
            s(format!("{}{cmd}{{{}}}", pr.text, req(&d, "e")?.text()))
        }
        "d" => {
            let d = children_dict(el, None)?;
            let pr = req(&d, "dPr")?.pr()?;
            let delim = |c: &Option<String>, default: &str| {
                let v = match c {
                    None => default.to_string(),
                    Some(k) => k
                        .chars()
                        .next()
                        .filter(|_| k.chars().count() == 1)
                        .and_then(t_char)
                        .map_or_else(|| k.clone(), str::to_string),
                };
                if v.is_empty() {
                    ".".to_string()
                } else {
                    escape_latex(&v)
                }
            };
            let left = delim(&pr.beg_chr, "(");
            let right = delim(&pr.end_chr, ")");
            s(format!(
                "{}\\left{left}{}\\right{right}",
                pr.text,
                req(&d, "e")?.text()
            ))
        }
        "rad" => {
            let d = children_dict(el, None)?;
            let text = or_none(get(&d, "e"));
            match get(&d, "deg").map(Val::text).filter(|t| !t.is_empty()) {
                Some(deg) => s(format!("\\sqrt[{deg}]{{{text}}}")),
                None => s(format!("\\sqrt{{{text}}}")),
            }
        }
        "eqArr" => {
            let rows = children_list(el, Some(&["e"]))?
                .into_iter()
                .map(|(_, v)| v.into_string())
                .collect::<R<Vec<_>>>()?;
            s(format!(
                "\\begin{{array}}{{c}}{}\\end{{array}}",
                rows.join(BRK)
            ))
        }
        "limLow" => {
            let d = children_dict(el, Some(&["e", "lim"]))?;
            let lim = or_none(get(&d, "lim"));
            match req(&d, "e")?.text() {
                f @ ("lim" | "max" | "min") => s(format!("\\{f}_{{{lim}}}")),
                _ => Err(Fail), // NotImplementedError("Not support lim ...")
            }
        }
        "limUpp" => {
            let d = children_dict(el, Some(&["e", "lim"]))?;
            s(format!(
                "\\overset{{{}}}{{{}}}",
                or_none(get(&d, "lim")),
                or_none(get(&d, "e"))
            ))
        }
        "lim" => s(process_children(el, None)?.replace("\\rightarrow", "\\to")),
        "m" => {
            let mut rows = Vec::new();
            for (stag, v) in children_list(el, None)? {
                if stag == "mr" {
                    rows.push(v.into_string()?);
                }
            }
            s(format!(
                "\\begin{{matrix}}{}\\end{{matrix}}",
                rows.join(BRK)
            ))
        }
        "mr" => {
            let cells = children_list(el, Some(&["e"]))?
                .into_iter()
                .map(|(_, v)| v.into_string())
                .collect::<R<Vec<_>>>()?;
            s(cells.join("&"))
        }
        "nary" => {
            let mut bo = Some(String::new());
            let mut res = Vec::new();
            for (stag, v) in children_list(el, None)? {
                if stag == "naryPr" {
                    // get_char(None) is None, and None + str raises below
                    bo = v
                        .pr()?
                        .chr
                        .as_ref()
                        .map(|c| big_operator(c).map_or_else(|| c.clone(), str::to_string));
                } else {
                    res.push(v.into_string()?);
                }
            }
            s(bo.ok_or(Fail)? + &res.concat())
        }
        _ => Ok(None),
    }
}

/// `do_r`: text of the first `m:t` child, mapped through the T table.
fn do_r(el: &Element) -> String {
    let text = el
        .child("m:t")
        .map(|t| {
            t.children
                .iter()
                .map_while(|n| {
                    if let Node::Text(s) = n {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .collect::<String>()
        })
        .unwrap_or_default();
    let mapped: String = text
        .chars()
        .map(|c| t_char(c).map_or_else(|| c.to_string(), str::to_string))
        .collect();
    escape_latex(&mapped)
}

fn escape_latex(s: &str) -> String {
    let s = s.replace("\\\\", "\\");
    let mut out = String::with_capacity(s.len());
    let mut last = None;
    for c in s.chars() {
        if "{}_^#&$%~".contains(c) && last != Some('\\') {
            out.push('\\');
        }
        out.push(c);
        last = Some(c);
    }
    out
}

fn func(name: &str) -> Option<&'static str> {
    Some(match name {
        "sin" => "\\sin({fe})",
        "cos" => "\\cos({fe})",
        "tan" => "\\tan({fe})",
        "arcsin" => "\\arcsin({fe})",
        "arccos" => "\\arccos({fe})",
        "arctan" => "\\arctan({fe})",
        "arccot" => "\\operatorname{arccot}({fe})",
        "sinh" => "\\sinh({fe})",
        "cosh" => "\\cosh({fe})",
        "tanh" => "\\tanh({fe})",
        "coth" => "\\coth({fe})",
        "sec" => "\\sec({fe})",
        "csc" => "\\csc({fe})",
        "log" => "\\log({fe})",
        "ln" => "\\ln({fe})",
        "exp" => "\\exp({fe})",
        "det" => "\\det({fe})",
        "gcd" => "\\gcd({fe})",
        "lcm" => "\\operatorname{lcm}({fe})",
        "lim" => "\\lim({fe})",
        "max" => "\\max({fe})",
        "min" => "\\min({fe})",
        "sup" => "\\sup({fe})",
        "inf" => "\\inf({fe})",
        "dim" => "\\dim({fe})",
        "ker" => "\\ker({fe})",
        "hom" => "\\hom({fe})",
        "deg" => "\\deg({fe})",
        "arg" => "\\arg({fe})",
        _ => return None,
    })
}

/// CHR: accent / group characters (keys are single characters).
fn chr_accent(c: &str) -> Option<&'static str> {
    let mut it = c.chars();
    let (Some(ch), None) = (it.next(), it.next()) else {
        return None;
    };
    Some(match ch {
        '\u{0300}' => "\\grave",
        '\u{0301}' => "\\acute",
        '\u{0302}' => "\\hat",
        '\u{0303}' => "\\tilde",
        '\u{0304}' => "\\bar",
        '\u{0305}' => "\\overbar",
        '\u{0306}' => "\\breve",
        '\u{0307}' => "\\dot",
        '\u{0308}' => "\\ddot",
        '\u{0309}' => "\\ovhook",
        '\u{030a}' => "\\ocirc",
        '\u{030c}' => "\\check",
        '\u{0310}' => "\\candra",
        '\u{0312}' => "\\oturnedcomma",
        '\u{0315}' => "\\ocommatopright",
        '\u{031a}' => "\\droang",
        '\u{0338}' => "\\not",
        '\u{20d0}' => "\\leftharpoonaccent",
        '\u{20d1}' => "\\rightharpoonaccent",
        '\u{20d2}' => "\\vertoverlay",
        '\u{20d6}' => "\\overleftarrow",
        '\u{20d7}' => "\\vec",
        '\u{20db}' => "\\dddot",
        '\u{20dc}' => "\\ddddot",
        '\u{20e1}' => "\\overleftrightarrow",
        '\u{20e7}' => "\\annuity",
        '\u{20e9}' => "\\widebridgeabove",
        '\u{20f0}' => "\\asteraccent",
        '\u{0330}' => "\\wideutilde",
        '\u{0331}' => "\\underbar",
        '\u{20e8}' => "\\threeunderdot",
        '\u{20ec}' => "\\underrightharpoondown",
        '\u{20ed}' => "\\underleftharpoondown",
        '\u{20ee}' => "\\underleftarrow",
        '\u{20ef}' => "\\underrightarrow",
        '\u{23b4}' => "\\overbracket",
        '\u{23dc}' => "\\overparen",
        '\u{23de}' => "\\overbrace",
        '\u{23b5}' => "\\underbracket",
        '\u{23dd}' => "\\underparen",
        '\u{23df}' => "\\underbrace",
        _ => return None,
    })
}

fn big_operator(c: &str) -> Option<&'static str> {
    Some(match c {
        "\u{2140}" => "\\Bbbsum",
        "\u{220f}" => "\\prod",
        "\u{2210}" => "\\coprod",
        "\u{2211}" => "\\sum",
        "\u{222b}" => "\\int",
        "\u{22c0}" => "\\bigwedge",
        "\u{22c1}" => "\\bigvee",
        "\u{22c2}" => "\\bigcap",
        "\u{22c3}" => "\\bigcup",
        "\u{2a00}" => "\\bigodot",
        "\u{2a01}" => "\\bigoplus",
        "\u{2a02}" => "\\bigotimes",
        _ => return None,
    })
}

/// T: per-character substitutions applied to math run text.
fn t_char(c: char) -> Option<&'static str> {
    const GREEK: [&str; 32] = [
        "\\alpha ",
        "\\beta ",
        "\\gamma ",
        "\\delta ",
        "\\epsilon ",
        "\\zeta ",
        "\\eta ",
        "\\theta ",
        "\\iota ",
        "\\kappa ",
        "\\lambda ",
        "\\mu ",
        "\\nu ",
        "\\xi ",
        "\\omicron ",
        "\\pi ",
        "\\rho ",
        "\\varsigma ",
        "\\sigma ",
        "\\tau ",
        "\\upsilon ",
        "\\phi ",
        "\\chi ",
        "\\psi ",
        "\\omega ",
        "\\partial ",
        "\\varepsilon ",
        "\\vartheta ",
        "\\varkappa ",
        "\\varphi ",
        "\\varrho ",
        "\\varpi ",
    ];
    const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
    let cp = c as u32;
    if (0x1d6fc..=0x1d71b).contains(&cp) {
        return Some(GREEK[(cp - 0x1d6fc) as usize]);
    }
    if (0x1d434..=0x1d44d).contains(&cp) {
        let i = (cp - 0x1d434) as usize;
        return Some(&UPPER[i..i + 1]);
    }
    // U+1D455 (italic small h) is reserved; U+210E stands in for it
    if (0x1d44e..=0x1d467).contains(&cp) && cp != 0x1d455 {
        let i = (cp - 0x1d44e) as usize;
        return Some(&LOWER[i..i + 1]);
    }
    Some(match c {
        '\u{210e}' => "h",
        '\u{2190}' => "\\leftarrow ",
        '\u{2191}' => "\\uparrow ",
        '\u{2192}' => "\\rightarrow ",
        '\u{2193}' => "\\downarrow ",
        '\u{2194}' => "\\leftrightarrow ",
        '\u{2195}' => "\\updownarrow ",
        '\u{2196}' => "\\nwarrow ",
        '\u{2197}' => "\\nearrow ",
        '\u{2198}' => "\\searrow ",
        '\u{2199}' => "\\swarrow ",
        '\u{22ee}' => "\\vdots ",
        '\u{22ef}' => "\\cdots ",
        '\u{22f0}' => "\\adots ",
        '\u{22f1}' => "\\ddots ",
        '\u{2260}' => "\\ne ",
        '\u{2264}' => "\\leq ",
        '\u{2265}' => "\\geq ",
        '\u{2266}' => "\\leqq ",
        '\u{2267}' => "\\geqq ",
        '\u{2268}' => "\\lneqq ",
        '\u{2269}' => "\\gneqq ",
        '\u{226a}' => "\\ll ",
        '\u{226b}' => "\\gg ",
        '\u{2208}' => "\\in ",
        '\u{2209}' => "\\notin ",
        '\u{220b}' => "\\ni ",
        '\u{220c}' => "\\nni ",
        '\u{221e}' => "\\infty ",
        '\u{00b1}' => "\\pm ",
        '\u{2213}' => "\\mp ",
        _ => return None,
    })
}
