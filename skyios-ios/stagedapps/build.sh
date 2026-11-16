#!/bin/sh
# build.sh OUT_DIR -> OUT_DIR/stagedapps (ad-hoc signed) and OUT_DIR/stagedapps.tool
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out=$1
mkdir -p "$out"
xcrun -sdk iphoneos clang -arch arm64 -mios-version-min=16.0 -O2 -Wall -fobjc-arc -framework Foundation \
	-o "$out/stagedapps" "$here/stagedapps.m"
codesign -f -s - --identifier org.skyios.stagedapps "$out/stagedapps"
"$here/../../target/release/skyios" mktool "$out/stagedapps.tool" "$out/stagedapps=/private/var/root/skyios-stagedapps"
