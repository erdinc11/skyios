#!/bin/sh
set -eu
cd "$(dirname "$0")"
MACOSX_DEPLOYMENT_TARGET=13.0 cargo "+${TOOLCHAIN:-nightly}" build --release -p skyios-kernel \
	-Z build-std=core,compiler_builtins -Z json-target-spec \
	--target aarch64-apple-xnu-sweet.json ${SWEET_GMU_FW:+--features gmufw} ${SWEET_SQE_FW:+--features sqefw} ${SWEET_ZAP_MDT:+--features zapfw}
lib=$(cd .. && pwd)/target/aarch64-apple-xnu-sweet/release/libskyios_kernel.a
undef=$(nm -u "$lib" 2>/dev/null | grep '^_' | sort -u | grep -v -E '^_(sweet_rs_[a-z0-9_]+|memcpy|memmove|memset|memcmp|bcmp|bzero)$' || true)
if [ -n "$undef" ]; then
	echo "error: unexpected undefined symbols in $lib:" >&2
	echo "$undef" >&2
	exit 1
fi
echo "$lib"
