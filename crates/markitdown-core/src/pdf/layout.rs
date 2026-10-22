//! pdfminer's layout analysis with the default LAParams, and the text
//! TextConverter writes for each analysed page.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use super::interp::{Char, Item, PageItems};
use crate::pystr;

const LINE_OVERLAP: f64 = 0.5;
const CHAR_MARGIN: f64 = 2.0;
const LINE_MARGIN: f64 = 0.5;
const WORD_MARGIN: f64 = 0.1;
const BOXES_FLOW: f64 = 0.5;
/// utils.INF
const INF: f64 = 2147483647.0;

type BBox = [f64; 4];

/// `min(a, b)` / `max(a, b)` as Python evaluates them.
fn min2(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

fn max2(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

fn expand(bbox: &mut BBox, o: &BBox) {
    *bbox = [
        min2(bbox[0], o[0]),
        min2(bbox[1], o[1]),
        max2(bbox[2], o[2]),
        max2(bbox[3], o[3]),
    ];
}

fn width(b: &BBox) -> f64 {
    b[2] - b[0]
}

fn height(b: &BBox) -> f64 {
    b[3] - b[1]
}

fn char_bbox(c: &Char) -> BBox {
    [c.x0, c.y0, c.x1, c.y1]
}

enum El {
    Char(usize),
    Anno(&'static str),
}

/// LTTextLineHorizontal
struct Line {
    els: Vec<El>,
    bbox: BBox,
    last_x1: f64,
}

impl Line {
    fn new() -> Self {
        Line {
            els: Vec::new(),
            bbox: [INF, INF, -INF, -INF],
            last_x1: INF,
        }
    }

    fn add(&mut self, idx: usize, c: &Char) {
        let margin = WORD_MARGIN * max2(c.width, c.height);
        if self.last_x1 < c.x0 - margin {
            self.els.push(El::Anno(" "));
        }
        self.last_x1 = c.x1;
        self.els.push(El::Char(idx));
        expand(&mut self.bbox, &char_bbox(c));
    }

    fn text(&self, chars: &[&Char]) -> String {
        self.els
            .iter()
            .map(|e| match e {
                El::Char(i) => chars[*i].text.as_str(),
                El::Anno(s) => s,
            })
            .collect()
    }

    fn is_empty(&self, chars: &[&Char]) -> bool {
        let t = self.text(chars);
        width(&self.bbox) <= 0.0
            || height(&self.bbox) <= 0.0
            || (!t.is_empty() && t.chars().all(pystr::is_space))
    }
}

fn is_voverlap(a: &BBox, b: &BBox) -> bool {
    b[1] <= a[3] && a[1] <= b[3]
}

fn voverlap(a: &BBox, b: &BBox) -> f64 {
    if is_voverlap(a, b) {
        ((a[1] - b[3]).abs()).min((a[3] - b[1]).abs())
    } else {
        0.0
    }
}

fn hdistance(a: &BBox, b: &BBox) -> f64 {
    if b[0] <= a[2] && a[0] <= b[2] {
        0.0
    } else {
        ((a[0] - b[2]).abs()).min((a[2] - b[0]).abs())
    }
}

/// LTLayoutContainer.group_objects with detect_vertical off.
fn group_objects(chars: &[&Char]) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut line: Option<Line> = None;
    for i in 1..chars.len() {
        let (o0, o1) = (char_bbox(chars[i - 1]), char_bbox(chars[i]));
        let (c0, c1) = (chars[i - 1], chars[i]);
        let halign = is_voverlap(&o0, &o1)
            && min2(c0.height, c1.height) * LINE_OVERLAP < voverlap(&o0, &o1)
            && hdistance(&o0, &o1) < max2(c0.width, c1.width) * CHAR_MARGIN;
        if halign && let Some(l) = line.as_mut() {
            l.add(i, c1);
        } else if let Some(l) = line.take() {
            lines.push(l);
        } else if halign {
            let mut l = Line::new();
            l.add(i - 1, c0);
            l.add(i, c1);
            line = Some(l);
        } else {
            let mut l = Line::new();
            l.add(i - 1, c0);
            lines.push(l);
        }
    }
    let line = line.unwrap_or_else(|| {
        let mut l = Line::new();
        l.add(chars.len() - 1, chars[chars.len() - 1]);
        l
    });
    lines.push(line);
    lines
}

/// utils.Plane: a 50-unit grid index over bounding boxes.
struct Plane {
    seq: Vec<usize>,
    alive: HashSet<usize>,
    grid: HashMap<(i64, i64), Vec<usize>>,
    bboxes: HashMap<usize, BBox>,
    bbox: BBox,
}

fn drange(v0: f64, v1: f64) -> std::ops::Range<i64> {
    (v0 as i64).div_euclid(50)..((v1 + 50.0) as i64).div_euclid(50)
}

impl Plane {
    fn new(bbox: BBox) -> Self {
        Plane {
            seq: Vec::new(),
            alive: HashSet::new(),
            grid: HashMap::new(),
            bboxes: HashMap::new(),
            bbox,
        }
    }

    fn range(&self, b: &BBox) -> Vec<(i64, i64)> {
        let p = &self.bbox;
        if b[2] <= p[0] || p[2] <= b[0] || b[3] <= p[1] || p[3] <= b[1] {
            return Vec::new();
        }
        let (x0, y0, x1, y1) = (
            max2(p[0], b[0]),
            max2(p[1], b[1]),
            min2(p[2], b[2]),
            min2(p[3], b[3]),
        );
        let mut out = Vec::new();
        for gy in drange(y0, y1) {
            for gx in drange(x0, x1) {
                out.push((gx, gy));
            }
        }
        out
    }

    fn add(&mut self, id: usize, b: BBox) {
        for k in self.range(&b) {
            self.grid.entry(k).or_default().push(id);
        }
        self.bboxes.insert(id, b);
        self.seq.push(id);
        self.alive.insert(id);
    }

    fn remove(&mut self, id: usize) {
        let b = self.bboxes[&id];
        for k in self.range(&b) {
            if let Some(cell) = self.grid.get_mut(&k)
                && let Some(p) = cell.iter().position(|&x| x == id)
            {
                cell.remove(p);
            }
        }
        self.alive.remove(&id);
    }

    fn find(&self, b: &BBox) -> Vec<usize> {
        let mut done = HashSet::new();
        let mut out = Vec::new();
        for k in self.range(b) {
            let Some(cell) = self.grid.get(&k) else {
                continue;
            };
            for &id in cell {
                if !done.insert(id) {
                    continue;
                }
                let o = &self.bboxes[&id];
                if o[2] <= b[0] || b[2] <= o[0] || o[3] <= b[1] || b[3] <= o[1] {
                    continue;
                }
                out.push(id);
            }
        }
        out
    }

    fn iter(&self) -> Vec<usize> {
        self.seq
            .iter()
            .copied()
            .filter(|id| self.alive.contains(id))
            .collect()
    }
}

/// LTTextBoxHorizontal: lines by index.
struct TBox {
    lines: Vec<usize>,
    bbox: BBox,
}

fn group_textlines(page_bbox: BBox, lines: &[Line]) -> Vec<TBox> {
    let mut plane = Plane::new(page_bbox);
    for (i, l) in lines.iter().enumerate() {
        plane.add(i, l.bbox);
    }
    let mut all: Vec<TBox> = Vec::new();
    let mut boxes: HashMap<usize, usize> = HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        let b = &line.bbox;
        let d = LINE_MARGIN * height(b);
        let found = plane.find(&[b[0], b[1] - d, b[2], b[3] + d]);
        let mut members = vec![i];
        for j in found {
            let o = &lines[j].bbox;
            let aligned = (height(o) - height(b)).abs() <= d
                && ((o[0] - b[0]).abs() <= d
                    || (o[2] - b[2]).abs() <= d
                    || ((o[0] + o[2]) / 2.0 - (b[0] + b[2]) / 2.0).abs() <= d);
            if !aligned {
                continue;
            }
            members.push(j);
            if let Some(bx) = boxes.remove(&j) {
                members.extend(all[bx].lines.iter().copied());
            }
        }
        let mut tbox = TBox {
            lines: Vec::new(),
            bbox: [INF, INF, -INF, -INF],
        };
        let id = all.len();
        let mut seen = HashSet::new();
        for m in members {
            if seen.insert(m) {
                tbox.lines.push(m);
                expand(&mut tbox.bbox, &lines[m].bbox);
                boxes.insert(m, id);
            }
        }
        all.push(tbox);
    }
    let mut done = HashSet::new();
    let mut order = Vec::new();
    for i in 0..lines.len() {
        let Some(&bx) = boxes.get(&i) else {
            continue;
        };
        if !done.insert(bx) {
            continue;
        }
        let b = &all[bx].bbox;
        if !(width(b) <= 0.0 || height(b) <= 0.0) {
            order.push(bx);
        }
    }
    let mut taken: Vec<Option<TBox>> = all.into_iter().map(Some).collect();
    order
        .into_iter()
        .map(|i| taken[i].take().unwrap())
        .collect()
}

/// A text box or an LTTextGroupLRTB of two elements.
enum Node {
    Box(usize),
    Group(Vec<usize>),
}

struct F(f64);

impl PartialEq for F {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == std::cmp::Ordering::Equal
    }
}

impl Eq for F {}

impl PartialOrd for F {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for F {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        // -0.0 and 0.0 are equal, as in Python's tuple comparison
        (self.0 + 0.0).total_cmp(&(o.0 + 0.0))
    }
}

/// LTLayoutContainer.group_textboxes; element ids stand in for `id()`.
fn group_textboxes(page_bbox: BBox, boxes: &[TBox]) -> (Vec<Node>, Vec<BBox>, Vec<usize>) {
    let mut nodes: Vec<Node> = (0..boxes.len()).map(Node::Box).collect();
    let mut bbs: Vec<BBox> = boxes.iter().map(|b| b.bbox).collect();
    let dist = |a: &BBox, b: &BBox| {
        let (x0, y0) = (min2(a[0], b[0]), min2(a[1], b[1]));
        let (x1, y1) = (max2(a[2], b[2]), max2(a[3], b[3]));
        (x1 - x0) * (y1 - y0) - width(a) * height(a) - width(b) * height(b)
    };
    let mut heap = BinaryHeap::new();
    for i in 0..boxes.len() {
        for j in i + 1..boxes.len() {
            heap.push(Reverse((false, F(dist(&bbs[i], &bbs[j])), i, j)));
        }
    }
    let mut plane = Plane::new(page_bbox);
    for (i, b) in bbs.iter().enumerate() {
        plane.add(i, *b);
    }
    let mut done = HashSet::new();
    while let Some(Reverse((skip_isany, d, id1, id2))) = heap.pop() {
        if done.contains(&id1) || done.contains(&id2) {
            continue;
        }
        if !skip_isany {
            let (a, b) = (&bbs[id1], &bbs[id2]);
            let u = [
                min2(a[0], b[0]),
                min2(a[1], b[1]),
                max2(a[2], b[2]),
                max2(a[3], b[3]),
            ];
            if plane.find(&u).into_iter().any(|o| o != id1 && o != id2) {
                heap.push(Reverse((true, d, id1, id2)));
                continue;
            }
        }
        let mut gb = [INF, INF, -INF, -INF];
        expand(&mut gb, &bbs[id1]);
        expand(&mut gb, &bbs[id2]);
        let g = nodes.len();
        nodes.push(Node::Group(vec![id1, id2]));
        bbs.push(gb);
        plane.remove(id1);
        plane.remove(id2);
        done.insert(id1);
        done.insert(id2);
        for other in plane.iter() {
            heap.push(Reverse((false, F(dist(&gb, &bbs[other])), g, other)));
        }
        plane.add(g, gb);
    }
    let roots = plane.iter();
    (nodes, bbs, roots)
}

/// Python's `a < b` ordering for sort keys (signed zeros are equal).
fn py_cmp(a: f64, b: f64) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

/// LTTextGroupLRTB.analyze: children first, then this group's order.
fn sort_group(nodes: &mut [Node], bbs: &[BBox], id: usize) {
    let Node::Group(children) = &mut nodes[id] else {
        return;
    };
    let mut children = std::mem::take(children);
    for &c in &children {
        sort_group(nodes, bbs, c);
    }
    let key = |c: &usize| {
        let b = &bbs[*c];
        (1.0 - BOXES_FLOW) * b[0] - (1.0 + BOXES_FLOW) * (b[1] + b[3])
    };
    children.sort_by(|a, b| py_cmp(key(a), key(b)));
    nodes[id] = Node::Group(children);
}

/// IndexAssigner: boxes numbered in group order.
fn assign(nodes: &[Node], id: usize, out: &mut Vec<usize>) {
    match &nodes[id] {
        Node::Box(b) => out.push(*b),
        Node::Group(children) => {
            for &c in children {
                assign(nodes, c, out);
            }
        }
    }
}

fn figure_text(items: &[Item], out: &mut String) {
    for it in items {
        match it {
            Item::Char(c) => out.push_str(&c.text),
            Item::Figure(f) => figure_text(f, out),
        }
    }
}

/// LTPage.analyze(LAParams()) and TextConverter.receive_layout.
pub fn page_text(page: &PageItems) -> String {
    let chars: Vec<&Char> = page
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Char(c) => Some(c),
            Item::Figure(_) => None,
        })
        .collect();
    let mut out = String::new();
    let figures = |out: &mut String| {
        for it in &page.items {
            if let Item::Figure(f) = it {
                figure_text(f, out);
            }
        }
    };
    if chars.is_empty() {
        figure_text(&page.items, &mut out);
        out.push('\x0c');
        return out;
    }
    let (empties, lines): (Vec<Line>, Vec<Line>) = group_objects(&chars)
        .into_iter()
        .partition(|l| l.is_empty(&chars));
    let mut boxes = group_textlines(page.bbox, &lines);
    let (mut nodes, bbs, roots) = group_textboxes(page.bbox, &boxes);
    let mut order = Vec::new();
    for root in roots {
        sort_group(&mut nodes, &bbs, root);
        assign(&nodes, root, &mut order);
    }
    for b in &mut boxes {
        // LTTextBoxHorizontal.analyze: lines top to bottom (stable)
        b.lines
            .sort_by(|x, y| py_cmp(-lines[*x].bbox[3], -lines[*y].bbox[3]));
    }
    for b in order {
        for &l in &boxes[b].lines {
            out.push_str(&lines[l].text(&chars));
            out.push('\n');
        }
        out.push('\n');
    }
    figures(&mut out);
    for l in &empties {
        out.push_str(&l.text(&chars));
        out.push('\n');
    }
    out.push('\x0c');
    out
}
