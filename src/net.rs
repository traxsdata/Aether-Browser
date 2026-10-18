//! net.rs - the seam between the browser and æther's network layer.
//!
//! Everything the browser knows about the network comes through these
//! re-exports from the `webnet` crate. While runner/webnet is being written
//! the dependency points at runner/webnet-stub, a stand-in with the same API
//! that serves built-in pages and refuses real network addresses; swapping
//! in the real crate is one line in runner/browser/Cargo.toml.

pub use webnet::{fetch, FetchError, Method, Request, Response, Url, UrlError};

/// The value of a response header (names compare case-insensitively).
pub fn header<'a>(r: &'a Response, name: &str) -> Option<&'a str> {
    r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}
