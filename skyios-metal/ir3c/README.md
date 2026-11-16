# ir3c — SPIR-V to Adreno ir3 machine code compiler

Host-side tool that translates SPIR-V shaders into Adreno 618 (a6xx gen1) ir3
machine code, using mesa's freedreno ir3 backend. The produced binaries are
loaded by the SkyAdrenoMetal bundle on the phone.

## Build

Requires Docker (builds inside `debian:bookworm` with mesa from source):

```sh
cd research/mesa-build   # or wherever build-mesa.sh lives
docker run --rm -v "$PWD":/w debian:bookworm sh /w/build-mesa.sh
# produces build2/src/freedreno/ir3/ir3c (Linux aarch64)
```

## Usage

```sh
# compile a GLSL fragment shader via glslang → SPIR-V → ir3
glslangValidator -G --target-env vulkan1.1 -S frag -o red.spv tests/red.frag
ir3c -s fs -o red.ir3 red.spv
```

## Validation

The red constant-colour fragment shader (`tests/red.frag`) produces the same
10 instruction dwords as the hand-assembled `sky_red_fragment` in
`skyios-metal/bundle/SkyRender.m`, which runs pixel-exact on the phone
(boot 0112k).

The pass-through vertex shader (`tests/pass.vert`) produces a single `end`
instruction, matching the hand-assembled `sky_position_vertex`.
