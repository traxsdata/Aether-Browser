//! inline.rs - inline formatting: white-space processing, line breaking,
//! vertical alignment on the baseline, text-align (including justify), and
//! the text runs, inline backgrounds and link regions each line produces.

use std::rc::Rc;

use super::flow::{Flow, Laid};
use super::*;
use crate::css::style::{Position, TextAlign, VAlign};

#[derive(Debug)]
enum P<'a> {
    Word { ts: &'a Rc<TextStyle>, text: String, w: f32 },
    /// brk: a line may break here. collapsible: dropped at line ends.
    Space { ts: &'a Rc<TextStyle>, w: f32, brk: bool, collapsible: bool },
    Atomic { b: &'a LBox, link: Option<u32> },
    Open { ts: &'a Rc<TextStyle>, w: f32, node: Option<NodeId> },
    Close { w: f32 },
    /// A forced break, with the `clear` of a <br>.
    Break(u8),
}

fn text_w(ts: &TextStyle, s: &str) -> f32 {
    ts.face.width(s) + ts.letter_spacing * s.chars().count() as f32
}

/// Inline items to pieces, with white space handled per `white-space`.
fn pieces(inl: &[Inl]) -> Vec<P<'_>> {
    let mut out: Vec<P> = Vec::new();
    // At the start of a line, collapsible spaces are dropped.
    let mut after_space = true;
    for item in inl {
        match item {
            Inl::Text(ts, t) => {
                let ws = ts.ws;
                let space_w = text_w(ts, " ");
                let mut word = String::new();
                for c in t.chars() {
                    let is_ws = matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C');
                    if !is_ws {
                        word.push(c);
                        after_space = false;
                        continue;
                    }
                    flush_word(&mut word, &mut out, ts);
                    if c == '\n' && ws.keeps_newlines() {
                        out.push(P::Break(0));
                        after_space = true;
                        continue;
                    }
                    if ws.collapses_spaces() {
                        if !after_space {
                            out.push(P::Space { ts, w: space_w, brk: ws.wraps(), collapsible: true });
                            after_space = true;
                        }
                    } else {
                        let w = if c == '\t' { space_w * 4.0 } else { space_w };
                        out.push(P::Space { ts, w, brk: ws.wraps(), collapsible: false });
                        after_space = false;
                    }
                }
                flush_word(&mut word, &mut out, ts);
            }
            Inl::Open(ts, w, node) => out.push(P::Open { ts, w: *w, node: *node }),
            Inl::Close(w) => out.push(P::Close { w: *w }),
            Inl::Atomic(b, link) => {
                out.push(P::Atomic { b, link: *link });
                after_space = false;
            }
            Inl::Br(ts) => {
                out.push(P::Break(ts.clear));
                after_space = true;
            }
        }
    }
    out
}

fn flush_word<'a>(word: &mut String, out: &mut Vec<P<'a>>, ts: &'a Rc<TextStyle>) {
    if !word.is_empty() {
        let w = text_w(ts, word);
        out.push(P::Word { ts, text: std::mem::take(word), w });
    }
}

fn is_break_point(p: &P) -> bool {
    matches!(p, P::Space { brk: true, .. } | P::Break(_) | P::Atomic { .. })
}

struct AtomicInfo {
    w: f32,
    ml: f32,
    mr: f32,
    mt: f32,
    mb: f32,
    start: usize,
    end: usize,
    laid: Laid,
    valign: VAlign,
    /// Absolutely positioned: takes no space in the line.
    abs: bool,
    /// Floated (Some(true) left, Some(false) right): out of the line.
    float: Option<bool>,
    clear: u8,
}

/// A broken line, before it is emitted.
struct LineRec {
    placed: Vec<Placed>,
    forced: bool,
    /// Top, and the free span (relative to the container) beside the floats.
    y: f32,
    left: f32,
    avail: f32,
    above: f32,
    below: f32,
    visible: bool,
}

#[derive(Debug)]
struct Placed {
    idx: usize,
    x: f32,
    w: f32,
    /// For a word split across lines: this line's part.
    text: Option<String>,
}

impl Flow {
    pub(crate) fn inline_intrinsic(&mut self, inl: &[Inl]) -> (f32, f32) {
        let ps = pieces(inl);
        let mut mn: f32 = 0.0;
        let mut mx: f32 = 0.0;
        let mut unit = 0.0;
        let mut line = 0.0;
        for p in &ps {
            match p {
                P::Word { w, .. } | P::Open { w, .. } | P::Close { w } => {
                    unit += w;
                    line += w;
                }
                P::Space { w, brk, .. } => {
                    if *brk {
                        mn = mn.max(unit);
                        unit = 0.0;
                    } else {
                        unit += w;
                    }
                    line += w;
                }
                P::Atomic { b, .. } => {
                    mn = mn.max(unit);
                    unit = 0.0;
                    let (amn, amx) = self.intrinsic(b);
                    let m = b.style.margin[3].resolve(Some(0.0)).unwrap_or(0.0).max(0.0) + b.style.margin[1].resolve(Some(0.0)).unwrap_or(0.0).max(0.0);
                    mn = mn.max(amn + m);
                    line += amx + m;
                }
                P::Break(_) => {
                    mn = mn.max(unit);
                    unit = 0.0;
                    mx = mx.max(line);
                    line = 0.0;
                }
            }
        }
        mn = mn.max(unit);
        mx = mx.max(line);
        (mn.ceil(), mx.ceil().max(mn.ceil()))
    }

    /// Lays out inline content in a block `cw` wide at (cx, cy). Returns
    /// (height, first baseline, last baseline).
    pub(crate) fn inline_flow(&mut self, b: &LBox, cx: f32, cy: f32, cw: f32) -> (f32, Option<f32>, Option<f32>) {
        let s = &b.style;
        let ps = pieces(&b.inlines);
        if ps.is_empty() {
            return (0.0, None, None);
        }
        // Size the atomic inlines first (laid out at the origin, moved later).
        let mut atomics: Vec<Option<AtomicInfo>> = Vec::with_capacity(ps.len());
        let mut floats_seen = self.floats.len();
        for p in &ps {
            if let P::Atomic { b: ab, .. } = p {
                let st = &ab.style;
                let w = self.box_width(ab, cw, false);
                let ml = st.margin[3].or_zero(Some(cw));
                let mr = st.margin[1].or_zero(Some(cw));
                let mt = st.margin[0].or_zero(Some(cw));
                let mb = st.margin[2].or_zero(Some(cw));
                let start = self.items.len();
                let laid = self.layout_box(ab, 0.0, 0.0, w, cw, None);
                let end = self.items.len();
                let abs = matches!(st.position, Position::Absolute | Position::Fixed);
                let float = if st.is_float() && floats_seen < super::flow::MAX_FLOATS {
                    floats_seen += 1;
                    Some(st.float == crate::css::style::Float::Left)
                } else {
                    None
                };
                atomics.push(Some(AtomicInfo { w, ml, mr, mt, mb, start, end, laid, valign: st.valign, abs, float, clear: st.clear }));
            } else {
                atomics.push(None);
            }
        }
        let width_of = |i: usize, p: &P| -> f32 {
            match p {
                P::Word { w, .. } | P::Space { w, .. } | P::Open { w, .. } | P::Close { w } => *w,
                P::Atomic { .. } => atomics[i].as_ref().map_or(0.0, |a| if a.abs || a.float.is_some() { 0.0 } else { a.w + a.ml + a.mr }),
                P::Break(_) => 0.0,
            }
        };
        // Line breaking, one line at a time: each line's span between the
        // floats depends on where the lines above it ended.
        let indent = s.indent.or_zero(Some(cw));
        let face = s.face();
        let lh = s.line_px();
        let half = (lh - (face.ascent() + face.descent())) / 2.0;
        let strut = (face.ascent() + half, face.descent() + half);
        let xh = s.font_size * 0.25;
        let x_end = cx + cw;
        let mut lines: Vec<LineRec> = Vec::new();
        let mut line: Vec<Placed> = Vec::new();
        let mut y = cy;
        let (l0, r0) = self.band(y, lh, cx, x_end);
        let mut left = l0 - cx;
        let mut avail = r0 - l0;
        let mut x = indent;
        let mut content = false;
        let mut pending: Vec<usize> = Vec::new();
        let mut float_top = f32::MIN;
        let mut i = 0;
        macro_rules! reband {
            () => {{
                let (l, r) = self.band(y, lh, cx, x_end);
                left = l - cx;
                avail = r - l;
            }};
        }
        macro_rules! finish {
            ($forced:expr) => {{
                // Trailing collapsible spaces hang off the end and are dropped.
                let mut k = line.len();
                while k > 0 {
                    match &ps[line[k - 1].idx] {
                        P::Close { .. } => k -= 1,
                        P::Space { collapsible: true, .. } => {
                            line.remove(k - 1);
                            k -= 1;
                        }
                        _ => break,
                    }
                }
                let (above, below, visible) = line_metrics(&line, &ps, &atomics, strut, xh, $forced);
                lines.push(LineRec { placed: std::mem::take(&mut line), forced: $forced, y, left, avail, above, below, visible });
                if visible {
                    y += (above + below).max(0.0);
                }
                // Floats that didn't fit beside the line go below it.
                for fi in std::mem::take(&mut pending) {
                    if let (Some(a), P::Atomic { link, .. }) = (atomics[fi].as_ref(), &ps[fi]) {
                        self.place_float(a, *link, y, cx, x_end, &mut float_top);
                    }
                }
                reband!();
            }};
        }
        while i < ps.len() {
            match &ps[i] {
                P::Break(clear) => {
                    line.push(Placed { idx: i, x, w: 0.0, text: None });
                    finish!(true);
                    if *clear != 0 {
                        y = self.clear_y(y, *clear);
                        reband!();
                    }
                    x = 0.0;
                    content = false;
                    i += 1;
                    continue;
                }
                P::Space { brk: true, collapsible, w, .. } => {
                    if !(*collapsible && !content) {
                        line.push(Placed { idx: i, x, w: *w, text: None });
                        x += w;
                    }
                    i += 1;
                    continue;
                }
                P::Atomic { link, .. } => {
                    if let Some(a) = atomics[i].as_ref().filter(|a| a.float.is_some()) {
                        if content && x + a.w + a.ml + a.mr > avail + 0.01 {
                            pending.push(i);
                        } else {
                            self.place_float(a, *link, y, cx, x_end, &mut float_top);
                            reband!();
                        }
                        i += 1;
                        continue;
                    }
                }
                _ => {}
            }
            // A unit: pieces up to the next break opportunity.
            let mut j = i;
            let mut unit_w = 0.0;
            if matches!(ps[i], P::Atomic { .. }) {
                unit_w = width_of(i, &ps[i]);
                j = i + 1;
            } else {
                while j < ps.len() && !is_break_point(&ps[j]) {
                    unit_w += width_of(j, &ps[j]);
                    j += 1;
                }
            }
            let wraps = match &ps[i] {
                P::Word { ts, .. } | P::Space { ts, .. } | P::Open { ts, .. } => ts.ws.wraps(),
                _ => true,
            };
            if wraps && content && x + unit_w > avail + 0.01 {
                finish!(false);
                x = 0.0;
                content = false;
            }
            if !content && x + unit_w > avail + 0.01 && (left > 0.01 || avail < cw - 0.01) {
                // Too wide beside the floats: try below the nearest one.
                if let Some(b) = self.next_float_bottom(y, lh, cx, x_end).filter(|&b| b > y) {
                    y = b;
                    reband!();
                    continue;
                }
            }
            if unit_w > avail && x <= 0.01 && wraps {
                // Too long for any line: break inside words.
                for k in i..j {
                    let pw = width_of(k, &ps[k]);
                    if let P::Word { ts, text, .. } = &ps[k] {
                        if x + pw > avail {
                            let mut part = String::new();
                            let mut pw2 = 0.0;
                            for ch in text.chars() {
                                let cwid = text_w(ts, ch.encode_utf8(&mut [0; 4]));
                                if x + pw2 + cwid > avail && (pw2 > 0.0 || x > 0.0) {
                                    if !part.is_empty() {
                                        line.push(Placed { idx: k, x, w: pw2, text: Some(std::mem::take(&mut part)) });
                                    }
                                    finish!(false);
                                    x = 0.0;
                                    pw2 = 0.0;
                                }
                                part.push(ch);
                                pw2 += cwid;
                            }
                            if !part.is_empty() {
                                line.push(Placed { idx: k, x, w: pw2, text: Some(part) });
                                x += pw2;
                            }
                            content = true;
                            continue;
                        }
                    }
                    let ml = atomics[k].as_ref().map_or(0.0, |a| a.ml);
                    line.push(Placed { idx: k, x: x + ml, w: pw, text: None });
                    x += pw;
                    content = true;
                }
            } else {
                for k in i..j {
                    let pw = width_of(k, &ps[k]);
                    let ml = atomics[k].as_ref().map_or(0.0, |a| a.ml);
                    line.push(Placed { idx: k, x: x + ml, w: pw, text: None });
                    x += pw;
                }
                content = true;
            }
            i = j;
        }
        if !line.is_empty() || !pending.is_empty() {
            finish!(false);
        }
        let _ = (left, avail); // the last finish! recomputes them for a next line
        let end_y = y;
        // Emit the lines.
        let mut first = None;
        let mut last = None;
        let nlines = lines.len();
        for (li, rec) in lines.iter().enumerate() {
            let LineRec { placed: line, forced, y, left, avail, above, below, visible } = rec;
            let (y, left, avail, above, below) = (*y, *left, *avail, *above, *below);
            if !visible {
                // Anchors on an otherwise empty line still get a position.
                for p in line {
                    if let P::Open { node: Some(n), .. } = ps[p.idx] {
                        self.push(Item::Anchor { y, node: n });
                    }
                }
                continue;
            }
            let line_h = (above + below).max(0.0);
            let base = y + above;
            let end_x = line.last().map_or(0.0, |p| {
                let ml = atomics[p.idx].as_ref().map_or(0.0, |a| a.ml);
                p.x - ml + p.w
            });
            let free = avail - end_x;
            let last_line = li + 1 == nlines;
            let spaces = line.iter().filter(|p| matches!(ps[p.idx], P::Space { .. })).count();
            let (offset, extra) = match s.text_align {
                _ if free <= 0.0 => (0.0, 0.0),
                TextAlign::Left => (0.0, 0.0),
                TextAlign::Right => (free, 0.0),
                TextAlign::Center => (free / 2.0, 0.0),
                TextAlign::Justify if !forced && !last_line && spaces > 0 => (0.0, free / spaces as f32),
                TextAlign::Justify => (0.0, 0.0),
            };
            let justify = extra > 0.0;
            // Runs of text with one style.
            let mut run: Option<(Rc<TextStyle>, f32, String, f32)> = None; // (style, x, text, width)
            let mut add = 0.0;
            let flush = |f: &mut Flow, run: &mut Option<(Rc<TextStyle>, f32, String, f32)>| {
                if let Some((ts, rx, text, rw)) = run.take() {
                    f.emit_run(&ts, rx, base, &text, rw, y, line_h);
                }
            };
            for p in line {
                let px = cx + left + offset + p.x + add;
                match &ps[p.idx] {
                    P::Word { ts, text, .. } => {
                        let t = p.text.as_deref().unwrap_or(text);
                        match &mut run {
                            Some((rts, rx, rt, rw)) if Rc::ptr_eq(rts, ts) && (*rx + *rw - px).abs() < 0.5 => {
                                rt.push_str(t);
                                *rw += p.w;
                            }
                            _ => {
                                flush(self, &mut run);
                                run = Some(((*ts).clone(), px, t.to_string(), p.w));
                            }
                        }
                    }
                    P::Space { ts, .. } => {
                        let w = p.w + if justify { extra } else { 0.0 };
                        match &mut run {
                            Some((rts, rx, rt, rw)) if Rc::ptr_eq(rts, ts) && !justify && (*rx + *rw - px).abs() < 0.5 => {
                                rt.push(' ');
                                *rw += w;
                            }
                            _ => {
                                flush(self, &mut run);
                                // Spaces matter for underlines and backgrounds.
                                if ts.deco != 0 || ts.bg.is_some() || ts.link.is_some() {
                                    run = Some(((*ts).clone(), px, " ".into(), w));
                                    if justify {
                                        flush(self, &mut run);
                                    }
                                }
                            }
                        }
                        if justify {
                            add += extra;
                        }
                    }
                    P::Open { node, .. } => {
                        if let Some(n) = node {
                            self.push(Item::Anchor { y, node: *n });
                        }
                    }
                    P::Atomic { link, .. } => {
                        flush(self, &mut run);
                        let a = atomics[p.idx].as_ref().unwrap();
                        let (asc, _) = atomic_metrics(a, xh);
                        let mut top = base - asc + a.mt;
                        let mut px = px;
                        if a.abs {
                            // Against the container's padding box.
                            let st = &atomics_style(&ps[p.idx]);
                            let [pt, pr, _, pl] = self.pad;
                            let pw = cw + pl + pr;
                            if let Some(l) = st.inset[3].resolve(Some(pw)) {
                                px = cx - pl + l + a.ml;
                            } else if let Some(r) = st.inset[1].resolve(Some(pw)) {
                                px = cx - pl + pw - r - a.w - a.mr;
                            }
                            if let Some(t) = st.inset[0].resolve(None) {
                                top = cy - pt + t + a.mt;
                            }
                        }
                        self.shift_from_range(a.start, a.end, px, top);
                        if let Some(l) = link {
                            self.push(Item::Hit { x: px, y: top, w: a.w, h: a.laid.h, target: Target::Link(*l) });
                        }
                        if first.is_none() {
                            first = Some(base);
                        }
                    }
                    P::Close { .. } | P::Break(_) => {}
                }
            }
            flush(self, &mut run);
            if first.is_none() {
                first = Some(base);
            }
            last = Some(base);
        }
        ((end_y - cy).max(0.0), first, last)
    }

    /// Puts a float at the highest place at or below `y` where it fits
    /// beside the floats already there, and records it.
    fn place_float(&mut self, a: &AtomicInfo, link: Option<u32>, y: f32, cx: f32, x_end: f32, float_top: &mut f32) {
        let fw = a.w + a.ml.max(0.0) + a.mr.max(0.0);
        let fh = (a.laid.h + a.mt + a.mb).max(0.0);
        // Not above an earlier float, and below the floats it clears.
        let y0 = self.clear_y(y.max(*float_top), a.clear);
        let (fy, l, r) = self.fit(y0, fh, fw, cx, x_end);
        let left = a.float == Some(true);
        let bx = if left { l } else { (r - fw).max(l) };
        let px = bx + a.ml.max(0.0);
        let top = fy + a.mt;
        self.shift_from_range(a.start, a.end, px, top);
        if let Some(l) = link {
            self.push(Item::Hit { x: px, y: top, w: a.w, h: a.laid.h, target: Target::Link(l) });
        }
        *float_top = fy;
        if self.floats.len() < super::flow::MAX_FLOATS {
            self.floats.push(super::flow::FloatBox { left, x0: bx, x1: bx + fw, y0: fy, y1: fy + fh });
        }
    }

    fn emit_run(&mut self, ts: &TextStyle, x: f32, base: f32, text: &str, w: f32, line_y: f32, line_h: f32) {
        if ts.hidden {
            return;
        }
        let y = base - ts.shift;
        if let Some(bg) = ts.bg {
            let f = ts.face;
            self.push(Item::Rect { x: x - 1.0, y: y - f.ascent() + f.cell_h() * 0.06, w: w + 2.0, h: f.ascent() + f.descent(), color: bg, radius: 3.0 });
        }
        if !text.trim().is_empty() || ts.deco != 0 {
            self.push(Item::Text {
                x,
                y,
                text: text.to_string(),
                face: ts.face,
                color: ts.color,
                deco: ts.deco,
                deco_color: ts.deco_color.unwrap_or(ts.color),
                letter_spacing: ts.letter_spacing,
            });
        }
        if let Some(l) = ts.link {
            self.push(Item::Hit { x, y: line_y, w, h: line_h, target: Target::Link(l) });
        }
        if let Some(node) = ts.elem {
            if !text.trim().is_empty() {
                self.push(Item::Region { x, y: line_y, w, h: line_h, node });
            }
        }
    }
}

/// A line's (above, below) its baseline, and whether it shows anything
/// (empty lines take no space).
fn line_metrics(line: &[Placed], ps: &[P], atomics: &[Option<AtomicInfo>], strut: (f32, f32), xh: f32, forced: bool) -> (f32, f32, bool) {
    let visible = forced
        || line.iter().any(|p| matches!(ps[p.idx], P::Word { .. } | P::Atomic { .. } | P::Space { collapsible: false, .. }))
        || line.iter().any(|p| matches!(ps[p.idx], P::Open { w, .. } if w > 0.0));
    let (mut above, mut below) = strut;
    for p in line {
        match &ps[p.idx] {
            P::Word { ts, .. } | P::Space { ts, .. } | P::Open { ts, .. } => {
                let f = ts.face;
                let hl = (ts.line_h - (f.ascent() + f.descent())) / 2.0;
                above = above.max(f.ascent() + hl + ts.shift);
                below = below.max(f.descent() + hl - ts.shift);
            }
            P::Atomic { .. } => {
                if let Some(a) = atomics[p.idx].as_ref() {
                    let (asc, desc) = atomic_metrics(a, xh);
                    above = above.max(asc);
                    below = below.max(desc);
                }
            }
            _ => {}
        }
    }
    (above, below, visible)
}

/// (above, below) the baseline for an atomic inline, margins included.
fn atomic_metrics(a: &AtomicInfo, xh: f32) -> (f32, f32) {
    let total = a.laid.h + a.mt + a.mb;
    match a.valign {
        VAlign::Middle => (total / 2.0 + xh, total / 2.0 - xh),
        VAlign::Top | VAlign::TextTop => (total.min(xh * 4.0), (total - xh * 4.0).max(0.0)),
        VAlign::Bottom | VAlign::TextBottom => (total, 0.0),
        _ => match a.laid.last_base {
            // Inline blocks with text sit on their last line's baseline.
            Some(lb) if lb > 0.0 && lb <= a.laid.h => (lb + a.mt, total - lb - a.mt),
            _ => (total, 0.0),
        },
    }
}

fn atomics_style<'a>(p: &P<'a>) -> std::rc::Rc<crate::css::Style> {
    match p {
        P::Atomic { b, .. } => b.style.clone(),
        _ => std::rc::Rc::new(crate::css::Style::initial()),
    }
}
