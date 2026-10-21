//! Package-level parts mammoth reads besides the body: relationships,
//! content types, styles and numbering.

use super::reader::NumLevel;
use crate::xml::Element;
use std::collections::HashMap;

/// Insert into an insertion-ordered map with Python dict semantics: a repeated
/// key keeps its first position and takes the new value.
fn dict_insert<V>(map: &mut Vec<(Option<String>, V)>, key: Option<String>, value: V) {
    match map.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => map.push((key, value)),
    }
}

#[derive(Default)]
pub struct Relationships {
    by_id: HashMap<String, String>,
    by_type: Vec<(String, String)>,
}

impl Relationships {
    pub fn parse(root: &Element) -> Self {
        let mut rels = Relationships::default();
        for r in root.children_named("relationships:Relationship") {
            let (Some(id), Some(target), Some(typ)) =
                (r.attr("Id"), r.attr("Target"), r.attr("Type"))
            else {
                continue;
            };
            rels.by_id.insert(id.to_string(), target.to_string());
            rels.by_type.push((typ.to_string(), target.to_string()));
        }
        rels
    }

    pub fn target(&self, id: &str) -> Option<&str> {
        self.by_id.get(id).map(String::as_str)
    }

    pub fn targets_of_type<'a>(&'a self, typ: &'a str) -> impl Iterator<Item = &'a str> {
        self.by_type
            .iter()
            .filter(move |(t, _)| t == typ)
            .map(|(_, target)| target.as_str())
    }
}

#[derive(Default)]
pub struct ContentTypes {
    defaults: HashMap<String, String>,
    overrides: HashMap<String, String>,
}

impl ContentTypes {
    pub fn parse(root: &Element) -> Self {
        let mut ct = ContentTypes::default();
        for d in root.children_named("content-types:Default") {
            if let (Some(ext), Some(t)) = (d.attr("Extension"), d.attr("ContentType")) {
                ct.defaults.insert(ext.to_string(), t.to_string());
            }
        }
        for o in root.children_named("content-types:Override") {
            if let (Some(part), Some(t)) = (o.attr("PartName"), o.attr("ContentType")) {
                ct.overrides
                    .insert(part.trim_start_matches('/').to_string(), t.to_string());
            }
        }
        ct
    }

    pub fn find(&self, path: &str) -> Option<String> {
        if let Some(t) = self.overrides.get(path) {
            return Some(t.clone());
        }
        let ext = path.rsplit('.').next().unwrap_or(path);
        if let Some(t) = self.defaults.get(ext) {
            return Some(t.clone());
        }
        let image = match ext.to_lowercase().as_str() {
            "png" => "png",
            "gif" => "gif",
            "jpeg" | "jpg" => "jpeg",
            "tif" | "tiff" => "tiff",
            "bmp" => "bmp",
            _ => return None,
        };
        Some(format!("image/{image}"))
    }
}

#[derive(Default)]
pub struct Styles {
    paragraph: HashMap<String, Option<String>>,
    character: HashMap<String, Option<String>>,
    /// numbering style id -> numId
    numbering: HashMap<String, Option<String>>,
}

impl Styles {
    /// markitdown first drops styles without `w:styleId` and defaults a
    /// missing `w:type` to "paragraph"; the first definition of an id wins.
    pub fn parse(root: &Element) -> Self {
        let mut s = Styles::default();
        for style in root.children_named("w:style") {
            let Some(id) = style.attr("w:styleId") else {
                continue;
            };
            let typ = style.attr("w:type").unwrap_or("paragraph");
            let name = || {
                style
                    .child("w:name")
                    .and_then(|n| n.attr("w:val"))
                    .map(str::to_string)
            };
            let (map, value) = match typ {
                "paragraph" => (&mut s.paragraph, name()),
                "character" => (&mut s.character, name()),
                "numbering" => (
                    &mut s.numbering,
                    style
                        .path(&["w:pPr", "w:numPr", "w:numId"])
                        .and_then(|n| n.attr("w:val"))
                        .map(str::to_string),
                ),
                _ => continue,
            };
            map.entry(id.to_string()).or_insert(value);
        }
        s
    }

    pub fn paragraph_name(&self, id: &str) -> Option<String> {
        self.paragraph.get(id).cloned().flatten()
    }

    pub fn character_name(&self, id: &str) -> Option<String> {
        self.character.get(id).cloned().flatten()
    }
}

/// A numbering level and the paragraph style linked to it, if any.
type StyledLevel = (NumLevel, Option<String>);

struct AbstractNum {
    /// ilvl -> level
    levels: Vec<(Option<String>, StyledLevel)>,
    num_style_link: Option<String>,
}

#[derive(Default)]
pub struct Numbering {
    abstracts: Vec<(Option<String>, AbstractNum)>,
    nums: HashMap<Option<String>, String>,
    by_style: HashMap<String, NumLevel>,
    num_style_ids: HashMap<String, Option<String>>,
}

impl Numbering {
    pub fn parse(root: &Element, styles: &Styles) -> Self {
        let mut n = Numbering {
            num_style_ids: styles.numbering.clone(),
            ..Default::default()
        };
        for a in root.children_named("w:abstractNum") {
            let mut levels = Vec::new();
            let mut without_index = None;
            for lvl in a.children_named("w:lvl") {
                let fmt = lvl.child("w:numFmt").and_then(|f| f.attr("w:val"));
                let pstyle = lvl
                    .child("w:pStyle")
                    .and_then(|p| p.attr("w:val"))
                    .map(str::to_string);
                let index = lvl.attr("w:ilvl");
                let level = NumLevel {
                    level_index: index.unwrap_or("0").to_string(),
                    is_ordered: fmt != Some("bullet"),
                };
                match index {
                    Some(i) => dict_insert(&mut levels, Some(i.to_string()), (level, pstyle)),
                    None => without_index = Some((level, pstyle)),
                }
            }
            if let Some(l) = without_index
                && !levels.iter().any(|(k, _)| k.as_deref() == Some("0"))
            {
                levels.push((Some("0".into()), l));
            }
            let num_style_link = a
                .child("w:numStyleLink")
                .and_then(|l| l.attr("w:val"))
                .map(str::to_string);
            let key = a.attr("w:abstractNumId").map(str::to_string);
            dict_insert(
                &mut n.abstracts,
                key,
                AbstractNum {
                    levels,
                    num_style_link,
                },
            );
        }
        for (_, abs) in &n.abstracts {
            for (_, (level, pstyle)) in &abs.levels {
                if let Some(p) = pstyle {
                    n.by_style.insert(p.clone(), level.clone());
                }
            }
        }
        for num in root.children_named("w:num") {
            let Some(abs) = num.child("w:abstractNumId").and_then(|a| a.attr("w:val")) else {
                continue;
            };
            n.nums
                .insert(num.attr("w:numId").map(str::to_string), abs.to_string());
        }
        n
    }

    pub fn find_level(&self, num_id: &str, level: &str) -> Option<NumLevel> {
        self.find_level_depth(Some(num_id), level, 0)
    }

    fn find_level_depth(
        &self,
        num_id: Option<&str>,
        level: &str,
        depth: usize,
    ) -> Option<NumLevel> {
        let abs_id = self.nums.get(&num_id.map(str::to_string))?;
        let (_, abs) = self
            .abstracts
            .iter()
            .find(|(k, _)| k.as_deref() == Some(abs_id))?;
        match &abs.num_style_link {
            None => abs
                .levels
                .iter()
                .find(|(k, _)| k.as_deref() == Some(level))
                .map(|(_, (l, _))| l.clone()),
            Some(link) if depth < 8 => {
                let next = self.num_style_ids.get(link)?.as_deref();
                self.find_level_depth(next, level, depth + 1)
            }
            Some(_) => None,
        }
    }

    pub fn find_level_by_paragraph_style(&self, style_id: &str) -> Option<NumLevel> {
        self.by_style.get(style_id).cloned()
    }
}
