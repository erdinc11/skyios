#!/bin/sh
# Build libsweet_kernel.a for the SWEET XNU kernel (linked through
# LD_KERNEL_ARCHIVES, xnu patch 0087). Needs a nightly with rust-src:
# custom target spec + build-std. TOOLCHAIN overrides the nightly used.
set -eu
cd "$(dirname "$0")"
MACOSX_DEPLOYMENT_TARGET=13.0 cargo "+${TOOLCHAIN:-nightly}" build --release \
	-Z build-std=core,compiler_builtins -Z json-target-spec \
	--target aarch64-apple-xnu-sweet.json
lib=target/aarch64-apple-xnu-sweet/release/libsweet_kernel.a
# Everything the archive needs from the kernel must exist there: only the
# C hooks in osfmk/arm64/sweet_rs.c and plain libc memory functions.
undef=$(nm -u "$lib" 2>/dev/null | grep '^_' | sort -u | grep -v -E '^_(sweet_rs_[a-z_]+|memcpy|memmove|memset|memcmp|bcmp|bzero)$' || true)
if [ -n "$undef" ]; then
	echo "error: unexpected undefined symbols in $lib:" >&2
	echo "$undef" >&2
	exit 1
fi
echo "$PWD/$lib"
