//! Layout geometry on small documents.

use browser::layout::{Item, Page, Target};
use browser::page;

fn lay(html: &str, w: f32) -> Page {
    let l = page::load_builtin("t", html);
    page::lay_out(&l, w, 600.0)
}

fn texts(p: &Page) -> Vec<(String, f32, f32)> {
    p.items
        .iter()
        .filter_map(|i| match i {
            Item::Text { x, y, text, .. } => Some((text.clone(), *x, *y)),
            _ => None,
        })
        .collect()
}

fn find(p: &Page, s: &str) -> (f32, f32) {
    texts(p).into_iter().find(|t| t.0.contains(s)).map(|t| (t.1, t.2)).unwrap_or_else(|| panic!("{s:?} not in {:?}", texts(p)))
}

#[test]
fn blocks_stack_and_margins_collapse() {
    let p = lay("<style>body{margin:0} p{margin:20px 0;line-height:20px;font-size:16px}</style><p>one<p>two", 400.0);
    let (_, y1) = find(&p, "one");
    let (_, y2) = find(&p, "two");
    // 20px line + 20px collapsed margin between the paragraphs.
    assert!((y2 - y1 - 40.0).abs() < 0.5, "{y1} {y2}");
}

#[test]
fn lines_wrap_within_width() {
    let p = lay("<p style='width:120px;margin:0'>alpha beta gamma delta epsilon zeta eta theta</p>", 400.0);
    let lines: std::collections::BTreeSet<i32> = texts(&p).iter().map(|t| t.2 as i32).collect();
    assert!(lines.len() >= 3, "{:?}", texts(&p));
    for (t, x, _) in texts(&p) {
        let face = browser::font::Face::new(16.0, false, false, false);
        assert!(x + face.width(&t) <= 8.0 + 120.0 + 1.0, "{t} overflows");
    }
}

#[test]
fn centering_and_alignment() {
    let p = lay("<body style='margin:0'><div style='width:200px;margin:0 auto'>c</div><p style='text-align:right;margin:0'>r</p>", 600.0);
    let (cx, _) = find(&p, "c");
    assert!((cx - 200.0).abs() < 1.0, "{cx}");
    let (rx, _) = find(&p, "r");
    assert!(rx > 580.0, "{rx}");
}

#[test]
fn tables_share_columns() {
    let p = lay("<table border=1><tr><td>a<td>bbbbbbbbbbbb<tr><td>cccccccc<td>d</table>", 600.0);
    let (ax, _) = find(&p, "a");
    let (cx, _) = find(&p, "cccccccc");
    let (bx, _) = find(&p, "bbbb");
    let (dx, _) = find(&p, "d");
    assert_eq!(ax, cx);
    assert_eq!(bx, dx);
    assert!(bx > cx + 40.0);
}

#[test]
fn lists_get_markers() {
    let p = lay("<ol start=3><li>x<li>y</ol><ul><li>z</ul>", 600.0);
    assert!(texts(&p).iter().any(|t| t.0 == "3."), "{:?}", texts(&p));
    assert!(texts(&p).iter().any(|t| t.0 == "4."));
    assert!(p.items.iter().any(|i| matches!(i, Item::Bullet { .. })));
}

#[test]
fn links_are_hit_targets() {
    let p = lay("<p style='margin:0'>go <a href='/next'>next page</a> now", 600.0);
    let (x, y) = find(&p, "next");
    assert_eq!(p.hit(x + 3.0, y - 4.0), Some(Target::Link(0)));
    assert_eq!(p.links[0], "/next");
    assert_eq!(p.hit(1.0, y - 4.0), None);
}

#[test]
fn display_none_and_scripts_are_not_shown() {
    let p = lay("<script>document.write('no')</script><p style=display:none>hidden<p hidden>also<p>shown<noscript>fallback</noscript>", 600.0);
    let t = p.text();
    assert!(t.contains("shown") && t.contains("fallback"), "{t}");
    assert!(!t.contains("no'") && !t.contains("hidden") && !t.contains("also"), "{t}");
}

#[test]
fn flex_rows() {
    let p = lay("<div style='display:flex;width:300px'><div style='flex:1'>a</div><div style='flex:2'>b</div></div>", 600.0);
    let (ax, _) = find(&p, "a");
    let (bx, ay) = find(&p, "b");
    assert!((bx - ax - 100.0).abs() < 1.0, "{ax} {bx}");
    assert_eq!(ay, find(&p, "a").1);
}

#[test]
fn images_are_placeholders_with_alt() {
    let p = lay("<img src=x.png alt='A cat' width=120 height=80>", 600.0);
    assert!(p.items.iter().any(|i| matches!(i, Item::Image { w, h, alt, .. } if *w == 120.0 && *h == 80.0 && alt == "A cat")));
}

fn deco_rects(p: &Page) -> Vec<(f32, f32, f32, f32)> {
    p.items
        .iter()
        .filter_map(|i| match i {
            Item::Deco { x, y, w, h, .. } => Some((*x, *y, *w, *h)),
            _ => None,
        })
        .collect()
}

#[test]
fn text_flows_around_floats() {
    let html = "<body style='margin:0;font-size:16px;line-height:20px'>\
        <div style='float:left;width:100px;height:60px;background:red'></div>\
        <div style='float:right;width:50px;height:30px;background:blue'></div>\
        <p style='margin:0'>alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon</p>";
    let p = lay(html, 400.0);
    let (ax, ay) = find(&p, "alpha");
    assert!(ax >= 100.0, "first line starts after the left float: {ax}");
    assert!(ay < 20.0, "and on the first line: {ay}");
    let rects = deco_rects(&p);
    assert!(rects.iter().any(|r| (r.0 - 350.0).abs() < 0.5 && r.1 == 0.0), "right float at the right edge: {rects:?}");
    for (t, x, y) in texts(&p) {
        let face = browser::font::Face::new(16.0, false, false, false);
        let right = x + face.width(&t);
        if y < 30.0 {
            assert!(right <= 350.5, "{t:?} runs under the right float");
        }
        if y < 60.0 {
            assert!(x >= 99.5, "{t:?} runs under the left float");
        }
    }
    // Below the floats, lines use the whole width again.
    let low: Vec<_> = texts(&p).into_iter().filter(|t| t.2 > 70.0).collect();
    assert!(low.iter().any(|t| t.1 < 1.0), "{low:?}");
}

#[test]
fn clear_moves_below_floats() {
    let p = lay("<body style='margin:0'><div style='float:left;width:50px;height:80px'></div><div style='clear:both'>after</div>", 400.0);
    let (x, y) = find(&p, "after");
    assert!(x < 1.0 && y > 80.0, "{x} {y}");
    let p = lay("<body style='margin:0'><img align=left width=50 height=80 alt=a>one<br clear=all>two", 400.0);
    let (_, y) = find(&p, "two");
    assert!(y > 80.0, "{y}");
}

#[test]
fn new_formatting_contexts_go_beside_floats_and_contain_them() {
    let p = lay("<body style='margin:0'><div style='float:left;width:100px;height:50px'></div><div style='overflow:hidden;background:#eee'>side</div>", 400.0);
    let (x, _) = find(&p, "side");
    assert!(x >= 100.0, "{x}");
    let rects = deco_rects(&p);
    assert!(rects.iter().any(|r| (r.0 - 100.0).abs() < 0.5 && (r.2 - 300.0).abs() < 0.5), "narrowed beside the float: {rects:?}");
    // A container that is its own formatting context grows around its floats.
    let p = lay("<body style='margin:0'><div style='overflow:hidden;background:#eee'><div style='float:left;width:10px;height:90px'></div></div><p style='margin:0'>below", 400.0);
    let (_, y) = find(&p, "below");
    assert!(y > 90.0, "{y}");
}

#[test]
fn page_scrolls_past_a_full_height_root() {
    let words = "word ".repeat(2000);
    let p = lay(&format!("<style>html,body{{height:100%;margin:0;overflow-x:hidden}}</style><p>{words}"), 400.0);
    assert!(p.height > 1000.0, "{}", p.height);
}

#[test]
fn center_and_align_center_centre_tables() {
    let p = lay("<body style='margin:0'><center><table style='width:200px'><tr><td>cell</table></center>", 600.0);
    let (x, _) = find(&p, "cell");
    assert!((195.0..215.0).contains(&x), "{x}");
    let p = lay("<body style='margin:0'><div align=center><table style='width:200px'><tr><td>cell</table></div>", 600.0);
    let (x, _) = find(&p, "cell");
    assert!((195.0..215.0).contains(&x), "{x}");
    // Plain text-align:center centres text, not blocks.
    let p = lay("<body style='margin:0'><div style='text-align:center'><table style='width:200px'><tr><td style='text-align:left'>cell</table></div>", 600.0);
    let (x, _) = find(&p, "cell");
    assert!(x < 20.0, "{x}");
}

#[test]
fn quirks_mode_tables_do_not_inherit_text_align() {
    // No doctype: quirks mode, as on many older sites.
    let p = lay("<body style='margin:0'><center><table style='width:400px'><tr><td>left</table></center>", 600.0);
    let (x, _) = find(&p, "left");
    assert!(x < 120.0, "{x}");
    // In standards mode the cell inherits the centring.
    let p = lay("<!doctype html><body style='margin:0'><center><table style='width:400px'><tr><td>mid</table></center>", 600.0);
    let (x, _) = find(&p, "mid");
    assert!(x > 250.0, "{x}");
}
