#!/bin/bash
set -euo pipefail

[[ "${MACHINE_CONFIG:-}" == SWEET ]] || exit 0
[[ "${MACOS_VERSION:-}" == 13.5 ]] || {
    echo 'SWEET libpthread integration requires xnu-8796.141.3 (macOS 13.5)' >&2
    exit 1
}
[[ -f bsd/pthread/pthread_shims.c ]] || exit 1

pthread_sha=a6c70ca0e544347a0377babf748f831577ec24f9
pthread_tmp=$(mktemp -d)
trap 'rm -rf "$pthread_tmp"' EXIT
git init -q "$pthread_tmp"
git -C "$pthread_tmp" fetch -q --depth 1 \
    https://github.com/apple-oss-distributions/libpthread "$pthread_sha"
git -C "$pthread_tmp" checkout -q FETCH_HEAD
[[ "$(git -C "$pthread_tmp" rev-parse HEAD)" == "$pthread_sha" ]]

replace() {
	local file=$1 old=$2 new=$3 count=$4 n
	n=$(OLD="$old" awk 'BEGIN { RS = "\001" } { s = $0; o = ENVIRON["OLD"]; c = 0; while ((i = index(s, o)) > 0) { c++; s = substr(s, i + length(o)) } print c }' "$file")
	[[ "$n" == "$count" ]] || { echo "source changed: $file: $old" >&2; exit 1; }
	OLD="$old" NEW="$new" awk 'BEGIN { RS = "\001"; ORS = "" } { s = $0; o = ENVIRON["OLD"]; r = ""; while ((i = index(s, o)) > 0) { r = r substr(s, 1, i - 1) ENVIRON["NEW"]; s = substr(s, i + length(o)) } print r s }' "$file" > "$file.tmp"
	mv "$file.tmp" "$file"
}

target=bsd/pthread/sweet_libpthread
[[ ! -e $target ]] || { echo "refusing to overwrite existing provider: $target" >&2; exit 1; }
mkdir -p "$target"
for d in kern private include; do cp -R "$pthread_tmp/$d" "$target/$d"; done
k=$target/kern
replace "$k/kern_trace.h" 'vm_kernel_unslide_or_perm_external(ptr,' 'vm_kernel_unslide_or_perm_external((vm_offset_t)ptr,' 1
replace "$k/kern_synch.c" 'pthread_kern->psynch_wait_update_owner(kwq,' 'pthread_kern->psynch_wait_update_owner((uintptr_t)kwq,' 4
replace "$k/kern_synch.c" 'pthread_kern->psynch_wait_complete(kwq,' 'pthread_kern->psynch_wait_complete((uintptr_t)kwq,' 1
replace "$k/kern_synch.c" 'pthread_kern->psynch_wait_wakeup(kwq,' 'pthread_kern->psynch_wait_wakeup((uintptr_t)kwq,' 2
replace "$k/kern_support.c" '*kevent_list_out = NULL;' '*kevent_list_out = 0;' 1
replace "$k/kern_support.c" 'user_addr_t kevent_list = NULL;' 'user_addr_t kevent_list = 0;' 2
replace "$k/kern_support.c" 'workq_kevent(p, &th_addrs, NULL,' 'workq_kevent(p, &th_addrs, 0,' 1
replace "$k/kern_support.c" 'panic(__func__ ": thread_set_wq_state failed: %d", error);' 'panic("%s: thread_set_wq_state failed: %d", __func__, error);' 4

pthread_sdk=$(xcrun --sdk macosx --show-sdk-path)
cp "$pthread_sdk/usr/include/TargetConditionals.h" bsd/pthread/sweet_libpthread/
echo "Staged libpthread-514.100.2 ($pthread_sha) for SWEET"
