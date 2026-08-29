#!/bin/sh
# Fully static release build for the controller (musl, no libc dependency).
#
# Requires:
#   rustup toolchain: nightly with rust-src and the musl target
#     rustup target add aarch64-unknown-linux-musl
#     rustup component add rust-src
#   musl cross toolchain in PATH or in ~/.local/opt (https://musl.cc):
#     aarch64-linux-musl-cross for the controller,
#     armv7l-linux-musleabihf-cross for 32-bit armhf controllers,
#     x86_64-linux-musl-native to build/test the static binary locally.
#
# Usage:
#   ./build-static.sh                     # aarch64-unknown-linux-musl
#   ./build-static.sh armv7-unknown-linux-musleabihf
#   ./build-static.sh x86_64-unknown-linux-musl
set -e

TARGET="${1:-aarch64-unknown-linux-musl}"

for tc in aarch64-linux-musl-cross armv7l-linux-musleabihf-cross x86_64-linux-musl-native; do
    if [ -d "$HOME/.local/opt/$tc/bin" ]; then
        PATH="$HOME/.local/opt/$tc/bin:$PATH"
    fi
done
export PATH

# build-std with panic=immediate-abort strips all panic/fmt machinery from
# std and rebuilds it with the size-oriented profile of this crate.
# rust's bundled lld links with identical-code-folding, deduplicating
# equal monomorphic instances the fat LTO kept apart.
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Zunstable-options -Cpanic=immediate-abort     -Clinker-flavor=gnu-lld-cc -Clink-self-contained=+linker -Clink-arg=-Wl,--icf=all"
exec cargo +nightly build --release --target "$TARGET" \
    -Z build-std=std,panic_abort \
    -Z build-std-features=optimize_for_size
