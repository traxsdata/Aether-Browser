#!/bin/sh
# build.sh - builds the browser and packages it as build/browser.app: a
# window and outgoing connections (CAP_WINDOW and CAP_NET in the header), no
# file picker and no private folder, since it keeps nothing.
#
#   tools/build.sh                    build and package
#   tools/build.sh --local-aether     build against $AETHER_DIR's runner/aether-host
#                                     and runner/webnet instead of the commit
#                                     Cargo.lock pins (for changes that span both)
#   tools/build.sh --shots            also render headless screenshots of the
#                                     test pages into build/shots
#   tools/build.sh --test-root CA.pem TEST BUILD: also trust CA.pem (Aether's
#                                     tools/test-https.sh); never ship this package
#
# Packaging uses tools/mkapp.py from an Aether checkout: $AETHER_DIR, default
# ../Aether. Needs Rust with the x86_64-unknown-linux-musl target.
set -e
cd "$(dirname "$0")/.."
root=$(pwd)
AETHER_DIR=$(cd "${AETHER_DIR:-../Aether}" 2>/dev/null && pwd) || {
    echo "build.sh: no Aether checkout at ${AETHER_DIR:-../Aether}; set AETHER_DIR" >&2
    exit 1
}
[ -f "$AETHER_DIR/tools/mkapp.py" ] || { echo "build.sh: $AETHER_DIR has no tools/mkapp.py" >&2; exit 1; }

unset WEBNET_TEST_ROOT_PEM # an extra trusted root only ever comes from --test-root
local_aether=
shots=
while [ $# -gt 0 ]; do
    case "$1" in
    --local-aether) local_aether=1; shift ;;
    --shots) shots=1; shift ;;
    --test-root)
        WEBNET_TEST_ROOT_PEM=$(cat "$2")
        export WEBNET_TEST_ROOT_PEM
        echo "build.sh: TEST BUILD trusting $2 as well as the built-in roots"
        shift 2 ;;
    *) echo "build.sh: unknown option $1" >&2; exit 2 ;;
    esac
done

set -- cargo build --release --target x86_64-unknown-linux-musl
if [ -n "$local_aether" ]; then
    # The patch rewrites Cargo.lock's sources; put the pinned one back after.
    mkdir -p build
    cp Cargo.lock build/Cargo.lock.pinned
    trap 'cp build/Cargo.lock.pinned Cargo.lock' EXIT
    git='patch."https://github.com/traxsdata/Aether"'
    set -- "$@" --config "$git.aether-host.path=\"$AETHER_DIR/runner/aether-host\"" \
        --config "$git.webnet.path=\"$AETHER_DIR/runner/webnet\""
fi
RUSTFLAGS="-C target-feature=+crt-static" "$@"

elf=target/x86_64-unknown-linux-musl/release/browser
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
commit=$(git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)
mkdir -p build
strip -o build/browser-stripped.elf "$elf"
"$AETHER_DIR/tools/mkapp.py" --name browser --title Browser --version "$version" \
    --source "github.com/traxsdata/Aether-Browser@$commit" \
    --quota 512 --color 38E1F5 --icon assets/browser-128.png --no-files --no-storage --net \
    build/browser-stripped.elf build/browser.app

if [ -n "$shots" ]; then
    out=build/shots
    mkdir -p "$out/shots"
    (cd "$out" && BROWSER_TEST_ROOT="$root/testpages" AETHER_SCRIPT="$root/tests/screens.txt" "$root/$elf")
    echo "screenshots in $out/shots"
fi
