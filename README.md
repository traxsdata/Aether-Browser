# Aether-Browser

The web browser for the æther OS ([github.com/traxsdata/Aether](https://github.com/traxsdata/Aether)):
a from-scratch HTML parser, CSS engine, layout, picture decoders and
JavaScript engine ("æther script"), in Rust with no external crates and
no third-party fonts. It runs on æther as an untrusted app with a window and
outgoing connections only. See [docs/BROWSER.md](docs/BROWSER.md).

![Browser with two tabs](docs/browser-tabs.png)

## Building

Needs Rust (the version in `rust-toolchain.toml`, with the
`x86_64-unknown-linux-musl` target) and an Aether checkout for the packaging
tool, by default next to this one:

```sh
git clone https://github.com/traxsdata/Aether ../Aether
cargo test                      # the engine, hostile-input and UI tests (headless)
tools/build.sh                  # build/browser.app, the æther package
tools/build.sh --shots          # also headless screenshots of testpages/ in build/shots
cargo run --example render -- page.html out.png   # render one page to a PNG
```

The network layer (`webnet`) and the window/system-call layer
(`aether-host`, the only `unsafe` code the browser runs) come from the Aether
repository at the commit `Cargo.lock` pins. `cargo update -p webnet -p
aether-host` moves to Aether's latest main; `tools/build.sh --local-aether`
builds against the Aether checkout instead, for a change that spans both.

## Publishing

Every push and pull request runs the tests and builds the package
([.github/workflows/build-browser.yml](.github/workflows/build-browser.yml)).
Actions → **Build the browser** → Run workflow also publishes it to the
`browser-latest` release:

- `browser-<version>-aether-x86_64.aeapp`: the æther package, byte for byte.
  (GitHub refuses release assets ending in `.app`.)
- `browser-<version>-aether-x86_64.app.zip`: the same package as a `.app`
  file, for manual download.
- `browser.json`: the version, the browser and Aether commits it was built
  from, size and SHA-256.

In an Aether checkout, `make browser && make` downloads the package, checks
its SHA-256 and puts it in the ISO's `/apps`. The browser is not part of the
installed system image.

## Rules for changes

- Clean room: everything here is written for æther. No crates.io
  dependencies, no third-party fonts or data.
- `unsafe` stays in Aether's `aether-host`; the browser itself has none.
- Least capabilities: the package asks for a window and the network, never
  for the user's files.
- Every parser change comes with hostile-input tests (`tests/hostile.rs`).
