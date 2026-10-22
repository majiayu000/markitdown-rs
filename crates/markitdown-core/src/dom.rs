//! The HTML tree markdownify walks, shaped like BeautifulSoup's: elements,
//! text strings, and comment-like nodes that are kept for sibling relations
//! but never rendered.

use std::borrow::Cow;

#[derive(Debug)]
pub enum Kind {
    Element {
        name: Cow<'static, str>,
        /// in document order; a repeated attribute keeps its first position
        attrs: Vec<(String, String)>,
    },
    /// NavigableString, including CDATA, declarations and processing
    /// instructions, which markdownify renders as text
    Text(String),
    /// comments and doctypes, which markdownify skips
    Comment(String),
}

#[derive(Debug)]
struct DNode {
    kind: Kind,
    parent: Option<usize>,
    children: Vec<usize>,
    /// index within the parent's children
    pos: usize,
}

#[derive(Debug)]
pub struct Dom {
    nodes: Vec<DNode>,
}

pub const ROOT: usize = 0;

impl Dom {
    /// `root_name` is "[document]" for a whole parsed document.
    pub fn new(root_name: &'static str) -> Self {
        let root = DNode {
            kind: Kind::Element {
                name: Cow::Borrowed(root_name),
                attrs: Vec::new(),
            },
            parent: None,
            children: Vec::new(),
            pos: 0,
        };
        Dom { nodes: vec![root] }
    }

    pub fn push(&mut self, parent: usize, kind: Kind) -> usize {
        let id = self.nodes.len();
        let pos = self.nodes[parent].children.len();
        self.nodes.push(DNode {
            kind,
            parent: Some(parent),
            children: Vec::new(),
            pos,
        });
        self.nodes[parent].children.push(id);
        id
    }

    pub fn element(
        &mut self,
        parent: usize,
        name: impl Into<Cow<'static, str>>,
        attrs: Vec<(String, String)>,
    ) -> usize {
        self.push(
            parent,
            Kind::Element {
                name: name.into(),
                attrs,
            },
        )
    }

    /// Append text, merging with a preceding text node, as the HTML parser
    /// does for text that mammoth writes in pieces.
    pub fn text(&mut self, parent: usize, s: String) {
        if s.is_empty() {
            return;
        }
        if let Some(&last) = self.nodes[parent].children.last()
            && let Kind::Text(t) = &mut self.nodes[last].kind
        {
            t.push_str(&s);
            return;
        }
        self.push(parent, Kind::Text(s));
    }

    /// `PageElement.extract()`: detach a node; its former neighbours become
    /// siblings without merging.
    pub fn remove(&mut self, id: usize) {
        let Some(p) = self.nodes[id].parent.take() else {
            return;
        };
        let pos = self.nodes[id].pos;
        self.nodes[p].children.remove(pos);
        for i in pos..self.nodes[p].children.len() {
            let c = self.nodes[p].children[i];
            self.nodes[c].pos = i;
        }
    }

    /// `PageElement.insert_before(string)`: a new text sibling before `id`.
    pub fn insert_text_before(&mut self, id: usize, text: String) {
        let Some(p) = self.nodes[id].parent else {
            return;
        };
        let pos = self.nodes[id].pos;
        let new = self.nodes.len();
        self.nodes.push(DNode {
            kind: Kind::Text(text),
            parent: Some(p),
            children: Vec::new(),
            pos,
        });
        self.nodes[p].children.insert(pos, new);
        for i in pos + 1..self.nodes[p].children.len() {
            let c = self.nodes[p].children[i];
            self.nodes[c].pos = i;
        }
    }

    /// `Tag.append(string)`: a new text node at the end, never merged.
    pub fn append_text(&mut self, id: usize, text: String) {
        self.push(id, Kind::Text(text));
    }

    /// The `.string` setter: replace all children with one text node.
    pub fn set_string(&mut self, id: usize, text: String) {
        for c in std::mem::take(&mut self.nodes[id].children) {
            self.nodes[c].parent = None;
        }
        self.push(id, Kind::Text(text));
    }

    /// bs4 class matching: any whitespace-separated class, or the whole value.
    pub fn has_class(&self, id: usize, class: &str) -> bool {
        self.attr(id, "class")
            .is_some_and(|v| v == class || v.split_ascii_whitespace().any(|c| c == class))
    }

    pub fn kind(&self, id: usize) -> &Kind {
        &self.nodes[id].kind
    }

    pub fn children(&self, id: usize) -> &[usize] {
        &self.nodes[id].children
    }

    pub fn name(&self, id: usize) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Element { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn attr(&self, id: usize, key: &str) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Element { attrs, .. } => attrs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    pub fn set_attr(&mut self, id: usize, key: &str, value: String) {
        if let Kind::Element { attrs, .. } = &mut self.nodes[id].kind {
            match attrs.iter_mut().find(|(k, _)| k == key) {
                Some(slot) => slot.1 = value,
                None => attrs.push((key.to_string(), value)),
            }
        }
    }

    pub fn parent(&self, id: usize) -> Option<usize> {
        self.nodes[id].parent
    }

    fn sibling(&self, id: usize, offset: isize) -> Option<usize> {
        let n = &self.nodes[id];
        let p = n.parent?;
        let idx = n.pos.checked_add_signed(offset)?;
        self.nodes[p].children.get(idx).copied()
    }

    pub fn prev(&self, id: usize) -> Option<usize> {
        self.sibling(id, -1)
    }

    pub fn next(&self, id: usize) -> Option<usize> {
        self.sibling(id, 1)
    }

    pub fn is_element_named(&self, id: usize, names: &[&str]) -> bool {
        self.name(id).is_some_and(|n| names.contains(&n))
    }

    /// Descendant elements in document order matching `pred`.
    pub fn find_all_by(&self, id: usize, pred: &dyn Fn(&Dom, usize) -> bool, out: &mut Vec<usize>) {
        let mut stack: Vec<usize> = self.nodes[id].children.iter().rev().copied().collect();
        while let Some(c) = stack.pop() {
            if matches!(self.nodes[c].kind, Kind::Element { .. }) {
                if pred(self, c) {
                    out.push(c);
                }
                stack.extend(self.nodes[c].children.iter().rev());
            }
        }
    }

    pub fn find_all(&self, id: usize, names: &[&str]) -> Vec<usize> {
        let mut out = Vec::new();
        self.find_all_by(id, &|d, c| d.is_element_named(c, names), &mut out);
        out
    }

    pub fn find(&self, id: usize, name: &str) -> Option<usize> {
        self.find_all(id, &[name]).into_iter().next()
    }

    pub fn prev_element_sibling(&self, id: usize) -> Option<usize> {
        let n = &self.nodes[id];
        let p = n.parent?;
        self.nodes[p].children[..n.pos]
            .iter()
            .rev()
            .copied()
            .find(|&c| self.name(c).is_some())
    }

    pub fn has_ancestor(&self, id: usize, name: &str) -> bool {
        let mut cur = self.parent(id);
        while let Some(p) = cur {
            if self.name(p) == Some(name) {
                return true;
            }
            cur = self.parent(p);
        }
        false
    }

    /// `Tag.get_text()`: all descendant strings (comments excluded).
    pub fn get_text(&self, id: usize, sep: &str, strip: bool) -> String {
        let mut parts = Vec::new();
        let mut stack: Vec<usize> = self.nodes[id].children.iter().rev().copied().collect();
        while let Some(c) = stack.pop() {
            match &self.nodes[c].kind {
                Kind::Text(t) => {
                    let t = if strip {
                        crate::pystr::strip(t)
                    } else {
                        t.as_str()
                    };
                    if !strip || !t.is_empty() {
                        parts.push(t);
                    }
                }
                Kind::Comment(_) => {}
                Kind::Element { name, .. } => {
                    // strings inside these are Script/Stylesheet/... types,
                    // which get_text() leaves out
                    if !matches!(name.as_ref(), "script" | "style" | "template" | "rt" | "rp") {
                        stack.extend(self.nodes[c].children.iter().rev());
                    }
                }
            }
        }
        parts.join(sep)
    }

    /// `Tag.string`: the single string child, following single-child elements.
    pub fn string(&self, id: usize) -> Option<&str> {
        let children = &self.nodes[id].children;
        if children.len() != 1 {
            return None;
        }
        match &self.nodes[children[0]].kind {
            Kind::Text(t) | Kind::Comment(t) => Some(t),
            Kind::Element { .. } => self.string(children[0]),
        }
    }
}
