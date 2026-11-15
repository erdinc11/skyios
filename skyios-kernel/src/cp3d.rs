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
const FS_TEX_OFF: usize = D3_OFF + 0x300;
const VB_TEX_OFF: usize = D3_OFF + 0x380;
const DESC_OFF: usize = D3_OFF + 0x400;
const TEX_OFF: usize = D3_OFF + PAGE;

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

fn lohi(a: u64) -> [u32; 2] {
    [a as u32, (a >> 32) as u32]
}

// tu_init_hw() static state, then the CCU in sysmem mode (same value as the 2D path)
fn emit_init(ring: &mut Ring, mem: &GpuMem) {
    let ts = mem.iova(CCU_TS_OFF);
    ring.event(CACHE_INVALIDATE, None);
    ring.reg(SP_UPDATE_CNTL, &[0x000f_ffff]);
    ring.wfi();
    for (r, v) in INIT {
        ring.reg(*r, &[*v]);
    }
    let border = mem.iova(BORDER_OFF);
    ring.reg(0xb302, &lohi(border)); // TPL1_GFX_BORDER_COLOR_BASE
    ring.reg(0xb180, &lohi(border)); // TPL1_CS_BORDER_COLOR_BASE
    ring.emit(pkt7(CP_SET_DRAW_STATE, 3));
    for v in [1 << 18, 0, 0] {
        ring.emit(v); // DISABLE_ALL_GROUPS
    }
    ring.event(PC_CCU_FLUSH_COLOR_TS, Some((ts, 0x10)));
    ring.event(PC_CCU_FLUSH_DEPTH_TS, Some((ts, 0x11)));
    ring.event(PC_CCU_INVALIDATE_COLOR, None);
    ring.event(PC_CCU_INVALIDATE_DEPTH, None);
    ring.wfi();
    ring.reg(RB_CCU_CNTL, &[CCU_CNTL_BYPASS]);
}

pub struct Target {
    pub base: u64,
    pub pitch: u32,
    // RB_MRT[0].BUF_INFO: format | swap << 13
    pub info: u32,
}

pub struct Draw<'a> {
    // written after STATE, so these win
    pub overlay: &'a [(u32, u32)],
    pub vs: u64,
    pub fs: u64,
    pub vb: u64,
    // texture descriptor (16 dwords) + sampler (4 dwords) at +64
    pub desc: Option<u64>,
    pub initiator: u32,
    pub verts: u32,
}

// program + fixed function state of one draw into `t`
fn emit_state(ring: &mut Ring, t: &Target, d: &Draw) {
    ring.reg(SP_UPDATE_CNTL, &[0x19f]);
    for (r, v) in STATE.iter().chain(d.overlay) {
        ring.reg(*r, &[*v]);
    }
    let [lo, hi] = lohi(t.base);
    ring.reg(0x8822, &[t.info, t.pitch >> 6, 0, lo, hi, 0]);
    ring.reg(0xa996, &[t.info & 0xff]); // SP_PS_MRT[0].REG: format only
    for i in 1..8u32 {
        ring.reg(0x8822 + 8 * i, &[0, 0, 0, 0, 0, 0]);
        ring.reg(0xa996 + i, &[0]);
    }
    ring.reg(0xa81c, &lohi(d.vs)); // SP_VS_BASE
    ring.reg(0xa983, &lohi(d.fs)); // SP_PS_BASE
    ring.reg(0xa010, &lohi(d.vb)); // VFD_VERTEX_BUFFER[0].BASE

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
    ring.emit(d.vs as u32);
    ring.emit((d.vs >> 32) as u32);
    ring.emit(pkt7(CP_LOAD_STATE6_FRAG, 3));
    ring.emit((2 << 16) | (0xc << 18) | (1 << 22));
    ring.emit(d.fs as u32);
    ring.emit((d.fs >> 32) as u32);

    if let Some(desc) = d.desc {
        // samplers: ST6_SHADER, SB6_FS_TEX; textures: ST6_CONSTANTS, SB6_FS_TEX
        let samp = desc + 64;
        ring.emit(pkt7(CP_LOAD_STATE6_FRAG, 3));
        ring.emit((2 << 16) | (4 << 18) | (1 << 22));
        ring.emit(samp as u32);
        ring.emit((samp >> 32) as u32);
        ring.reg(0xa9e0, &lohi(samp)); // SP_PS_SAMPLER_BASE
        ring.emit(pkt7(CP_LOAD_STATE6_FRAG, 3));
        ring.emit((1 << 14) | (2 << 16) | (4 << 18) | (1 << 22));
        ring.emit(desc as u32);
        ring.emit((desc >> 32) as u32);
        ring.reg(0xa9e4, &lohi(desc)); // SP_PS_TEXMEMOBJ_BASE
        ring.reg(0xa9a7, &[1]); // SP_PS_TSIZE
    }
}

fn emit_draw(ring: &mut Ring, d: &Draw) {
    ring.emit(pkt7(CP_DRAW_INDX_OFFSET, 3));
    ring.emit(d.initiator);
    ring.emit(1);
    ring.emit(d.verts);
}

// CCU colour to memory, then the fence
fn emit_finish(ring: &mut Ring, mem: &GpuMem) -> u32 {
    let ts = mem.iova(CCU_TS_OFF);
    ring.event(PC_CCU_FLUSH_COLOR_TS, Some((ts, 0x12)));
    ring.event(PC_CCU_INVALIDATE_COLOR, None);
    ring.wfi();
    let fseq = fence_read(mem) + 1;
    ring.fence(fseq);
    fseq
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

    emit_init(ring, mem);
    let t = Target { base: mem.iova(DST_OFF), pitch: DST_PITCH, info: 0x30 };
    // DI_PT_TRILIST, AUTO_INDEX, USE_VISIBILITY; 3 vertices
    let d = Draw {
        overlay: &[],
        vs: mem.iova(VS_OFF),
        fs: mem.iova(FS_OFF),
        vb: mem.iova(VB_OFF),
        desc: None,
        initiator: 0x184,
        verts: 3,
    };
    emit_state(ring, &t, &d);
    emit_draw(ring, &d);
    let fseq = emit_finish(ring, mem);
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

// Textured quad (turnip's r3d blit shape): vertex = pos vec4 + uv vec2,
// VS still `end` (r0 = pos, r1.xy = uv straight out), FS = a6xx texture
// prefetch of r1 at the interpolated uv plus the dummy `bary.f (ei)` ir3
// inserts and `(sy)end`. Linear BGRA8 texture, bilinear, clamp to edge.
const TEXQ: &[(u32, u32)] = &[
    (0xa000, 0x202),      // VFD_CNTL_0: 2 fetches, 2 decodes
    (0xa0d1, 0x43),       // VFD_DEST_CNTL[1]: r1.xy
    (0xa092, 0xc670_0200), // VFD_FETCH_INSTR[1]: FMT6_32_32_FLOAT at +16
    (0xa093, 0),
    (0xa013, 24),         // VFD_VERTEX_BUFFER[0].STRIDE
    (0xa012, 96),         // VFD_VERTEX_BUFFER[0].SIZE
    (0xa800, 0x0010_0100), // SP_VS_CNTL_0: footprint 2
    (0xa803, 0x0f00_0304), // SP_VS_OUTPUT[0]: uv r1.xy, pos r0.xyzw
    (0xa813, 0x200),      // SP_VS_VPC_DEST[0]: uv loc 0, pos loc 2
    (0x9301, 0x00ff_0206), // VPC_VS_CNTL: stride 6, position loc 2
    (0x9b01, 6),          // PC_VS_CNTL
    (0xa802, 2),          // SP_VS_OUTPUT_CNTL
    (0x9200, 0),          // VPC_VARYING_INTERP_MODE[0..7]: smooth
    (0x9201, 0),
    (0x9202, 0),
    (0x9203, 0),
    (0x9204, 0),
    (0x9205, 0),
    (0x9206, 0),
    (0x9207, 0),
    (0x9208, 0),          // VPC_VARYING_REPLACE_MODE[0..7]
    (0x9209, 0),
    (0x920a, 0),
    (0x920b, 0),
    (0x920c, 0),
    (0x920d, 0),
    (0x920e, 0),
    (0x920f, 0),
    (0xa980, 0x8540_0100), // SP_PS_CNTL_0: footprint 2, VARYING, PIXLODENABLE
    (0xb980, 2),          // SP_PS_WAVE_CNTL: THREAD64, VARYINGS
    (0xab04, 0x0002_0300), // SP_PS_CONFIG: 1 tex, 1 sampler
    (0xa99e, 0x11),       // SP_PS_INITIAL_TEX_LOAD_CNTL: 1 prefetch, ENDOFQUAD
    (0xa99f, 0x23c4_0000), // ..._CMD[0]: SAM loc 0 -> r1.xyzw, s#0 t#0
    (0xa9a3, 0),          // SP_PS_INITIAL_TEX_INDEX[0]
    (0xb984, 0xfcfc_fc00), // SP_REG_PROG_ID_1: IJ_PERSP_PIXEL in r0.xy
    (0x8005, 1),          // GRAS_CL_INTERP_CNTL: IJ_PERSP_PIXEL
    (0x8809, 0x401),      // RB_INTERP_CNTL: IJ_PERSP_PIXEL, INTERP_EN
    (0x9212, 0xffff_fffc), // VPC_VARYING_LM_TRANSFER_CNTL[0]: locs 0,1
    (0x9304, 0xff01_ff02), // VPC_PS_CNTL: 2 varyings
    (0xa98e, 4),          // SP_PS_OUTPUT[0].REG: r1
];

const BARY_EI_R63: [u32; 2] = [0x0000_2000, 0x4730_80fc];
const SY_END: [u32; 2] = [0, 0x1300_0000];

// A6XX_TEX_MEMOBJ for a linear BGRA8 ('BGRA' IOSurface) image + sampler
fn tex_desc(mem: &GpuMem, off: usize, base: u64, w: u32, h: u32, pitch: u32) {
    // FMT6_8_8_8_8_UNORM, swap WXYZ, identity swizzle, linear
    let d = [
        0x4c00_6880,
        w | (h << 15),
        2 | (pitch << 7) | (1 << 29),
        0,
        base as u32,
        (base >> 32) as u32 | (1 << 17),
    ];
    for i in 0..16 {
        mem.w32(off + 4 * i, d.get(i).copied().unwrap_or(0));
    }
    // bilinear, clamp to edge, no mips
    for (i, v) in [0x92a, 0x40, 0, 0].iter().enumerate() {
        mem.w32(off + 64 + 4 * i, *v);
    }
}

fn texel(x: u32, y: u32) -> u32 {
    // memory order B, G, R, A
    let (b, g, r, a) = (4 * x, 4 * y, (4 * (x ^ y)) & 0xfc, 0x80 + x);
    b | (g << 8) | (r << 16) | (a << 24)
}

pub fn texquad(gpu: &Gpu, mem: &GpuMem, ring: &mut Ring) -> Outcome {
    for i in 0..DST_BYTES / 4 {
        mem.w32(DST_OFF + 4 * i, POISON);
    }
    for y in 0..DST_H {
        for x in 0..DST_W {
            mem.w32(TEX_OFF + (y * DST_PITCH + 4 * x) as usize, texel(x, y));
        }
    }
    put(mem, FS_TEX_OFF, &[BARY_EI_R63, SY_END], 16);
    const M1: u32 = 0xbf80_0000;
    // tristrip (x, y, z, w, u, v)
    let verts = [
        M1, M1, 0, ONE, 0, 0, //
        ONE, M1, 0, ONE, ONE, 0, //
        M1, ONE, 0, ONE, 0, ONE, //
        ONE, ONE, 0, ONE, ONE, ONE,
    ];
    for (i, v) in verts.iter().enumerate() {
        mem.w32(VB_TEX_OFF + 4 * i, *v);
    }
    tex_desc(mem, DESC_OFF, mem.iova(TEX_OFF), DST_W, DST_H, DST_PITCH);
    mem.dma.clean();

    emit_init(ring, mem);
    let t = Target { base: mem.iova(DST_OFF), pitch: DST_PITCH, info: 0x30 };
    let d = Draw {
        overlay: TEXQ,
        vs: mem.iova(VS_OFF),
        fs: mem.iova(FS_TEX_OFF),
        vb: mem.iova(VB_TEX_OFF),
        desc: Some(mem.iova(DESC_OFF)),
        initiator: 0x186, // DI_PT_TRISTRIP
        verts: 4,
    };
    emit_state(ring, &t, &d);
    emit_draw(ring, &d);
    let fseq = emit_finish(ring, mem);
    let ok = ring.submit(gpu, "textured quad");
    let f = fence_read(mem);
    mem.dma.flush(TABLES + DST_OFF, DST_BYTES);

    let (mut exact, mut near, mut poison) = (0u32, 0u32, 0u32);
    let mut first_bad = None;
    for y in 0..DST_H {
        for x in 0..DST_W {
            let v = mem.r32(DST_OFF + (y * DST_PITCH + 4 * x) as usize);
            let t = texel(x, y);
            // the render target is RGBA in memory: B and R trade places
            let want = (t & 0xff00_ff00) | ((t & 0xff) << 16) | ((t >> 16) & 0xff);
            let close = (0..4).all(|c| ((v >> (8 * c)) & 0xff).abs_diff((want >> (8 * c)) & 0xff) <= 1);
            if v == want {
                exact += 1;
            } else if close {
                near += 1;
            } else {
                poison += (v == POISON) as u32;
                if first_bad.is_none() {
                    first_bad = Some((x, y, v, want));
                }
            }
        }
    }
    let n = DST_W * DST_H;
    crate::log!(
        "SWEET CP: textured quad fence {f} (want {fseq}); exact {exact}/{n}, within 1 {near}, untouched {poison}; px(0,0) {:#010x} px(33,17) {:#010x} (want {:#010x})",
        mem.r32(DST_OFF),
        mem.r32(DST_OFF + (17 * DST_PITCH + 4 * 33) as usize),
        {
            let t = texel(33, 17);
            (t & 0xff00_ff00) | ((t & 0xff) << 16) | ((t >> 16) & 0xff)
        }
    );
    if let Some((x, y, v, w)) = first_bad {
        crate::log!("SWEET CP: textured quad first mismatch at ({x},{y}) = {v:#010x} want {w:#010x}");
    }
    if !ok || f != fseq {
        crate::log!(
            "SWEET CP: textured quad did not finish: rptr {} wptr {} status {:#010x} int0 {:#010x} hw_fault {:#x}",
            gpu.read(CP_RB_RPTR),
            ring.wptr,
            gpu.read(RBBM_STATUS),
            gpu.read(RBBM_INT_0_STATUS),
            gpu.read(CP_HW_FAULT)
        );
        smmu_fault();
        return if f == fseq { Outcome::Wrong } else { Outcome::Hung };
    }
    if exact + near == n {
        crate::log!("SWEET CP: 3D TEXTURED QUAD OK (64x64 BGRA8 texture, bilinear, {exact} exact)");
        Outcome::Drawn
    } else {
        Outcome::Wrong
    }
}

// n / d as f32 bits (truncated, |n / d| < 2^24), without float or 128-bit
// division support in the kernel: 40 fractional bits by long division
fn ratio_f32(n: i64, d: i64) -> u32 {
    if n == 0 || d == 0 {
        return 0;
    }
    let sign = if (n < 0) != (d < 0) { 1u32 << 31 } else { 0 };
    let (n, d) = (n.unsigned_abs(), d.unsigned_abs());
    let (int, mut rem) = (n / d, n % d);
    let mut v = int << 40;
    for i in (0..40).rev() {
        rem <<= 1;
        if rem >= d {
            rem -= d;
            v |= 1 << i;
        }
    }
    if v == 0 {
        return 0;
    }
    let p = 63 - v.leading_zeros() as i32; // value = v * 2^-40
    let mant = if p >= 23 { (v >> (p - 23)) as u32 } else { (v << (23 - p)) as u32 } & 0x7f_ffff;
    sign | (((p - 40 + 127) as u32) << 23) | mant
}

// Present composition: the whole sw x sh BGRA surface scaled onto the dw x dh
// BGRA panel (pitch dpitch). Panel strides that are not 64-byte multiples
// (4320) are drawn as two passes of every other row with a doubled pitch;
// the odd pass starts 64-byte aligned below its row and is shifted right
// (scissored) by the remainder, and each pass offsets v by half a panel row
// so pixel centres sample their own rows. Emits commands only.
pub struct Compose {
    pub panel: u64,
    pub dpitch: u32,
    pub dw: u32,
    pub dh: u32,
    pub tex: u64,
    pub sw: u32,
    pub sh: u32,
    pub spitch: u32,
}

pub const COMPOSE_VB_OFF: usize = D3_OFF + 0x500;
pub const COMPOSE_DESC_OFF: usize = D3_OFF + 0x600;

pub fn compose_check(c: &Compose) -> bool {
    let step = if c.dpitch % 64 == 0 { 1 } else { 2 };
    c.tex % 64 == 0
        && c.spitch % 64 == 0
        && (step * c.dpitch) % 64 == 0
        && c.dw + 16 <= 0x4000
        && c.dh <= 0x4000
        && c.sw < 0x8000
        && c.sh < 0x8000
        && (c.dpitch % 64) % 4 == 0
}

pub fn compose(ring: &mut Ring, mem: &GpuMem, c: &Compose) {
    let step = if c.dpitch % 64 == 0 { 1u32 } else { 2 };
    let rows = c.dh / step;
    tex_desc(mem, COMPOSE_DESC_OFF, c.tex, c.sw, c.sh, c.spitch);
    const M1: u32 = 0xbf80_0000;
    for k in 0..step {
        // v of the quad's top and bottom edge: (k - 0.5 * (step - 1)) / dh offset
        let off = 2 * k as i64 - (step as i64 - 1); // in half rows: -1, +1 or 0
        let v0 = ratio_f32(off, 2 * c.dh as i64);
        let v1 = ratio_f32(2 * c.dh as i64 + off, 2 * c.dh as i64);
        let verts = [M1, M1, 0, ONE, 0, v0, ONE, M1, 0, ONE, ONE, v0, M1, ONE, 0, ONE, 0, v1, ONE, ONE, 0, ONE, ONE, v1];
        for (i, v) in verts.iter().enumerate() {
            mem.w32(COMPOSE_VB_OFF + 0x80 * k as usize + 4 * i, *v);
        }
    }
    mem.dma.clean();

    emit_init(ring, mem);
    for k in 0..step {
        let start = c.panel + k as u64 * c.dpitch as u64;
        let base = start & !63;
        let xs = ((start & 63) / 4) as u32;
        let t = Target { base, pitch: step * c.dpitch, info: 0x30 | (1 << 13) };
        let (x1, y1) = (xs + c.dw - 1, rows - 1);
        let half_w = ratio_f32(c.dw as i64, 2);
        let half_h = ratio_f32(rows as i64, 2);
        let pass: [(u32, u32); 14] = [
            (0x80f0, xs),        // GRAS_SC_WINDOW_SCISSOR
            (0x80f1, x1 | (y1 << 16)),
            (0x840a, xs),        // GRAS_A2D_SCISSOR
            (0x840b, x1 | (y1 << 16)),
            (0x80b0, xs),        // GRAS_SC_SCREEN_SCISSOR[0]
            (0x80b1, x1 | (y1 << 16)),
            (0x80d0, xs),        // GRAS_SC_VIEWPORT_SCISSOR[0]
            (0x80d1, x1 | (y1 << 16)),
            (0x8010, ratio_f32(2 * xs as i64 + c.dw as i64, 2)), // viewport x offset
            (0x8011, half_w),
            (0x8012, half_h),
            (0x8013, half_h),
            (0x8006, 0x0005_ad77), // guardband for ~1080x1200 (fd_calc_guardband)
            (0x9b00, 0),
        ];
        let d = Draw {
            overlay: TEXQ,
            vs: mem.iova(VS_OFF),
            fs: mem.iova(FS_TEX_OFF),
            vb: mem.iova(COMPOSE_VB_OFF + 0x80 * k as usize),
            desc: Some(mem.iova(COMPOSE_DESC_OFF)),
            initiator: 0x186,
            verts: 4,
        };
        emit_state(ring, &t, &d);
        for (r, v) in pass {
            ring.reg(r, &[v]);
        }
        emit_draw(ring, &d);
    }
    let ts = mem.iova(CCU_TS_OFF);
    ring.event(PC_CCU_FLUSH_COLOR_TS, Some((ts, 0x13)));
    ring.event(PC_CCU_INVALIDATE_COLOR, None);
    ring.wfi();
}
