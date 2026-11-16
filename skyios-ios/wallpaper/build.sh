#!/bin/sh
# build.sh OUT_DIR -> OUT_DIR/wallpaper (signed with the wallpaper entitlement)
# and OUT_DIR/wallpaper.tool (skyios mktool container for mkpayload --ramdisk)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out=$1
mkdir -p "$out"
xcrun -sdk iphoneos clang -arch arm64 -mios-version-min=16.0 -O2 -Wall -fobjc-arc \
	-framework Foundation -framework UIKit -framework CoreGraphics -framework CoreText \
	-o "$out/wallpaper" "$here/wallpaper.m"
codesign -f -s - --identifier org.skyios.wallpaper --entitlements "$here/entitlements.plist" "$out/wallpaper"
"$here/../../target/release/skyios" mktool "$out/wallpaper.tool" "$out/wallpaper=/private/var/root/skyios-wallpaper"
