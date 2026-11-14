#!/bin/bash
set -euo pipefail
[[ "${MACHINE_CONFIG:-}" == SWEET ]] || exit 0
[[ -f libkern/crypto/corecrypto_aes.c ]] || exit 1
bearssl_sha=7bea48e5e850ab4cafbe68d3765cdaba13a86d6f
bearssl_tmp=$(mktemp -d)
trap 'rm -rf "$bearssl_tmp"' EXIT
git init -q "$bearssl_tmp"
git -C "$bearssl_tmp" fetch -q --depth 1 \
    https://www.bearssl.org/git/BearSSL "$bearssl_sha"
git -C "$bearssl_tmp" checkout -q FETCH_HEAD
[[ "$(git -C "$bearssl_tmp" rev-parse HEAD)" == "$bearssl_sha" ]]
replace() {
	local file=$1 old=$2 new=$3 count=$4 n
	n=$(OLD="$old" awk 'BEGIN { RS = "\001" } { s = $0; o = ENVIRON["OLD"]; c = 0; while ((i = index(s, o)) > 0) { c++; s = substr(s, i + length(o)) } print c }' "$file")
	[[ "$n" == "$count" ]] || { echo "source changed: $file: $old" >&2; exit 1; }
	OLD="$old" NEW="$new" awk 'BEGIN { RS = "\001"; ORS = "" } { s = $0; o = ENVIRON["OLD"]; r = ""; while ((i = index(s, o)) > 0) { r = r substr(s, 1, i - 1) ENVIRON["NEW"]; s = substr(s, i + length(o)) } print r s }' "$file" > "$file.tmp"
	mv "$file.tmp" "$file"
}
target=libkern/crypto/sweet_bearssl
[[ ! -e $target ]] || { echo "refusing to overwrite existing AES source: $target" >&2; exit 1; }
mkdir -p "$target/src/symcipher"
cp -R "$bearssl_tmp/inc" "$target/inc"
for f in inner.h config.h; do cp -p "$bearssl_tmp/src/$f" "$target/src/$f"; done
replace "$target/src/inner.h" '#include <limits.h>' $'#if defined(KERNEL)\n#include <machine/limits.h>\n#else\n#include <limits.h>\n#endif' 1
for n in aes_ct aes_ct_enc aes_ct_dec aes_ct_cbcenc aes_ct_cbcdec; do cp -p "$bearssl_tmp/src/symcipher/$n.c" "$target/src/symcipher/$n.c"; done
for n in hash/md5.c codec/dec32le.c codec/enc32le.c hash/sha1.c codec/dec32be.c codec/enc32be.c hash/sha2big.c codec/dec64be.c codec/enc64be.c; do
	mkdir -p "$target/src/$(dirname "$n")"
	cp -p "$bearssl_tmp/src/$n" "$target/src/$n"
done
cp -p "$bearssl_tmp/LICENSE.txt" "$target/LICENSE.txt"
echo "Staged BearSSL portable AES, MD5, SHA-1 and SHA-384/512 ($bearssl_sha) for SWEET"
