//! render - lays an HTML file out and writes it as a PNG, for looking at the
//! engine's output without the browser's chrome.
//!
//!   cargo run -p browser --example render -- page.html out.png [width] [height]
//!
//! Linked stylesheets are read from the HTML file's folder (served as
//! https://test.aether/ through the webnet stub's AETHER_STUB_ROOT).

use std::sync::atomic::AtomicBool;

use browser::font::Cache;
use browser::net::{Request, Url};
use browser::page;
use browser::paint::{paint_page, Canvas};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: render <page.html> <out.png> [width] [max height]");
        std::process::exit(2);
    }
    let path = std::path::Path::new(&args[1]);
    let w: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(900);
    let max_h: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(4000);
    let dir = path.parent().unwrap_or(std::path::Path::new(".")).canonicalize().unwrap();
    std::env::set_var("AETHER_STUB_ROOT", &dir);
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    let out = args[2].clone();
    let t = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let url = Url::parse(&format!("https://test.aether/{name}")).unwrap();
            let t0 = std::time::Instant::now();
            let l = page::load(Request::get(url), None, page::default_fetcher(), &AtomicBool::new(false)).unwrap_or_else(|e| panic!("{e}"));
            for b in &l.blocked {
                eprintln!("blocked: {b}");
            }
            let pg = page::lay_out(&l, w as f32, 700.0);
            let t1 = t0.elapsed();
            let h = (pg.height.ceil() as u32).clamp(100, max_h);
            let mut px = vec![0u32; (w * h) as usize];
            let mut cache = Cache::default();
            let form = page::initial_form(&l.doc);
            {
                let mut cv = Canvas::new(&mut px, w, h, w);
                paint_page(&mut cv, &mut cache, &pg, 0, 0, 0.0, &form);
            }
            eprintln!("title {:?}, {} items, {}x{} (page {}), layout {:?}, total {:?}", l.title, pg.items.len(), w, h, pg.height, t1, t0.elapsed());
            std::fs::write(&out, aether_host::png::encode_xrgb(&px, w, h, w)).unwrap();
        })
        .unwrap();
    if t.join().is_err() {
        std::process::exit(1);
    }
}
