// First 3D draw: one triangle in direct (sysmem) mode into the 64x64 RGBA8
// self-test buffer. Register values follow turnip's a6xx gen1 paths
// (tu_init_hw / tu6_init_static_regs, tu6_sysmem_render_begin, program,
// vertex input, viewport, rasterizer, tu6_draw_common), packed from the
// current mesa xml; plain PKT4 where turnip uses CP_REG_WRITE trackers.

use super::*;

#[derive(PartialEq, Eq)]
pub enum Outcome {
    Drawn,
    Wrong,
    Hung,
}

const CP_DRAW_INDX_OFFSET: u32 = 0x38;
const CP_LOAD_STATE6_GEOM: u32 = 0x32;
const CP_LOAD_STATE6_FRAG: u32 = 0x34;
const CP_SET_DRAW_STATE: u32 = 0x43;
const CP_SET_MARKER: u32 = 0x65;
const CP_SKIP_IB2_ENABLE_GLOBAL: u32 = 0x1d;
const CP_SET_VISIBILITY_OVERRIDE: u32 = 0x64;
const CP_SET_MODE: u32 = 0x63;
const RM6_DIRECT_RENDER: u32 = 1;
const SP_UPDATE_CNTL: u32 = 0xbb08;

// layout of the 3D page: VS, FS (128 B aligned), vertices, zeroed border colours
const VS_OFF: usize = D3_OFF;
const FS_OFF: usize = D3_OFF + 0x80;
const VB_OFF: usize = D3_OFF + 0x100;
const BORDER_OFF: usize = D3_OFF + 0x200;

const NOP: [u32; 2] = [0, 0];
const END: [u32; 2] = [0, 0x0300_0000];

// mov.f32f32 r0.<comp>, (imm)
const fn mov_imm(comp: u32, f: u32) -> [u32; 2] {
    [f, 0x2044_4000 | comp]
}

const ONE: u32 = 0x3f80_0000;
// red, opaque; WZYX puts R in the low byte
const WANT: u32 = 0xff00_00ff;
const POISON: u32 = 0x5a5a_5a5a;

// one-time static state not covered by A618_MAGIC / STATIC_CTX
const INIT: &[(u32, u32)] = &[
    (0xae04, 0),          // SP_NC_MODE_CNTL_2
    (0xae0f, 0x3f),       // SP_PERFCTR_SHADER_MASK
    (0xb605, 0x44),       // TPL1_UNKNOWN_B605
    (0xbe00, 0x80),       // HLSQ_UNKNOWN_BE00
    (0xbe01, 0),          // HLSQ_UNKNOWN_BE01
    (0xab20, 0),          // SP_GFX_USIZE
    (0xb182, 0),          // TPL1_PS_ROTATION_CNTL
    (0xbb11, 0),          // HLSQ_SHARED_CONSTS
    (0xa9a8, 0),          // SP_UNKNOWN_A9A8
    (0xab00, 5),          // SP_MODE_CNTL: const demotion, ISAMMODE_GL
    (0xa009, 3),          // VFD_MODE_CNTL
    (0x8818, 0),          // RB_UNKNOWN_8818..881E
    (0x8819, 0),
    (0x881a, 0),
    (0x881b, 0),
    (0x881c, 0),
    (0x881d, 0),
    (0x881e, 0),
    (0x88f0, 0),          // RB_UNKNOWN_88F0
    (0x9236, 0),          // VPC_REPLACE_MODE_CNTL
    (0x9300, 0),          // VPC_ROTATION_CNTL
    (0x9306, 1),          // VPC_SO_OVERRIDE: streamout off
    (0xb183, 0),          // TPL1_PS_SWIZZLE_CNTL
    (0x80af, 0),          // GRAS_SC_SCREEN_SCISSOR_CNTL
    (0x8099, 0),          // GRAS_SU_CONSERVATIVE_RAS_CNTL
    (0x980a, 0),          // PC_DGEN_SU_CONSERVATIVE_RAS_CNTL
    (0x9210, 0),          // VPC_UNKNOWN_9210/9211
    (0x9211, 0),
    (0x9602, 0),          // VPC_LB_MODE_CNTL
    (0x9e72, 0),          // PC_CONTEXT_SWITCH_GFX_PREEMPTION_MODE
    (0xb309, 0xa2),       // TPL1_MODE_CNTL
    (0xb986, 0xfcfc),     // SP_REG_PROG_ID_3
    (0xa007, 0),          // VFD_RENDER_MODE: rendering pass
    (0x8864, 0),          // RB_ALPHA_TEST_CNTL
    (0x8100, 0),          // GRAS_LRZ_CNTL
    (0x8898, 0),          // RB_LRZ_CNTL
];

// render pass, framebuffer, program and fixed function state of the draw
const STATE: &[(u32, u32)] = &[
    // sysmem render begin: window scissor/offsets, bin control = sysmem
    (0x80f0, 0),          // GRAS_SC_WINDOW_SCISSOR_TL
    (0x80f1, 0x003f_003f),
    (0x840a, 0),          // GRAS_A2D_SCISSOR_TL/BR
    (0x840b, 0x003f_003f),
    (0x8890, 0),          // RB_WINDOW_OFFSET
    (0x88d4, 0),          // RB_RESOLVE_WINDOW_OFFSET
    (0xb4d1, 0),          // SP_WINDOW_OFFSET
    (0xb307, 0),          // TPL1_WINDOW_OFFSET
    (0x80a1, 0x00e0_0000), // GRAS_SC_BIN_CNTL: buffers in sysmem, no LRZ write
    (0x8800, 0x00e0_0000), // RB_CNTL
    (0x88d3, 0),          // RB_RESOLVE_CNTL_3
    // no depth/stencil
    (0x8872, 0),          // RB_DEPTH_BUFFER_INFO .. BASE_GMEM
    (0x8873, 0),
    (0x8874, 0),
    (0x8875, 0),
    (0x8876, 0),
    (0x8877, 0),
    (0x8098, 0),          // GRAS_SU_DEPTH_BUFFER_INFO
    (0x8881, 0),          // RB_STENCIL_BUFFER_INFO
    // MRT0 format; base/pitch are written by triangle()
    (0xa996, 0x30),       // SP_PS_MRT[0].REG: FMT6_8_8_8_8_UNORM
    (0x8903, 0),          // RB_COLOR_FLAG_BUFFER[0]
    (0x8904, 0),
    (0x8905, 0),
    (0x8102, 0x30),       // GRAS_LRZ_MRT_BUFFER_INFO_0
    (0x880e, 0),          // RB_DITHER_CNTL
    (0x880f, 0),          // RB_SRGB_CNTL
    (0xa98a, 0),          // SP_SRGB_CNTL
    (0x8004, 0),          // GRAS_CL_ARRAY_SIZE
    (0x8801, 0x10),       // RB_RENDER_CNTL: CCUSINGLECACHELINESIZE 2
    // one sample
    (0xb300, 0),          // TPL1_RAS_MSAA_CNTL
    (0xb301, 4),          // TPL1_DEST_MSAA_CNTL: MSAA_DISABLE
    (0x80a2, 0),          // GRAS_SC_RAS_MSAA_CNTL
    (0x80a3, 4),          // GRAS_SC_DEST_MSAA_CNTL
    (0x8802, 0),          // RB_RAS_MSAA_CNTL
    (0x8803, 4),          // RB_DEST_MSAA_CNTL
    // program config (after SP_UPDATE_CNTL)
    (0xbb11, 0),          // HLSQ_SHARED_CONSTS
    (0xab00, 5),          // SP_MODE_CNTL
    (0xb800, 0x102),      // SP_VS_CONST_CONFIG: constlen 8, enabled
    (0xb801, 0),          // SP_HS/DS/GS_CONST_CONFIG
    (0xb802, 0),
    (0xb803, 0),
    (0xbb10, 0x100),      // SP_PS_CONST_CONFIG: enabled
    (0xa823, 0x100),      // SP_VS_CONFIG: enabled
    (0xa83b, 0),          // SP_HS/DS/GS_CONFIG
    (0xa863, 0),
    (0xa894, 0),
    (0xab04, 0x100),      // SP_PS_CONFIG: enabled
    // VS: full footprint 1 (r0), merged regs
    (0xa800, 0x0010_0080), // SP_VS_CNTL_0
    (0xa824, 1),          // SP_VS_INSTR_SIZE (x16 instructions)
    (0xa81b, 0),          // SP_VS_PROGRAM_COUNTER_OFFSET
    (0xa81e, 0),          // SP_VS_PVT_MEM_*
    (0xa81f, 0),
    (0xa820, 0),
    (0xa821, 0),
    (0xa825, 0),
    (0x9b07, 6),          // PC_STEREO_RENDERING_CNTL: 1 view, no multipos
    (0xa008, 6),          // VFD_STEREO_RENDERING_CNTL
    (0xa000, 0x101),      // VFD_CNTL_0: 1 fetch, 1 decode
    (0xa0d0, 0xf),        // VFD_DEST_CNTL[0]: r0.xyzw
    (0xa001, 0xfcfc_fcfc), // VFD_CNTL_1..5: no sysvals
    (0xa002, 0xfcfc),
    (0xa003, 0xfcfc_fcfc),
    (0xa004, 0xfc),
    (0xa005, 0xfcfc),
    // FS: THREAD64 (SP_PS_CNTL_0 and SP_PS_WAVE_CNTL agree), r0 output
    (0xa980, 0x8100_0080), // SP_PS_CNTL_0
    (0xab05, 1),          // SP_PS_INSTR_SIZE
    (0xa982, 0),          // SP_PS_PROGRAM_COUNTER_OFFSET
    (0xa985, 0),          // SP_PS_PVT_MEM_*
    (0xa986, 0),
    (0xa987, 0),
    (0xa988, 0),
    (0xa9a9, 0),
    (0xa006, 0),          // VFD_CNTL_6
    (0x9806, 0),          // PC_PS_CNTL
    (0xa99e, 8),          // SP_PS_INITIAL_TEX_LOAD_CNTL: IJ write disabled
    (0xb982, 7),          // SP_LB_PARAM_LIMIT
    (0xb983, 0xfcfc_fcfc), // SP_REG_PROG_ID_0..2: nothing
    (0xb984, 0xfcfc_fcfc),
    (0xb985, 0xfcfc_fcfc),
    (0xb980, 0),          // SP_PS_WAVE_CNTL: THREAD64, no varyings
    (0x8005, 0),          // GRAS_CL_INTERP_CNTL
    (0x8809, 0),          // RB_INTERP_CNTL
    (0x880a, 0),          // RB_PS_INPUT_CNTL
    (0x8810, 0),          // RB_PS_SAMPLEFREQ_CNTL
    (0x8101, 0),          // GRAS_LRZ_PS_INPUT_CNTL
    (0x8109, 0),          // GRAS_LRZ_PS_SAMPLEFREQ_CNTL
    (0x9212, 0xffff_ffff), // VPC_VARYING_LM_TRANSFER_CNTL[0..3].DISABLE
    (0x9213, 0xffff_ffff),
    (0x9214, 0xffff_ffff),
    (0x9215, 0xffff_ffff),
    (0x9304, 0xff00_ff00), // VPC_PS_CNTL
    (0xa98c, 0xfcfc_fc00), // SP_PS_OUTPUT_CNTL
    (0xa98e, 0),          // SP_PS_OUTPUT[0].REG: r0
    (0xa98b, 0xf),        // SP_PS_OUTPUT_MASK
    (0x880b, 0),          // RB_PS_OUTPUT_CNTL
    (0x880d, 0xf),        // RB_PS_OUTPUT_MASK
    // VPC: position from r0.xyzw at loc 0
    (0x9216, 0),          // VPC_SO_MAPPING_WPTR
    (0x9305, 0),          // VPC_SO_CNTL
    (0xa803, 0xf00),      // SP_VS_OUTPUT[0].REG
    (0xa813, 0),          // SP_VS_VPC_DEST[0].REG
    (0x9301, 0x00ff_0004), // VPC_VS_CNTL: stride 4, position loc 0
    (0x9101, 0x00ff_ff00), // VPC_VS_CLIP_CULL_CNTL
    (0x9311, 0x00ff_ff00), // VPC_VS_CLIP_CULL_CNTL_V2
    (0x8001, 0),          // GRAS_CL_VS_CLIP_CULL_DISTANCE
    (0x9b01, 4),          // PC_VS_CNTL
    (0xa802, 1),          // SP_VS_OUTPUT_CNTL
    (0x9104, 0x00ff_ffff), // VPC_VS_SIV_CNTL
    (0x9314, 0x00ff_ffff), // VPC_VS_SIV_CNTL_V2
    (0x809b, 0),          // GRAS_SU_VS_SIV_CNTL
    // vertex input: one vec4 f32 attribute (address written by triangle())
    (0xa090, 0xc820_0000), // VFD_FETCH_INSTR[0]: FMT6_32_32_32_32_FLOAT
    (0xa091, 0),
    (0xa013, 16),         // VFD_VERTEX_BUFFER[0].STRIDE
    (0xa012, 48),         // VFD_VERTEX_BUFFER[0].SIZE
    (0xa00e, 0),          // VFD_INDEX_OFFSET
    (0xa00f, 0),          // VFD_INSTANCE_START_OFFSET
    // viewport 64x64, depth 0..1
    (0x8010, 0x4200_0000), // GRAS_CL_VIEWPORT[0] X/Y offset+scale = 32
    (0x8011, 0x4200_0000),
    (0x8012, 0x4200_0000),
    (0x8013, 0x4200_0000),
    (0x8014, 0),
    (0x8015, ONE),
    (0x80d0, 0),          // GRAS_SC_VIEWPORT_SCISSOR[0]
    (0x80d1, 0x003f_003f),
    (0x8070, 0),          // GRAS_CL_VIEWPORT_ZCLAMP[0]
    (0x8071, ONE),
    (0x88c0, 0),          // RB_VIEWPORT_ZCLAMP_MIN/MAX
    (0x88c1, ONE),
    (0x8006, 0x0007_fdff), // GRAS_CL_GUARDBAND_CLIP_ADJ
    (0x80b0, 0),          // GRAS_SC_SCREEN_SCISSOR[0]
    (0x80b1, 0x003f_003f),
    // rasterizer, no cull, no depth/stencil/blend
    (0x8090, 0x2010),     // GRAS_SU_CNTL
    (0x8000, 0xc0),       // GRAS_CL_CNTL
    (0x9108, 3),          // VPC_RAST_CNTL: triangles
    (0x9981, 3),          // PC_DGEN_RAST_CNTL
    (0x9107, 0),          // VPC_RAST_STREAM_CNTL
    (0x9980, 0),          // PC_RAST_STREAM_CNTL
    (0x8091, 0xffc0_0001), // GRAS_SU_POINT_MINMAX
    (0x8092, 0x10),       // GRAS_SU_POINT_SIZE
    (0x8095, 0),          // GRAS_SU_POLY_OFFSET_*
    (0x8096, 0),
    (0x8097, 0),
    (0x80a0, 2),          // GRAS_SC_CNTL
    (0x80a4, 0),          // sample positions off
    (0x8804, 0),
    (0xb304, 0),
    (0x8880, 0),          // RB_STENCIL_CNTL
    (0x8115, 0),          // GRAS_SU_STENCIL_CNTL
    (0x8888, 0),          // RB_STENCIL_MASK
    (0x8889, 0),          // RB_STENCIL_WRITE_MASK
    (0x8887, 0),          // RB_STENCIL_REF_CNTL
    (0x8871, 0),          // RB_DEPTH_CNTL
    (0x8114, 0),          // GRAS_SU_DEPTH_CNTL
    (0x8878, 0),          // RB_DEPTH_BOUND_MIN/MAX
    (0x8879, 0),
    (0x8094, 0),          // GRAS_SU_DEPTH_PLANE_CNTL: early z
    (0x8870, 0),          // RB_DEPTH_PLANE_CNTL
    (0xa989, 0x100),      // SP_BLEND_CNTL
    (0x8865, 0xffff_0100), // RB_BLEND_CNTL
    (0x8820, 0x7e0),      // RB_MRT[0].CONTROL: RGBA enabled, ROP_COPY, no blend
    (0x8821, 0x0001_0001), // RB_MRT[0].BLEND_CONTROL
    (0xa98d, 1),          // SP_PS_MRT_CNTL
    (0x880c, 1),          // RB_PS_MRT_CNTL
    (0x8860, 0),          // RB_BLEND_CONSTANT_*
    (0x8861, 0),
    (0x8862, 0),
    (0x8863, 0),
    // per draw
    (0x9b00, 0),          // PC_CNTL
    (0x9803, 0xffff_ffff), // PC_RESTART_INDEX
    (0x9802, 0),          // PC_DS_PARAM
];

fn put(mem: &GpuMem, off: usize, words: &[[u32; 2]], total: usize) {
    for i in 0..total {
        let w = words.get(i).copied().unwrap_or(NOP);
        mem.w32(off + 8 * i, w[0]);
        mem.w32(off + 8 * i + 4, w[1]);
    }
}

// expected coverage of the triangle (0,0) (64,0) (32,64) at pixel centres;
// None for pixels within half a pixel of an edge (rasterisation rule)
fn inside(x: u32, y: u32) -> Option<bool> {
    // quarter-pixel units (the kernel has no float support): centre (4x+2, 4y+2),
    // left edge x = y / 2, right edge x = 64 - y / 2
    let (px, py) = (4 * x as i32 + 2, 4 * y as i32 + 2);
    let d = (px - py / 2).min(256 - py / 2 - px);
    if d > 3 {
        Some(true)
    } else if d < -3 {
        Some(false)
    } else {
        None
    }
}

pub fn triangle(gpu: &Gpu, mem: &GpuMem, ring: &mut Ring) -> Outcome {
    for i in 0..DST_BYTES / 4 {
        mem.w32(DST_OFF + 4 * i, POISON);
    }
    for i in 0..D3_BYTES / 4 {
        mem.w32(D3_OFF + 4 * i, 0);
    }
    put(mem, VS_OFF, &[END], 16);
    put(mem, FS_OFF, &[mov_imm(0, ONE), mov_imm(1, 0), mov_imm(2, 0), mov_imm(3, ONE), END], 16);
    // (-1,-1,0,1) (1,-1,0,1) (0,1,0,1) as f32 bits
    const M1: u32 = 0xbf80_0000;
    let verts = [M1, M1, 0, ONE, ONE, M1, 0, ONE, 0, ONE, 0, ONE];
    for (i, v) in verts.iter().enumerate() {
        mem.w32(VB_OFF + 4 * i, *v);
    }
    mem.dma.clean();

    let ts = mem.iova(CCU_TS_OFF);
    let (rt, vs, fs, vb, border) =
        (mem.iova(DST_OFF), mem.iova(VS_OFF), mem.iova(FS_OFF), mem.iova(VB_OFF), mem.iova(BORDER_OFF));
    let lohi = |a: u64| [a as u32, (a >> 32) as u32];

    // tu_init_hw()
    ring.event(CACHE_INVALIDATE, None);
    ring.reg(SP_UPDATE_CNTL, &[0x000f_ffff]);
    ring.wfi();
    for (r, v) in INIT {
        ring.reg(*r, &[*v]);
    }
    ring.reg(0xb302, &lohi(border)); // TPL1_GFX_BORDER_COLOR_BASE
    ring.reg(0xb180, &lohi(border)); // TPL1_CS_BORDER_COLOR_BASE
    ring.emit(pkt7(CP_SET_DRAW_STATE, 3));
    for v in [1 << 18, 0, 0] {
        ring.emit(v); // DISABLE_ALL_GROUPS
    }

    // CCU in sysmem mode (same value as the 2D path)
    ring.event(PC_CCU_FLUSH_COLOR_TS, Some((ts, 0x10)));
    ring.event(PC_CCU_FLUSH_DEPTH_TS, Some((ts, 0x11)));
    ring.event(PC_CCU_INVALIDATE_COLOR, None);
    ring.event(PC_CCU_INVALIDATE_DEPTH, None);
    ring.wfi();
    ring.reg(RB_CCU_CNTL, &[CCU_CNTL_BYPASS]);

    ring.reg(SP_UPDATE_CNTL, &[0x19f]);
    for (r, v) in STATE {
        ring.reg(*r, &[*v]);
    }
    // MRT0: linear RGBA8, pitch 256 B, then MRT1..7 off
    let [lo, hi] = lohi(rt);
    ring.reg(0x8822, &[0x30, DST_PITCH >> 6, (DST_PITCH * DST_H) >> 6, lo, hi, 0]);
    for i in 1..8u32 {
        ring.reg(0x8822 + 8 * i, &[0, 0, 0, 0, 0, 0]);
        ring.reg(0xa996 + i, &[0]);
    }
    ring.reg(0xa81c, &lohi(vs)); // SP_VS_BASE
    ring.reg(0xa983, &lohi(fs)); // SP_PS_BASE
    ring.reg(0xa010, &lohi(vb)); // VFD_VERTEX_BUFFER[0].BASE

    ring.emit(pkt7(CP_SET_MARKER, 1));
    ring.emit(RM6_DIRECT_RENDER);
    ring.emit(pkt7(CP_SKIP_IB2_ENABLE_GLOBAL, 1));
    ring.emit(0);
    ring.emit(pkt7(CP_SET_VISIBILITY_OVERRIDE, 1));
    ring.emit(1);
    ring.emit(pkt7(CP_SET_MODE, 1));
    ring.emit(0);

    // instruction cache preload: ST6_SHADER, SS6_INDIRECT, SB6_VS/FS_SHADER, 1 unit
    ring.emit(pkt7(CP_LOAD_STATE6_GEOM, 3));
    ring.emit((2 << 16) | (8 << 18) | (1 << 22));
    ring.emit(vs as u32);
    ring.emit((vs >> 32) as u32);
    ring.emit(pkt7(CP_LOAD_STATE6_FRAG, 3));
    ring.emit((2 << 16) | (0xc << 18) | (1 << 22));
    ring.emit(fs as u32);
    ring.emit((fs >> 32) as u32);

    // DI_PT_TRILIST, AUTO_INDEX, USE_VISIBILITY; 1 instance, 3 vertices
    ring.emit(pkt7(CP_DRAW_INDX_OFFSET, 3));
    ring.emit(0x184);
    ring.emit(1);
    ring.emit(3);

    ring.event(PC_CCU_FLUSH_COLOR_TS, Some((ts, 0x12)));
    ring.event(PC_CCU_INVALIDATE_COLOR, None);
    ring.wfi();
    let fseq = fence_read(mem) + 1;
    ring.fence(fseq);
    let ok = ring.submit(gpu, "triangle");
    let f = fence_read(mem);
    mem.dma.flush(TABLES + DST_OFF, DST_BYTES);

    let (mut tin, mut tout, mut win, mut wout, mut edge_red) = (0u32, 0u32, 0u32, 0u32, 0u32);
    let mut first_bad = None;
    let (mut other, mut flipped) = (0u32, 0u32);
    for y in 0..DST_H {
        for x in 0..DST_W {
            let v = mem.r32(DST_OFF + (y * DST_PITCH + 4 * x) as usize);
            if inside(x, DST_H - 1 - y) == Some(true) && v == WANT {
                flipped += 1;
            }
            match inside(x, y) {
                Some(true) => {
                    win += 1;
                    if v == WANT {
                        tin += 1;
                    } else if first_bad.is_none() {
                        first_bad = Some((x, y, v));
                    }
                }
                Some(false) => {
                    wout += 1;
                    if v == POISON {
                        tout += 1;
                    } else if first_bad.is_none() {
                        first_bad = Some((x, y, v));
                    }
                }
                None => edge_red += (v == WANT) as u32,
            }
            if v != WANT && v != POISON {
                other += 1;
            }
        }
    }
    crate::log!(
        "SWEET CP: triangle fence {f} (want {fseq}); inside red {tin}/{win}, outside untouched {tout}/{wout}, edge red {edge_red}, other values {other}, red as if y-flipped {flipped}; px(32,16) {:#010x} px(32,48) {:#010x} px(2,60) {:#010x}",
        mem.r32(DST_OFF + (16 * DST_PITCH + 4 * 32) as usize),
        mem.r32(DST_OFF + (48 * DST_PITCH + 4 * 32) as usize),
        mem.r32(DST_OFF + (60 * DST_PITCH + 4 * 2) as usize)
    );
    if let Some((x, y, v)) = first_bad {
        crate::log!("SWEET CP: triangle first mismatch at ({x},{y}) = {v:#010x}");
    }
    if !ok || f != fseq {
        crate::log!(
            "SWEET CP: triangle did not finish: rptr {} wptr {} status {:#010x} int0 {:#010x} hw_fault {:#x}",
            gpu.read(CP_RB_RPTR),
            ring.wptr,
            gpu.read(RBBM_STATUS),
            gpu.read(RBBM_INT_0_STATUS),
            gpu.read(CP_HW_FAULT)
        );
        smmu_fault();
        return if f == fseq { Outcome::Wrong } else { Outcome::Hung };
    }
    if tin == win && tout == wout {
        crate::log!("SWEET CP: 3D TRIANGLE OK (64x64 rgba8, {WANT:#010x})");
        Outcome::Drawn
    } else {
        // a flipped triangle (viewport y) shows up as inside 0 / outside mostly wrong
        Outcome::Wrong
    }
}
