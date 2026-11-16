#!/bin/sh
# build.sh IMAGE OUT_DIR -> OUT_DIR/posterwall.tool: the signed tool + the
# SkyIOS wallpaper collection made from IMAGE, for mkpayload --ramdisk
set -eu
here=$(cd "$(dirname "$0")" && pwd)
img=$1
out=$2
mkdir -p "$out"
xcrun -sdk iphoneos clang -arch arm64 -mios-version-min=16.0 -O2 -Wall -fobjc-arc -framework Foundation -framework UIKit -framework CoreGraphics \
	-o "$out/posterwall" "$here/posterwall.m"
codesign -f -s - --identifier org.skyios.posterwall --entitlements "$here/entitlements.plist" "$out/posterwall"
"$here/make-collection.sh" "$img" "$out" >/dev/null
c=$out/SkyIOS.wallpaperCollection
w=Wallpapers/9000.SkyIOS-375w-812h@3x~iphone.wallpaper
d=/private/var/root/wp/C
"$here/../../target/release/skyios" mktool "$out/posterwall.tool" "$out/posterwall=/private/var/root/skyios-posterwall" \
	/private/var/root/wp/ $d/ $d/Wallpapers/ "$d/$w/" \
	"$c/Info.plist=$d/Info.plist" "$c/WallpaperCollection.plist=$d/WallpaperCollection.plist" \
	"$c/$w/Wallpaper.plist=$d/$w/Wallpaper.plist" "$c/$w/SkyIOS.png=$d/$w/SkyIOS.png" \
	"$c/$w/SkyIOS-thumb.png=$d/$w/SkyIOS-thumb.png"
