#!/bin/sh
# runs inside debian:bookworm with this directory mounted at /w:
# builds mesa's freedreno ir3 library + the ir3 assembler/disassembler and the
# skyios ir3c tool (SPIR-V -> ir3) that we add to the tree before meson setup.
# usage: docker run --rm -v "$PWD":/w debian:bookworm sh /w/build2.sh
set -e
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq git ca-certificates build-essential python3-pip python3-mako python3-yaml \
  python3-packaging ninja-build bison flex pkg-config libexpat1-dev zlib1g-dev libzstd-dev \
  libdrm-dev spirv-tools glslang-tools >/dev/null
pip3 install --break-system-packages -q 'meson>=1.4' pycparser
git config --global --add safe.directory '*'
cd /w
if [ ! -d mesa-src ]; then
  git clone -q --depth 1 https://gitlab.freedesktop.org/mesa/mesa.git mesa-src
fi
cd mesa-src
git log --oneline -1
# our IR3 compiler driver: SPIR-V -> NIR -> ir3 (see ir3c.c in this directory)
cp /w/ir3c.c src/freedreno/ir3/sky_ir3c.c
if ! grep -q sky_ir3c src/freedreno/ir3/meson.build; then
  cat >> src/freedreno/ir3/meson.build <<'EOF'

# skyios: standalone SPIR-V -> ir3 compiler (host tool, no GPU needed)
sky_ir3c = executable(
  'ir3c',
  'sky_ir3c.c',
  link_with: libfreedreno_ir3,
  dependencies: [idep_libir3decode, idep_mesautil, idep_nir, idep_vtn, dep_dl, dep_unwind],
  include_directories: [inc_freedreno, inc_include, inc_src],
  build_by_default: true,
)
EOF
fi
: # keep /w/build2 between runs (incremental)
meson setup /w/build2 -Dgallium-drivers=freedreno -Dvulkan-drivers= -Dplatforms= -Dglx=disabled \
  -Degl=disabled -Dgbm=disabled -Dllvm=disabled -Dtools=freedreno -Dbuildtype=release \
  >/w/meson2.log 2>&1 || { tail -40 /w/meson2.log; exit 1; }
ninja -C /w/build2 src/freedreno/ir3/ir3c src/freedreno/isa/ir3-disasm >/w/ninja2.log 2>&1 || {
  tail -60 /w/ninja2.log; exit 1; }
ls -la /w/build2/src/freedreno/ir3/ir3c /w/build2/src/freedreno/isa/ir3-disasm

# smoke tests: GLSL -> SPIR-V (glslang) -> ir3 (ir3c)
for t in red.frag fbfetch.frag; do
  s=${t%.frag}
  glslangValidator -G --target-env vulkan1.1 -S frag -o /w/build2/$s.spv /w/tests/$t || exit 1
  /w/build2/src/freedreno/ir3/ir3c -s fs -o /w/build2/$s.ir3 /w/build2/$s.spv || exit 1
done
echo "=== ir3 smoke tests ok ==="
