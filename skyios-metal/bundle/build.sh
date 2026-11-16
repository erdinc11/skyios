#!/bin/sh
# build.sh OUT_DIR -> OUT_DIR/SkyAdrenoMetal.bundle (ad-hoc signed iOS arm64)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
b=$1/SkyAdrenoMetal.bundle
mkdir -p "$b"
xcrun -sdk iphoneos clang -arch arm64 -mios-version-min=16.0 -O2 -Wall -fobjc-arc -bundle \
	-framework Foundation -framework IOKit -framework Metal -o "$b/SkyAdrenoMetal" "$here/SkyAdrenoMetal.m" "$here/SkyObjects.m"
plutil -convert binary1 -o "$b/Info.plist" "$here/Info.plist"
codesign -f -s - --identifier org.skyios.SkyAdrenoMetal "$b/SkyAdrenoMetal"
