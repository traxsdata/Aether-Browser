//! Browser behaviour through the window's event handling: navigation,
//! the http warning, history, links, forms, scrolling and focus. Pages come
//! from the webnet stub (https://test.aether/ serves runner/browser/testpages).

use aether_host::{abi, Event, Mouse};
use browser::ui::State;

fn setup() -> State {
    std::env::set_var("AETHER_STUB_ROOT", concat!(env!("CARGO_MANIFEST_DIR"), "/testpages"));
    State::new(1000, 700, true)
}

/// Runs `f` on a thread with a large stack, as the browser does.
fn big<F: FnOnce() + Send + 'static>(f: F) {
    std::thread::Builder::new().stack_size(64 << 20).spawn(f).unwrap().join().unwrap();
}

fn key(st: &mut State, k: u32, mods: u32) {
    assert!(st.handle(Event::Key { key: k, mods }));
}

fn type_text(st: &mut State, s: &str) {
    for c in s.chars() {
        key(st, c as u32, 0);
    }
}

fn click(st: &mut State, (x, y): (i32, i32)) {
    let m = Mouse { x, y, buttons: 1, mods: 0 };
    st.handle(Event::MouseDown(m));
    st.handle(Event::MouseUp(Mouse { buttons: 0, ..m }));
}

fn go(st: &mut State, addr: &str) {
    key(st, 'l' as u32, abi::MOD_CTRL);
    type_text(st, addr);
    key(st, '\n' as u32, 0);
}

#[test]
fn bare_names_mean_https() {
    big(|| {
        let mut st = setup();
        assert_eq!(st.address(), "");
        go(&mut st, "test.aether");
        assert_eq!(st.address(), "https://test.aether/");
        assert!(st.page_text().contains("It works"), "{}", st.page_text());
        assert_eq!(st.window_title(), "test.aether");
        assert!(st.is_secure());
    });
}

#[test]
fn http_needs_consent_and_back_works() {
    big(|| {
        let mut st = setup();
        go(&mut st, "test.aether/article.html");
        assert!(st.page_text().contains("Lighthouse"));
        go(&mut st, "http://test.aether/listings.html");
        assert!(st.is_interstitial());
        assert!(st.page_text().contains("isn't private"));
        // "Continue over http" is the second focusable link.
        key(&mut st, '\t' as u32, 0);
        key(&mut st, '\t' as u32, 0);
        key(&mut st, '\n' as u32, 0);
        assert!(!st.is_interstitial());
        assert!(st.page_text().contains("Branches"));
        assert!(!st.is_secure());
        assert_eq!(st.address(), "http://test.aether/listings.html");
        // Back to the article, forward again (consent is remembered for the host).
        key(&mut st, abi::KEY_LEFT, abi::MOD_ALT);
        assert!(st.page_text().contains("Lighthouse"));
        key(&mut st, abi::KEY_RIGHT, abi::MOD_ALT);
        assert!(st.page_text().contains("Branches"));
    });
}

#[test]
fn errors_show_the_fetch_error() {
    big(|| {
        let mut st = setup();
        go(&mut st, "https://nowhere.example/");
        let t = st.page_text();
        assert!(t.contains("Can't open this page"), "{t}");
        assert!(t.contains("no network layer"), "{t}");
        go(&mut st, "not a valid address");
        assert!(st.page_text().contains("Can't open this page"));
    });
}

#[test]
fn hostile_page_cannot_escape() {
    big(|| {
        let mut st = setup();
        go(&mut st, "test.aether/hostile.html");
        let t = st.page_text();
        assert!(t.contains("A page that tries things"), "{t}");
        assert!(t.contains("noscript"), "noscript content is shown: {t}");
        assert!(!t.contains("You should not see this"));
        assert!(!t.contains("alert"));
        // A javascript: link does nothing.
        let p = st.find_link("javascript:").unwrap();
        click(&mut st, p);
        assert!(st.status().contains("disabled"), "{}", st.status());
        assert_eq!(st.address(), "https://test.aether/hostile.html");
        // A page can't use the warning page's internal "continue" action.
        let p = st.find_link("about:continue").unwrap();
        click(&mut st, p);
        assert_eq!(st.address(), "https://test.aether/hostile.html");
        // Nor open other internal pages.
        let p = st.find_link("about:config").unwrap();
        click(&mut st, p);
        assert_eq!(st.address(), "https://test.aether/hostile.html");
        // The in-page link scrolls.
        let p = st.find_link("#bottom").unwrap();
        st.handle(Event::Scroll { dx: 0, dy: 100, mods: 0 });
        let before = st.scroll();
        click(&mut st, p);
        assert!(st.scroll() >= before);
        assert_eq!(st.address(), "https://test.aether/hostile.html");
    });
}

#[test]
fn links_and_forms() {
    big(|| {
        let mut st = setup();
        go(&mut st, "test.aether/listings.html");
        let p = st.find_link("central.html").unwrap();
        click(&mut st, p);
        assert_eq!(st.address(), "https://test.aether/central.html");
        assert!(st.page_text().contains("404"));
        key(&mut st, 8, 0); // Backspace: back
        assert_eq!(st.address(), "https://test.aether/listings.html");
        // Tab to the search field, type, press Enter.
        for _ in 0..40 {
            key(&mut st, '\t' as u32, 0);
            if st.status().is_empty() {
                break; // a field (no status text)
            }
        }
        type_text(&mut st, "lighthouse keepers");
        key(&mut st, '\n' as u32, 0);
        assert_eq!(st.address(), "https://test.aether/search.html?q=lighthouse+keepers&where=All+branches");
    });
}

#[test]
fn scrolling() {
    big(|| {
        let mut st = setup();
        go(&mut st, "test.aether/article.html");
        assert_eq!(st.scroll(), 0.0);
        key(&mut st, abi::KEY_PGDN, 0);
        let one = st.scroll();
        assert!(one > 400.0, "{one}");
        key(&mut st, ' ' as u32, 0);
        assert!(st.scroll() > one);
        key(&mut st, abi::KEY_END, 0);
        let end = st.scroll();
        key(&mut st, abi::KEY_DOWN, 0);
        assert_eq!(st.scroll(), end);
        key(&mut st, abi::KEY_HOME, 0);
        assert_eq!(st.scroll(), 0.0);
        st.handle(Event::MouseMove(Mouse { x: 500, y: 400, buttons: 0, mods: 0 }));
        st.handle(Event::Scroll { dx: 0, dy: -3, mods: 0 });
        assert!(st.scroll() > 100.0);
        // Reload keeps the position.
        let s = st.scroll();
        key(&mut st, 'r' as u32, abi::MOD_CTRL);
        assert_eq!(st.scroll(), s);
    });
}

#[test]
fn drawing_doesnt_panic_at_odd_sizes() {
    big(|| {
        let mut st = setup();
        go(&mut st, "test.aether/styled.html");
        for (w, h) in [(1000u32, 700u32), (320, 240), (1600, 1000), (1, 1), (200, 60)] {
            st.resize(w as i32, h as i32);
            let mut px = vec![0u32; (w * h) as usize];
            st.draw(&mut px, w, h, w);
        }
    });
}
