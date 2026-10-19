//! live - renders a real site to a PNG on a development host, for finding
//! and checking rendering gaps. Never part of the browser on æther.
//!
//!   cargo run --release --example live -- https://en.wikipedia.org/ out.png [width] [max height]
//!
//! Fetches go through webnet itself (full TLS validation against the
//! built-in roots), tunnelled with CONNECT through $HTTPS_PROXY when it is
//! set. Every response is kept in $LIVE_CACHE (default target/live-cache),
//! so later runs render the same bytes without the network; LIVE_REFRESH=1
//! fetches again. SCRIPTS=0 turns the page's scripts off.

use std::sync::atomic::AtomicBool;

use browser::font::Cache;
use browser::net::{FetchError, Request, Response, Url};
use browser::page;
use browser::paint::{paint_page, Canvas};

fn cache_dir() -> std::path::PathBuf {
    std::env::var("LIVE_CACHE").map(Into::into).unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/target/live-cache").into())
}

fn key(req: &Request) -> String {
    // FNV-1a over the method and URL is plenty for a local cache.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in format!("{:?} {}", req.method, req.url).bytes().chain(req.body.iter().copied()) {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn load_cached(path: &std::path::Path) -> Option<Response> {
    let data = std::fs::read(path).ok()?;
    let split = data.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&data[..split]).ok()?;
    let mut lines = head.split("\r\n");
    let status: u16 = lines.next()?.parse().ok()?;
    let url = Url::parse(lines.next()?).ok()?;
    let secure = lines.next()? == "secure";
    let headers = lines.filter_map(|l| l.split_once(": ")).map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Some(Response { url, status, headers, body: data[split + 4..].to_vec(), secure })
}

fn store(path: &std::path::Path, r: &Response) {
    let mut out = format!("{}\r\n{}\r\n{}", r.status, r.url, if r.secure { "secure" } else { "insecure" }).into_bytes();
    for (k, v) in &r.headers {
        out.extend_from_slice(format!("\r\n{k}: {v}").as_bytes());
    }
    out.extend_from_slice(b"\r\n\r\n");
    out.extend_from_slice(&r.body);
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(path, out);
}

fn fetch(req: &Request) -> Result<Response, FetchError> {
    let path = cache_dir().join(key(req));
    if std::env::var("LIVE_REFRESH").is_err() {
        if let Some(r) = load_cached(&path) {
            return Ok(r);
        }
    }
    let mut cfg = webnet::Config::default();
    if let Ok(p) = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("https_proxy")) {
        let hp = p.trim_start_matches("http://").trim_end_matches('/');
        let hp = hp.rsplit('@').next().unwrap();
        if let Some((h, port)) = hp.rsplit_once(':') {
            cfg.proxy = Some((h.to_string(), port.parse().unwrap_or(80)));
        }
    }
    let t = std::time::Instant::now();
    let r = webnet::fetch_with(&cfg, req);
    match &r {
        Ok(resp) => {
            eprintln!("fetch {} -> {} ({} bytes, {:?})", req.url, resp.status, resp.body.len(), t.elapsed());
            store(&path, resp);
        }
        Err(e) => eprintln!("fetch {} failed: {e}", req.url),
    }
    r
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: live <url> <out.png> [width] [max height]");
        std::process::exit(2);
    }
    let w: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1024);
    let max_h: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(6000);
    let url = Url::parse(&args[1]).unwrap_or_else(|e| panic!("{e}"));
    let out = args[2].clone();
    let t = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let t0 = std::time::Instant::now();
            let mut policy = browser::script::Policy::default();
            if std::env::var("SCRIPTS").is_ok_and(|v| v == "0") {
                policy.set(&url, browser::script::Mode::Off);
            }
            let mut l = page::load(Request::get(url.clone()), None, &fetch, &AtomicBool::new(false), &policy).unwrap_or_else(|e| panic!("{e}"));
            for b in &l.blocked {
                eprintln!("blocked: {b}");
            }
            let mut form = page::initial_form(&l.doc);
            if l.script_mode.runs() {
                let page_url = match &l.location {
                    page::Location::Web(u) => u.clone(),
                    _ => url.clone(),
                };
                let mut rt = browser::script::Runtime::new(browser::script::Setup {
                    url: page_url,
                    base: l.base.clone(),
                    third_party: false,
                    fetcher: Some(&fetch),
                    storage: Default::default(),
                    viewport: (w as f32, 700.0),
                });
                let scripts = std::mem::take(&mut l.scripts);
                rt.run_page(&mut l.doc, &mut form, &scripts);
                rt.settle(&mut l.doc, &mut form, 2000.0);
                for e in rt.console() {
                    eprintln!("console: {e}");
                }
                l.title = l.doc.title();
            }
            let pg = page::lay_out(&l, w as f32, 700.0);
            if std::env::var("LIVE_BOTTOM").is_ok() {
                // The items reaching lowest, for debugging page heights.
                let mut v: Vec<(f32, &browser::layout::Item)> = pg.items.iter().filter_map(|it| it.y_range().map(|r| (r.1, it))).collect();
                v.sort_by(|a, b| b.0.total_cmp(&a.0));
                for (y, it) in v.iter().take(8) {
                    eprintln!("bottom {y}: {it:?}");
                }
            }
            let h = (pg.height.ceil() as u32).clamp(100, max_h);
            let mut px = vec![0u32; (w * h) as usize];
            let mut cache = Cache::default();
            {
                let mut cv = Canvas::new(&mut px, w, h, w);
                paint_page(&mut cv, &mut cache, &pg, &l.images, 0, 0, 0.0, &form);
            }
            eprintln!("title {:?}, {} items, {}x{} (page {}), total {:?}", l.title, pg.items.len(), w, h, pg.height, t0.elapsed());
            std::fs::write(&out, aether_host::png::encode_xrgb(&px, w, h, w)).unwrap();
        })
        .unwrap();
    if t.join().is_err() {
        std::process::exit(1);
    }
}
