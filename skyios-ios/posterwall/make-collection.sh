#!/bin/sh
# make-collection.sh IMAGE OUT_DIR -> OUT_DIR/SkyIOS.wallpaperCollection, a
# WallpaperKit collection with one "Still" wallpaper (id 9000) made from
# IMAGE (scaled to 1125 px wide, the iPhone X screen at @3x), plus thumbnail
set -eu
img=$1
out=$2/SkyIOS.wallpaperCollection
w=$out/Wallpapers/9000.SkyIOS-375w-812h@3x~iphone.wallpaper
rm -rf "$out"
mkdir -p "$w"
sips -s format png --resampleWidth 1125 "$img" --out "$w/SkyIOS.png" >/dev/null
sips -s format png --resampleWidth 225 "$img" --out "$w/SkyIOS-thumb.png" >/dev/null
cat > "$out/Info.plist" <<P
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
	<key>CFBundleDevelopmentRegion</key><string>en</string>
	<key>CFBundleIdentifier</key><string>org.skyios.wallpaper</string>
	<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
	<key>CFBundleName</key><string>SkyIOS</string>
	<key>CFBundlePackageType</key><string>BNDL</string>
	<key>CFBundleShortVersionString</key><string>1.0</string>
	<key>CFBundleVersion</key><string>1</string>
</dict></plist>
P
cat > "$out/WallpaperCollection.plist" <<P
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
	<key>identifier</key><string>5C1E5000-0000-4000-8000-000000009000</string>
	<key>isLegacyContent</key><false/>
	<key>name</key><string>SkyIOS</string>
	<key>order</key><array><integer>9000</integer></array>
	<key>source</key><string>Apple</string>
</dict></plist>
P
cat > "$w/Wallpaper.plist" <<P
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
	<key>appearanceAware</key><false/>
	<key>assets</key><dict><key>lockAndHome</key><dict><key>default</key><dict>
		<key>fullSizeImageFileName</key><string>SkyIOS.png</string>
		<key>thumbnailImageFileName</key><string>SkyIOS-thumb.png</string>
		<key>identifier</key><integer>9000</integer>
		<key>name</key><string>SkyIOS</string>
		<key>type</key><string>Still</string>
	</dict></dict></dict>
	<key>family</key><string>SkyIOS</string>
	<key>identifier</key><integer>9000</integer>
	<key>logicalScreenClass</key><string>375w-812h@3x~iphone</string>
	<key>name</key><string>SkyIOS</string>
	<key>version</key><integer>1</integer>
</dict></plist>
P
for f in "$out/Info.plist" "$out/WallpaperCollection.plist" "$w/Wallpaper.plist"; do plutil -lint "$f" >/dev/null; done
echo "$out"
