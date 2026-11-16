// Generated from skyios-kernel/src/cp3d.rs (INIT, STATE, TEXQ): the a6xx
// gen1 register values of the kernel 3D self tests, for user command streams.
#include <stdint.h>

static const uint32_t CP3D_INIT[][2] = {
    {0xae04, 0}, // SP_NC_MODE_CNTL_2
    {0xae0f, 0x3f}, // SP_PERFCTR_SHADER_MASK
    {0xb605, 0x44}, // TPL1_UNKNOWN_B605
    {0xbe00, 0x80}, // HLSQ_UNKNOWN_BE00
    {0xbe01, 0}, // HLSQ_UNKNOWN_BE01
    {0xab20, 0}, // SP_GFX_USIZE
    {0xb182, 0}, // TPL1_PS_ROTATION_CNTL
    {0xbb11, 0}, // HLSQ_SHARED_CONSTS
    {0xa9a8, 0}, // SP_UNKNOWN_A9A8
    {0xab00, 5}, // SP_MODE_CNTL: const demotion, ISAMMODE_GL
    {0xa009, 3}, // VFD_MODE_CNTL
    {0x8818, 0}, // RB_UNKNOWN_8818..881E
    {0x8819, 0},
    {0x881a, 0},
    {0x881b, 0},
    {0x881c, 0},
    {0x881d, 0},
    {0x881e, 0},
    {0x88f0, 0}, // RB_UNKNOWN_88F0
    {0x9236, 0}, // VPC_REPLACE_MODE_CNTL
    {0x9300, 0}, // VPC_ROTATION_CNTL
    {0x9306, 1}, // VPC_SO_OVERRIDE: streamout off
    {0xb183, 0}, // TPL1_PS_SWIZZLE_CNTL
    {0x80af, 0}, // GRAS_SC_SCREEN_SCISSOR_CNTL
    {0x8099, 0}, // GRAS_SU_CONSERVATIVE_RAS_CNTL
    {0x980a, 0}, // PC_DGEN_SU_CONSERVATIVE_RAS_CNTL
    {0x9210, 0}, // VPC_UNKNOWN_9210/9211
    {0x9211, 0},
    {0x9602, 0}, // VPC_LB_MODE_CNTL
    {0x9e72, 0}, // PC_CONTEXT_SWITCH_GFX_PREEMPTION_MODE
    {0xb309, 0xa2}, // TPL1_MODE_CNTL
    {0xb986, 0xfcfc}, // SP_REG_PROG_ID_3
    {0xa007, 0}, // VFD_RENDER_MODE: rendering pass
    {0x8864, 0}, // RB_ALPHA_TEST_CNTL
    {0x8100, 0}, // GRAS_LRZ_CNTL
    {0x8898, 0}, // RB_LRZ_CNTL
};

static const uint32_t CP3D_STATE[][2] = {
    {0x80f0, 0}, // GRAS_SC_WINDOW_SCISSOR_TL
    {0x80f1, 0x003f003f},
    {0x840a, 0}, // GRAS_A2D_SCISSOR_TL/BR
    {0x840b, 0x003f003f},
    {0x8890, 0}, // RB_WINDOW_OFFSET
    {0x88d4, 0}, // RB_RESOLVE_WINDOW_OFFSET
    {0xb4d1, 0}, // SP_WINDOW_OFFSET
    {0xb307, 0}, // TPL1_WINDOW_OFFSET
    {0x80a1, 0x00e00000}, // GRAS_SC_BIN_CNTL: buffers in sysmem, no LRZ write
    {0x8800, 0x00e00000}, // RB_CNTL
    {0x88d3, 0}, // RB_RESOLVE_CNTL_3
    {0x8872, 0}, // RB_DEPTH_BUFFER_INFO .. BASE_GMEM
    {0x8873, 0},
    {0x8874, 0},
    {0x8875, 0},
    {0x8876, 0},
    {0x8877, 0},
    {0x8098, 0}, // GRAS_SU_DEPTH_BUFFER_INFO
    {0x8881, 0}, // RB_STENCIL_BUFFER_INFO
    {0xa996, 0x30}, // SP_PS_MRT[0].REG: FMT6_8_8_8_8_UNORM
    {0x8903, 0}, // RB_COLOR_FLAG_BUFFER[0]
    {0x8904, 0},
    {0x8905, 0},
    {0x8102, 0x30}, // GRAS_LRZ_MRT_BUFFER_INFO_0
    {0x880e, 0}, // RB_DITHER_CNTL
    {0x880f, 0}, // RB_SRGB_CNTL
    {0xa98a, 0}, // SP_SRGB_CNTL
    {0x8004, 0}, // GRAS_CL_ARRAY_SIZE
    {0x8801, 0x10}, // RB_RENDER_CNTL: CCUSINGLECACHELINESIZE 2
    {0xb300, 0}, // TPL1_RAS_MSAA_CNTL
    {0xb301, 4}, // TPL1_DEST_MSAA_CNTL: MSAA_DISABLE
    {0x80a2, 0}, // GRAS_SC_RAS_MSAA_CNTL
    {0x80a3, 4}, // GRAS_SC_DEST_MSAA_CNTL
    {0x8802, 0}, // RB_RAS_MSAA_CNTL
    {0x8803, 4}, // RB_DEST_MSAA_CNTL
    {0xbb11, 0}, // HLSQ_SHARED_CONSTS
    {0xab00, 5}, // SP_MODE_CNTL
    {0xb800, 0x102}, // SP_VS_CONST_CONFIG: constlen 8, enabled
    {0xb801, 0}, // SP_HS/DS/GS_CONST_CONFIG
    {0xb802, 0},
    {0xb803, 0},
    {0xbb10, 0x100}, // SP_PS_CONST_CONFIG: enabled
    {0xa823, 0x100}, // SP_VS_CONFIG: enabled
    {0xa83b, 0}, // SP_HS/DS/GS_CONFIG
    {0xa863, 0},
    {0xa894, 0},
    {0xab04, 0x100}, // SP_PS_CONFIG: enabled
    {0xa800, 0x00100080}, // SP_VS_CNTL_0
    {0xa824, 1}, // SP_VS_INSTR_SIZE (x16 instructions)
    {0xa81b, 0}, // SP_VS_PROGRAM_COUNTER_OFFSET
    {0xa81e, 0}, // SP_VS_PVT_MEM_*
    {0xa81f, 0},
    {0xa820, 0},
    {0xa821, 0},
    {0xa825, 0},
    {0x9b07, 6}, // PC_STEREO_RENDERING_CNTL: 1 view, no multipos
    {0xa008, 6}, // VFD_STEREO_RENDERING_CNTL
    {0xa000, 0x101}, // VFD_CNTL_0: 1 fetch, 1 decode
    {0xa0d0, 0xf}, // VFD_DEST_CNTL[0]: r0.xyzw
    {0xa001, 0xfcfcfcfc}, // VFD_CNTL_1..5: no sysvals
    {0xa002, 0xfcfc},
    {0xa003, 0xfcfcfcfc},
    {0xa004, 0xfc},
    {0xa005, 0xfcfc},
    {0xa980, 0x81000080}, // SP_PS_CNTL_0
    {0xab05, 1}, // SP_PS_INSTR_SIZE
    {0xa982, 0}, // SP_PS_PROGRAM_COUNTER_OFFSET
    {0xa985, 0}, // SP_PS_PVT_MEM_*
    {0xa986, 0},
    {0xa987, 0},
    {0xa988, 0},
    {0xa9a9, 0},
    {0xa006, 0}, // VFD_CNTL_6
    {0x9806, 0}, // PC_PS_CNTL
    {0xa99e, 8}, // SP_PS_INITIAL_TEX_LOAD_CNTL: IJ write disabled
    {0xb982, 7}, // SP_LB_PARAM_LIMIT
    {0xb983, 0xfcfcfcfc}, // SP_REG_PROG_ID_0..2: nothing
    {0xb984, 0xfcfcfcfc},
    {0xb985, 0xfcfcfcfc},
    {0xb980, 0}, // SP_PS_WAVE_CNTL: THREAD64, no varyings
    {0x8005, 0}, // GRAS_CL_INTERP_CNTL
    {0x8809, 0}, // RB_INTERP_CNTL
    {0x880a, 0}, // RB_PS_INPUT_CNTL
    {0x8810, 0}, // RB_PS_SAMPLEFREQ_CNTL
    {0x8101, 0}, // GRAS_LRZ_PS_INPUT_CNTL
    {0x8109, 0}, // GRAS_LRZ_PS_SAMPLEFREQ_CNTL
    {0x9212, 0xffffffff}, // VPC_VARYING_LM_TRANSFER_CNTL[0..3].DISABLE
    {0x9213, 0xffffffff},
    {0x9214, 0xffffffff},
    {0x9215, 0xffffffff},
    {0x9304, 0xff00ff00}, // VPC_PS_CNTL
    {0xa98c, 0xfcfcfc00}, // SP_PS_OUTPUT_CNTL
    {0xa98e, 0}, // SP_PS_OUTPUT[0].REG: r0
    {0xa98b, 0xf}, // SP_PS_OUTPUT_MASK
    {0x880b, 0}, // RB_PS_OUTPUT_CNTL
    {0x880d, 0xf}, // RB_PS_OUTPUT_MASK
    {0x9216, 0}, // VPC_SO_MAPPING_WPTR
    {0x9305, 0}, // VPC_SO_CNTL
    {0xa803, 0xf00}, // SP_VS_OUTPUT[0].REG
    {0xa813, 0}, // SP_VS_VPC_DEST[0].REG
    {0x9301, 0x00ff0004}, // VPC_VS_CNTL: stride 4, position loc 0
    {0x9101, 0x00ffff00}, // VPC_VS_CLIP_CULL_CNTL
    {0x9311, 0x00ffff00}, // VPC_VS_CLIP_CULL_CNTL_V2
    {0x8001, 0}, // GRAS_CL_VS_CLIP_CULL_DISTANCE
    {0x9b01, 4}, // PC_VS_CNTL
    {0xa802, 1}, // SP_VS_OUTPUT_CNTL
    {0x9104, 0x00ffffff}, // VPC_VS_SIV_CNTL
    {0x9314, 0x00ffffff}, // VPC_VS_SIV_CNTL_V2
    {0x809b, 0}, // GRAS_SU_VS_SIV_CNTL
    {0xa090, 0xc8200000}, // VFD_FETCH_INSTR[0]: FMT6_32_32_32_32_FLOAT
    {0xa091, 0},
    {0xa013, 16}, // VFD_VERTEX_BUFFER[0].STRIDE
    {0xa012, 48}, // VFD_VERTEX_BUFFER[0].SIZE
    {0xa00e, 0}, // VFD_INDEX_OFFSET
    {0xa00f, 0}, // VFD_INSTANCE_START_OFFSET
    {0x8010, 0x42000000}, // GRAS_CL_VIEWPORT[0] X/Y offset+scale = 32
    {0x8011, 0x42000000},
    {0x8012, 0x42000000},
    {0x8013, 0x42000000},
    {0x8014, 0},
    {0x8015, 0x3f800000},
    {0x80d0, 0}, // GRAS_SC_VIEWPORT_SCISSOR[0]
    {0x80d1, 0x003f003f},
    {0x8070, 0}, // GRAS_CL_VIEWPORT_ZCLAMP[0]
    {0x8071, 0x3f800000},
    {0x88c0, 0}, // RB_VIEWPORT_ZCLAMP_MIN/MAX
    {0x88c1, 0x3f800000},
    {0x8006, 0x0007fdff}, // GRAS_CL_GUARDBAND_CLIP_ADJ
    {0x80b0, 0}, // GRAS_SC_SCREEN_SCISSOR[0]
    {0x80b1, 0x003f003f},
    {0x8090, 0x2010}, // GRAS_SU_CNTL
    {0x8000, 0xc0}, // GRAS_CL_CNTL
    {0x9108, 3}, // VPC_RAST_CNTL: triangles
    {0x9981, 3}, // PC_DGEN_RAST_CNTL
    {0x9107, 0}, // VPC_RAST_STREAM_CNTL
    {0x9980, 0}, // PC_RAST_STREAM_CNTL
    {0x8091, 0xffc00001}, // GRAS_SU_POINT_MINMAX
    {0x8092, 0x10}, // GRAS_SU_POINT_SIZE
    {0x8095, 0}, // GRAS_SU_POLY_OFFSET_*
    {0x8096, 0},
    {0x8097, 0},
    {0x80a0, 2}, // GRAS_SC_CNTL
    {0x80a4, 0}, // sample positions off
    {0x8804, 0},
    {0xb304, 0},
    {0x8880, 0}, // RB_STENCIL_CNTL
    {0x8115, 0}, // GRAS_SU_STENCIL_CNTL
    {0x8888, 0}, // RB_STENCIL_MASK
    {0x8889, 0}, // RB_STENCIL_WRITE_MASK
    {0x8887, 0}, // RB_STENCIL_REF_CNTL
    {0x8871, 0}, // RB_DEPTH_CNTL
    {0x8114, 0}, // GRAS_SU_DEPTH_CNTL
    {0x8878, 0}, // RB_DEPTH_BOUND_MIN/MAX
    {0x8879, 0},
    {0x8094, 0}, // GRAS_SU_DEPTH_PLANE_CNTL: early z
    {0x8870, 0}, // RB_DEPTH_PLANE_CNTL
    {0xa989, 0x100}, // SP_BLEND_CNTL
    {0x8865, 0xffff0100}, // RB_BLEND_CNTL
    {0x8820, 0x7e0}, // RB_MRT[0].CONTROL: RGBA enabled, ROP_COPY, no blend
    {0x8821, 0x00010001}, // RB_MRT[0].BLEND_CONTROL
    {0xa98d, 1}, // SP_PS_MRT_CNTL
    {0x880c, 1}, // RB_PS_MRT_CNTL
    {0x8860, 0}, // RB_BLEND_CONSTANT_*
    {0x8861, 0},
    {0x8862, 0},
    {0x8863, 0},
    {0x9b00, 0}, // PC_CNTL
    {0x9803, 0xffffffff}, // PC_RESTART_INDEX
    {0x9802, 0}, // PC_DS_PARAM
};

static const uint32_t CP3D_TEXQ[][2] = {
    {0xa000, 0x202}, // VFD_CNTL_0: 2 fetches, 2 decodes
    {0xa0d1, 0x43}, // VFD_DEST_CNTL[1]: r1.xy
    {0xa092, 0xc6700200}, // VFD_FETCH_INSTR[1]: FMT6_32_32_FLOAT at +16
    {0xa093, 0},
    {0xa013, 24}, // VFD_VERTEX_BUFFER[0].STRIDE
    {0xa012, 96}, // VFD_VERTEX_BUFFER[0].SIZE
    {0xa800, 0x00100100}, // SP_VS_CNTL_0: footprint 2
    {0xa803, 0x0f000304}, // SP_VS_OUTPUT[0]: uv r1.xy, pos r0.xyzw
    {0xa813, 0x200}, // SP_VS_VPC_DEST[0]: uv loc 0, pos loc 2
    {0x9301, 0x00ff0206}, // VPC_VS_CNTL: stride 6, position loc 2
    {0x9b01, 6}, // PC_VS_CNTL
    {0xa802, 2}, // SP_VS_OUTPUT_CNTL
    {0x9200, 0}, // VPC_VARYING_INTERP_MODE[0..7]: smooth
    {0x9201, 0},
    {0x9202, 0},
    {0x9203, 0},
    {0x9204, 0},
    {0x9205, 0},
    {0x9206, 0},
    {0x9207, 0},
    {0x9208, 0}, // VPC_VARYING_REPLACE_MODE[0..7]
    {0x9209, 0},
    {0x920a, 0},
    {0x920b, 0},
    {0x920c, 0},
    {0x920d, 0},
    {0x920e, 0},
    {0x920f, 0},
    {0xa980, 0x85400100}, // SP_PS_CNTL_0: footprint 2, VARYING, PIXLODENABLE
    {0xb980, 2}, // SP_PS_WAVE_CNTL: THREAD64, VARYINGS
    {0xab04, 0x00020300}, // SP_PS_CONFIG: 1 tex, 1 sampler
    {0xa99e, 0x11}, // SP_PS_INITIAL_TEX_LOAD_CNTL: 1 prefetch, ENDOFQUAD
    {0xa99f, 0x23c40000}, // ..._CMD[0]: SAM loc 0 -> r1.xyzw, s#0 t#0
    {0xa9a3, 0}, // SP_PS_INITIAL_TEX_INDEX[0]
    {0xb984, 0xfcfcfc00}, // SP_REG_PROG_ID_1: IJ_PERSP_PIXEL in r0.xy
    {0x8005, 1}, // GRAS_CL_INTERP_CNTL: IJ_PERSP_PIXEL
    {0x8809, 0x401}, // RB_INTERP_CNTL: IJ_PERSP_PIXEL, INTERP_EN
    {0x9212, 0xfffffffc}, // VPC_VARYING_LM_TRANSFER_CNTL[0]: locs 0,1
    {0x9304, 0xff01ff02}, // VPC_PS_CNTL: 2 varyings
    {0xa98e, 4}, // SP_PS_OUTPUT[0].REG: r1
};

