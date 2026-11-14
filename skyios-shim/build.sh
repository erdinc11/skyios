#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --release -p skyios-shim --target aarch64-unknown-none-softfloat
cargo build --release -p skyios-tool
payload=${1:?usage: skyios-shim/build.sh PAYLOAD [OUT.img]}
out=${2:-skyios-shim/out/shim-boot.img}
mkdir -p "$(dirname "$out")"
target/release/skyios mkshim target/aarch64-unknown-none-softfloat/release/skyios-shim "$payload" dtb/boot_unpacked/dtb "$out"
