#!/bin/bash
set -euo pipefail

[[ "${MACHINE_CONFIG:-}" == SWEET ]] || exit 0
[[ "${MACOS_VERSION:-}" == 13.5 ]] || {
    echo 'SWEET HFS integration requires xnu-8796.141.3 (macOS 13.5)' >&2
    exit 1
}
[[ -f bsd/kern/bsd_init.c ]] || exit 1

hfs_sha=31d5ec238dfab485e78a9a30b8b92d13e8914f03
hfs_tmp=$(mktemp -d)
trap 'rm -rf "$hfs_tmp"' EXIT
git init -q "$hfs_tmp"
git -C "$hfs_tmp" fetch -q --depth 1 \
    https://github.com/apple-oss-distributions/hfs "$hfs_sha"
git -C "$hfs_tmp" checkout -q FETCH_HEAD
[[ "$(git -C "$hfs_tmp" rev-parse HEAD)" == "$hfs_sha" ]]

target=bsd/sweet_hfs
[[ ! -e $target ]] || { echo "refusing to overwrite existing HFS sources: $target" >&2; exit 1; }
mkdir -p "$target"
for d in core hfs_encodings; do cp -R "$hfs_tmp/$d" "$target/$d"; done
h=$target/core/hfs_iokit.h
INC=$'#include <AppleKeyStore/AppleKeyStoreFSServices.h>\n' FIRST='int hfs_unwrap_key(' END='__END_DECLS' awk '
function count(s, o,  c, i) { c = 0; while ((i = index(s, o)) > 0) { c++; s = substr(s, i + length(o)) } return c }
BEGIN { RS = "\001"; ORS = "" }
{
	s = $0; inc = ENVIRON["INC"]; first = ENVIRON["FIRST"]; end = ENVIRON["END"]
	if (count(s, inc) != 1 || count(s, first) != 1 || count(s, end) != 1) { print "HFS source changed" > "/dev/stderr"; exit 1 }
	i = index(s, inc); s = substr(s, 1, i - 1) "#if CONFIG_PROTECT\n" inc "#endif\n" substr(s, i + length(inc))
	i = index(s, first); j = index(s, end); mid = substr(s, i, j - i); sub(/\n+$/, "", mid)
	print substr(s, 1, i - 1) "#if CONFIG_PROTECT\n" mid "\n#endif\n\n" substr(s, j)
}' "$h" > "$h.tmp"
mv "$h.tmp" "$h"

hfs_sdk=$(xcrun --sdk macosx --show-sdk-path)
cp "$hfs_sdk/usr/include/TargetConditionals.h" bsd/sweet_hfs/
echo "Staged hfs-627.100.6 ($hfs_sha) for SWEET"
