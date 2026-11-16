#!/bin/sh
# build.sh OUT_DIR -> OUT_DIR/accel-test (ad-hoc signed iOS arm64) and
# OUT_DIR/accel-test.tool (skyios mktool container for mkpayload --ramdisk)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out=$1
mkdir -p "$out"
xcrun -sdk iphoneos clang -arch arm64 -mios-version-min=16.0 -O2 -Wall -framework IOKit \
	-o "$out/accel-test" "$here/accel-test.c"
codesign -f -s - --identifier org.skyios.accel-test "$out/accel-test"
"$here/../../target/release/skyios" mktool "$out/accel-test.tool" "$out/accel-test=/private/var/root/skyios-accel-test"
