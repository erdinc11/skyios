// Render command encoder that draws on the Adreno 618, for the subset that
// has GPU code: render target = one IOSurface texture in a 32-bit RGBA/BGRA
// format (imported with user client selector 6), load action clear by the 2D
// engine, triangle lists/strips with float4 positions from setVertexBytes /
// setVertexBuffer (float4 position, + float2 uv for textured programs),
// fragment texture 0 = a BGRA8 IOSurface, pipelines whose functions are built into this bundle
// (hand-assembled ir3 below). Register state = the kernel's cp3d.rs tables
// (../test/cp3d-tables.h). Anything else falls back to the recording
// encoder (SkyRecord.m) and draws nothing.

#pragma clang diagnostic ignored "-Wprotocol"
#pragma clang diagnostic ignored "-Wobjc-property-implementation"
#pragma clang diagnostic ignored "-Wobjc-protocol-property-synthesis"

#import "SkyMetal.h"
#import <IOSurface/IOSurfaceRef.h>
#import <objc/runtime.h>
#include <os/lock.h>
#include "../test/cp3d-tables.h"

#pragma mark - PM4

static uint32_t parity(uint32_t v) {
    return (0x9669 >> (0xf & (v ^ (v >> 4) ^ (v >> 8) ^ (v >> 12) ^ (v >> 16) ^ (v >> 20) ^ (v >> 24) ^ (v >> 28)))) & 1;
}
static uint32_t pkt4(uint32_t reg, uint32_t cnt) {
    return 0x40000000 | (parity(reg) << 27) | (reg << 8) | (parity(cnt) << 7) | cnt;
}
static uint32_t pkt7(uint32_t op, uint32_t cnt) {
    return 0x70000000 | (parity(op) << 23) | (op << 16) | (parity(cnt) << 15) | cnt;
}
static void emit(NSMutableData *s, uint32_t v) {
    [s appendBytes:&v length:4];
}
static void reg(NSMutableData *s, uint32_t r, const uint32_t *v, uint32_t n) {
    emit(s, pkt4(r, n));
    for (uint32_t i = 0; i < n; i++)
        emit(s, v[i]);
}
static void reg1(NSMutableData *s, uint32_t r, uint32_t v) {
    reg(s, r, &v, 1);
}
static void reg64(NSMutableData *s, uint32_t r, uint64_t a) {
    uint32_t v[2] = {(uint32_t)a, (uint32_t)(a >> 32)};
    reg(s, r, v, 2);
}
static void event(NSMutableData *s, uint32_t ev) {
    emit(s, pkt7(0x46, 1));
    emit(s, ev);
}
static void event_ts(NSMutableData *s, uint32_t ev, uint64_t a, uint32_t v) {
    emit(s, pkt7(0x46, 4));
    emit(s, ev);
    emit(s, (uint32_t)a);
    emit(s, (uint32_t)(a >> 32));
    emit(s, v);
}
static void wfi(NSMutableData *s) {
    emit(s, pkt7(0x26, 0));
}
static uint32_t fbits(float f) {
    uint32_t u;
    memcpy(&u, &f, 4);
    return u;
}

#define FMT6_8_8_8_8_UNORM 0x30
#define CCU_CNTL_BYPASS ((0x10000 >> 12) << 23)

#pragma mark - built-in programs

// GPU memory shared by all encoders of the process: shader code (128-byte
// aligned), zeroed border colours, a CCU timestamp word
enum { CODE_VS = 0x000, CODE_FS_RED = 0x080, CODE_FS_TEX = 0x100, CODE_BORDER = 0x200, CODE_TS = 0x300, CODE_SIZE = 0x4000 };

static SkyMem sky_code;

static BOOL code_ready(void) {
    static os_unfair_lock lock = OS_UNFAIR_LOCK_INIT;
    os_unfair_lock_lock(&lock);
    if (!sky_code.handle && sky_alloc(CODE_SIZE, &sky_code)) {
        uint32_t *w = sky_code.cpu;
        memset(w, 0, CODE_SIZE);
        // VS: end (VFD writes the float4 position straight into r0)
        w[CODE_VS / 4 + 1] = 0x03000000;
        // FS: mov.f32f32 r0.x..w = 1, 0, 0, 1; end
        const uint32_t red[10] = {0x3f800000, 0x20444000, 0, 0x20444001, 0, 0x20444002, 0x3f800000, 0x20444003, 0, 0x03000000};
        memcpy(&w[CODE_FS_RED / 4], red, sizeof(red));
        // FS: texture prefetch (SP_PS_INITIAL_TEX_LOAD, uv varying -> r1);
        // bary.f (ei)r63.x, 0, r0.x; (sy)end
        const uint32_t tex[4] = {0x00002000, 0x473080fc, 0, 0x13000000};
        memcpy(&w[CODE_FS_TEX / 4], tex, sizeof(tex));
    }
    os_unfair_lock_unlock(&lock);
    return sky_code.handle != 0;
}

// built-in programs: vertex function, fragment function, fragment code,
// whether it samples texture 0 (then the vertex layout is float4 position +
// float2 uv, the cp3d.rs TEXQ state)
static const struct {
    NSString *vertex, *fragment;
    uint32_t code;
    BOOL textured;
} sky_programs[] = {
    {@"sky_position_vertex", @"sky_red_fragment", CODE_FS_RED, NO},
    {@"sky_position_uv_vertex", @"sky_texture_fragment", CODE_FS_TEX, YES},
};

#pragma mark - render targets

// IOSurface ID -> GPU mapping, imported once per process
typedef struct {
    uint64_t handle, iova, bpr, offset;
} SkyImport;

static BOOL import_surface(IOSurfaceRef s, SkyImport *out) {
    static os_unfair_lock lock = OS_UNFAIR_LOCK_INIT;
    static NSMutableDictionary<NSNumber *, NSData *> *cache;
    uint32_t sid = IOSurfaceGetID(s);
    os_unfair_lock_lock(&lock);
    if (!cache)
        cache = [NSMutableDictionary new];
    NSData *d = cache[@(sid)];
    if (!d) {
        uint64_t in = sid, o[4];
        uint32_t n = 4;
        if (IOConnectCallScalarMethod(sky_conn, SKY_IMPORT, &in, 1, o, &n) == KERN_SUCCESS) {
            SkyImport im = {o[0], o[1], o[2], o[3]};
            d = [NSData dataWithBytes:&im length:sizeof(im)];
            cache[@(sid)] = d;
        }
    }
    os_unfair_lock_unlock(&lock);
    if (!d)
        return NO;
    memcpy(out, d.bytes, sizeof(*out));
    return YES;
}

// per-command-buffer upload memory for setVertexBytes, freed with it
@interface SkyUpload : NSObject {
@public
    SkyMem mem;
    size_t used;
}
@end
@implementation SkyUpload
- (void)dealloc {
    sky_free(&mem);
}
@end

#pragma mark - encoder

// SkyRenderPipeline (SkyRecord.m) properties; called directly, not through
// KVC: valueForKey: tries -getVname first, which SkyObject's missing-selector
// resolver answers with nil (0112j)
@protocol SkyPipelineNames
- (NSString *)vname;
- (NSString *)fname;
- (uint32_t)mrtControl;
- (uint32_t)mrtBlend;
@end

// MTLBlendFactor -> adreno_rb_blend_factor; MTLBlendOperation values equal
// a3xx_rb_blend_opcode (add, src - dst, dst - src, min, max)
static uint32_t blend_factor(MTLBlendFactor f) {
    static const uint8_t map[] = {0, 1, 4, 5, 6, 7, 8, 9, 10, 11, 16, 12, 13, 14, 15, 20, 21, 22, 23};
    return f < sizeof(map) ? map[f] : 1;
}

void sky_blend_state(MTLRenderPipelineColorAttachmentDescriptor *c, uint32_t *control, uint32_t *blend) {
    MTLColorWriteMask m = c.writeMask;
    // COMPONENT_ENABLE: bit 0 = R .. bit 3 = A; Metal: R = 8, G = 4, B = 2, A = 1
    uint32_t comp = (m & MTLColorWriteMaskRed ? 1 : 0) | (m & MTLColorWriteMaskGreen ? 2 : 0) |
                    (m & MTLColorWriteMaskBlue ? 4 : 0) | (m & MTLColorWriteMaskAlpha ? 8 : 0);
    // ROP_CODE COPY (0xc) as in cp3d.rs STATE; COLOR_BLEND_EN | ALPHA_BLEND_EN
    *control = (comp << 7) | (0xc << 3) | (c.blendingEnabled ? 3 : 0);
    *blend = blend_factor(c.sourceRGBBlendFactor) | ((uint32_t)c.rgbBlendOperation << 5) |
             (blend_factor(c.destinationRGBBlendFactor) << 8) | (blend_factor(c.sourceAlphaBlendFactor) << 16) |
             ((uint32_t)c.alphaBlendOperation << 21) | (blend_factor(c.destinationAlphaBlendFactor) << 24);
}

@interface SkyDrawEncoder : SkyObject <MTLRenderCommandEncoder>
@property (weak) id<MTLDevice> device;
@property (copy) NSString *label;
@end

@implementation SkyDrawEncoder {
    NSMutableData *_s;
    id _owner; // the command buffer (keeps the uploads alive)
    SkyImport _rt;
    uint32_t _w, _h, _info; // RB_MRT[0].BUF_INFO
    NSString *_vname, *_fname;
    uint32_t _mrtControl, _mrtBlend;
    uint64_t _vb;
    id<MTLTexture> _tex;
    NSUInteger _draws, _skipped;
}

- (instancetype)initWithStream:(NSMutableData *)s owner:(id)owner target:(SkyImport)rt width:(uint32_t)w height:(uint32_t)h info:(uint32_t)info {
    if ((self = [super init])) {
        _info = info;
        _s = s;
        _owner = owner;
        _rt = rt;
        _w = w;
        _h = h;
    }
    return self;
}

- (uint64_t)upload:(const void *)p length:(NSUInteger)len {
    static const char key;
    NSMutableArray<SkyUpload *> *all = objc_getAssociatedObject(_owner, &key);
    if (!all) {
        all = [NSMutableArray array];
        objc_setAssociatedObject(_owner, &key, all, OBJC_ASSOCIATION_RETAIN);
    }
    SkyUpload *u = all.lastObject;
    if (!u || u->used + len > u->mem.size) {
        u = [SkyUpload new];
        size_t size = MAX(len, 65536);
        if (!sky_alloc(size, &u->mem))
            return 0;
        u->mem.size = size;
        [all addObject:u];
    }
    uint64_t a = u->mem.iova + u->used;
    memcpy((uint8_t *)u->mem.cpu + u->used, p, len);
    u->used = (u->used + len + 63) & ~(size_t)63;
    return a;
}

- (void)setRenderPipelineState:(id<MTLRenderPipelineState>)p {
    _vname = [(id<SkyPipelineNames>)p vname];
    _fname = [(id<SkyPipelineNames>)p fname];
    _mrtControl = [(id<SkyPipelineNames>)p mrtControl];
    _mrtBlend = [(id<SkyPipelineNames>)p mrtBlend];
}

- (void)setVertexBytes:(const void *)p length:(NSUInteger)l atIndex:(NSUInteger)i {
    if (i == 0)
        _vb = [self upload:p length:l];
}

- (void)setVertexBuffer:(id<MTLBuffer>)b offset:(NSUInteger)o atIndex:(NSUInteger)i {
    if (i == 0)
        _vb = b.gpuAddress + o;
}

// cp3d.rs emit_state + emit_draw, viewport/scissors for the whole target
- (void)draw:(uint32_t)initiator count:(NSUInteger)n start:(NSUInteger)first {
    int prog = -1;
    for (int i = 0; i < (int)(sizeof(sky_programs) / sizeof(sky_programs[0])); i++)
        if ([_vname isEqualToString:sky_programs[i].vertex] && [_fname isEqualToString:sky_programs[i].fragment])
            prog = i;
    BOOL textured = prog >= 0 && sky_programs[prog].textured;
    uint64_t desc = textured ? [self textureDescriptor] : 0;
    if (prog < 0 || !_vb || first != 0 || (textured && !desc)) {
        if (_skipped++ == 0)
            sky_log("draw skipped: no GPU code for %s + %s (or no usable texture)", _vname.UTF8String, _fname.UTF8String);
        return;
    }
    uint64_t fs = sky_programs[prog].code;
    NSMutableData *s = _s;
    reg1(s, 0xbb08, 0x19f); // SP_UPDATE_CNTL
    for (size_t i = 0; i < sizeof(CP3D_STATE) / sizeof(CP3D_STATE[0]); i++)
        reg1(s, CP3D_STATE[i][0], CP3D_STATE[i][1]);
    if (textured)
        for (size_t i = 0; i < sizeof(CP3D_TEXQ) / sizeof(CP3D_TEXQ[0]); i++)
            reg1(s, CP3D_TEXQ[i][0], CP3D_TEXQ[i][1]);
    // blend state of the pipeline (STATE has blending off, all components)
    BOOL blend = (_mrtControl & 3) != 0;
    reg1(s, 0x8820, _mrtControl ? _mrtControl : 0x7e0); // RB_MRT[0].CONTROL
    reg1(s, 0x8821, _mrtBlend);                         // RB_MRT[0].BLEND_CONTROL
    reg1(s, 0xa989, blend ? 0x101 : 0x100);              // SP_BLEND_CNTL: ENABLE_BLEND mrt0
    reg1(s, 0x8865, blend ? 0xffff0101 : 0xffff0100);    // RB_BLEND_CNTL: BLEND_READS_DEST mrt0
    uint32_t br = (_w - 1) | ((_h - 1) << 16);
    reg1(s, 0x80f1, br); // GRAS_SC_WINDOW_SCISSOR_BR
    reg1(s, 0x840b, br); // GRAS_A2D_SCISSOR_BR
    reg1(s, 0x80d1, br); // GRAS_SC_VIEWPORT_SCISSOR[0].BR
    reg1(s, 0x80b1, br); // GRAS_SC_SCREEN_SCISSOR[0].BR
    // GRAS_CL_VIEWPORT[0]: x offset, scale, y offset, scale (NDC y = -1 is row 0)
    uint32_t vp[4] = {fbits(_w / 2.0f), fbits(_w / 2.0f), fbits(_h / 2.0f), fbits(_h / 2.0f)};
    reg(s, 0x8010, vp, 4);
    reg1(s, 0xa012, (uint32_t)((textured ? 24 : 16) * n)); // VFD_VERTEX_BUFFER[0].SIZE
    uint32_t mrt[6] = {_info, (uint32_t)(_rt.bpr >> 6), 0, (uint32_t)(_rt.iova + _rt.offset),
                       (uint32_t)((_rt.iova + _rt.offset) >> 32), 0};
    reg(s, 0x8822, mrt, 6);
    reg1(s, 0xa996, FMT6_8_8_8_8_UNORM);
    uint32_t zero6[6] = {0};
    for (uint32_t i = 1; i < 8; i++) {
        reg(s, 0x8822 + 8 * i, zero6, 6);
        reg1(s, 0xa996 + i, 0);
    }
    uint64_t vs = sky_code.iova + CODE_VS, fsa = sky_code.iova + (uint64_t)fs;
    reg64(s, 0xa81c, vs);   // SP_VS_BASE
    reg64(s, 0xa983, fsa);  // SP_PS_BASE
    reg64(s, 0xa010, _vb);  // VFD_VERTEX_BUFFER[0].BASE
    emit(s, pkt7(0x65, 1)); // CP_SET_MARKER: RM6_DIRECT_RENDER
    emit(s, 1);
    emit(s, pkt7(0x1d, 1)); // CP_SKIP_IB2_ENABLE_GLOBAL
    emit(s, 0);
    emit(s, pkt7(0x64, 1)); // CP_SET_VISIBILITY_OVERRIDE
    emit(s, 1);
    emit(s, pkt7(0x63, 1)); // CP_SET_MODE
    emit(s, 0);
    emit(s, pkt7(0x32, 3)); // CP_LOAD_STATE6_GEOM: VS icache preload
    emit(s, (2 << 16) | (8 << 18) | (1 << 22));
    emit(s, (uint32_t)vs);
    emit(s, (uint32_t)(vs >> 32));
    emit(s, pkt7(0x34, 3)); // CP_LOAD_STATE6_FRAG: FS icache preload
    emit(s, (2 << 16) | (0xc << 18) | (1 << 22));
    emit(s, (uint32_t)fsa);
    emit(s, (uint32_t)(fsa >> 32));
    if (textured) {
        uint64_t samp = desc + 64;
        emit(s, pkt7(0x34, 3)); // samplers: ST6_SHADER, SB6_FS_TEX
        emit(s, (2 << 16) | (4 << 18) | (1 << 22));
        emit(s, (uint32_t)samp);
        emit(s, (uint32_t)(samp >> 32));
        reg64(s, 0xa9e0, samp);  // SP_PS_SAMPLER_BASE
        emit(s, pkt7(0x34, 3)); // textures: ST6_CONSTANTS, SB6_FS_TEX
        emit(s, (1 << 14) | (2 << 16) | (4 << 18) | (1 << 22));
        emit(s, (uint32_t)desc);
        emit(s, (uint32_t)(desc >> 32));
        reg64(s, 0xa9e4, desc);  // SP_PS_TEXMEMOBJ_BASE
        reg1(s, 0xa9a7, 1);      // SP_PS_TSIZE
    }
    emit(s, pkt7(0x38, 3)); // CP_DRAW_INDX_OFFSET: auto index, use visibility
    emit(s, initiator);
    emit(s, 1);
    emit(s, (uint32_t)n);
    _draws++;
}

- (void)drawPrimitives:(MTLPrimitiveType)t vertexStart:(NSUInteger)st vertexCount:(NSUInteger)n {
    if (t == MTLPrimitiveTypeTriangle)
        [self draw:0x184 count:n start:st];
    else if (t == MTLPrimitiveTypeTriangleStrip)
        [self draw:0x186 count:n start:st];
}

- (void)drawPrimitives:(MTLPrimitiveType)t vertexStart:(NSUInteger)st vertexCount:(NSUInteger)n instanceCount:(NSUInteger)ic {
    if (ic == 1)
        [self drawPrimitives:t vertexStart:st vertexCount:n];
}

// colour to memory, caches dropped (later passes may sample this target)
- (void)endEncoding {
    uint64_t ts = sky_code.iova + CODE_TS;
    event_ts(_s, 0x1d, ts, 0x40); // PC_CCU_FLUSH_COLOR_TS
    event(_s, 0x19);              // PC_CCU_INVALIDATE_COLOR
    wfi(_s);
    event(_s, 0x31);              // CACHE_INVALIDATE
    wfi(_s);
}

- (void)setViewport:(MTLViewport)v {}
- (void)setScissorRect:(MTLScissorRect)r {}
- (void)setCullMode:(MTLCullMode)m {}
- (void)setFrontFacingWinding:(MTLWinding)w {}
- (void)setTriangleFillMode:(MTLTriangleFillMode)m {}
- (void)setDepthStencilState:(id<MTLDepthStencilState>)s {}
- (void)setFragmentBuffer:(id<MTLBuffer>)b offset:(NSUInteger)o atIndex:(NSUInteger)i {}
- (void)setFragmentBytes:(const void *)p length:(NSUInteger)l atIndex:(NSUInteger)i {}
- (void)setFragmentTexture:(id<MTLTexture>)t atIndex:(NSUInteger)i {
    if (i == 0)
        _tex = t;
}

// A6XX_TEX_MEMOBJ + sampler (cp3d.rs tex_desc) for fragment texture 0, an
// IOSurface in BGRA8: FMT6_8_8_8_8_UNORM, swap WXYZ, linear; bilinear, clamp
- (uint64_t)textureDescriptor {
    IOSurfaceRef surf = _tex.iosurface;
    SkyImport im;
    MTLPixelFormat f = _tex.pixelFormat;
    if (!surf || (f != MTLPixelFormatBGRA8Unorm && f != MTLPixelFormatBGRA8Unorm_sRGB) || !import_surface(surf, &im) ||
        (im.bpr & 63))
        return 0;
    uint64_t base = im.iova + im.offset;
    uint32_t d[20] = {0x4c006880, (uint32_t)_tex.width | ((uint32_t)_tex.height << 15), 2 | ((uint32_t)im.bpr << 7) | (1u << 29), 0,
                      (uint32_t)base, (uint32_t)(base >> 32) | (1u << 17)};
    d[16] = 0x92a;
    d[17] = 0x40;
    return [self upload:d length:sizeof(d)];
}
- (void)setFragmentSamplerState:(id<MTLSamplerState>)s atIndex:(NSUInteger)i {}
- (void)pushDebugGroup:(NSString *)s {}
- (void)popDebugGroup {}
- (void)insertDebugSignpost:(NSString *)s {}
@end

#pragma mark - pass set-up

// cp3d.rs emit_init, then the clear: 2D solid fill of the whole target
static void begin_pass(NSMutableData *s, SkyImport rt, uint32_t w, uint32_t h, MTLRenderPassColorAttachmentDescriptor *c) {
    uint64_t ts = sky_code.iova + CODE_TS, border = sky_code.iova + CODE_BORDER;
    event(s, 0x31);            // CACHE_INVALIDATE
    reg1(s, 0xbb08, 0x000fffff);
    wfi(s);
    for (size_t i = 0; i < sizeof(CP3D_INIT) / sizeof(CP3D_INIT[0]); i++)
        reg1(s, CP3D_INIT[i][0], CP3D_INIT[i][1]);
    reg64(s, 0xb302, border);  // TPL1_GFX_BORDER_COLOR_BASE
    reg64(s, 0xb180, border);  // TPL1_CS_BORDER_COLOR_BASE
    emit(s, pkt7(0x43, 3));    // CP_SET_DRAW_STATE: DISABLE_ALL_GROUPS
    emit(s, 1 << 18);
    emit(s, 0);
    emit(s, 0);
    event_ts(s, 0x1d, ts, 0x10);
    event_ts(s, 0x1c, ts, 0x11);
    event(s, 0x19);
    event(s, 0x18);
    wfi(s);
    reg1(s, 0x8e07, CCU_CNTL_BYPASS);
    if (c.loadAction != MTLLoadActionClear)
        return;
    // clear colour as UNORM8 per channel; RB writes R into the low byte, so
    // swap R/B for BGRA memory order
    MTLClearColor cc = c.clearColor;
    BOOL bgra = c.texture.pixelFormat == MTLPixelFormatBGRA8Unorm || c.texture.pixelFormat == MTLPixelFormatBGRA8Unorm_sRGB;
    double ch[4] = {bgra ? cc.blue : cc.red, cc.green, bgra ? cc.red : cc.blue, cc.alpha};
    uint32_t rgba[4];
    for (int i = 0; i < 4; i++)
        rgba[i] = (uint32_t)lround(fmin(fmax(ch[i], 0), 1) * 255);
    uint32_t blt = (1 << 7) | (FMT6_8_8_8_8_UNORM << 8) | (0xf << 20);
    reg1(s, 0x8c00, blt);                                      // RB_A2D_BLT_CNTL
    reg1(s, 0x8400, blt);                                      // GRAS_A2D_BLT_CNTL
    reg1(s, 0xacc0, (FMT6_8_8_8_8_UNORM << 3) | (0xf << 12));  // SP_A2D_OUTPUT_INFO
    reg1(s, 0x8c01, 0);                                        // RB_A2D_PIXEL_CNTL
    uint64_t base = rt.iova + rt.offset;
    uint32_t info[4] = {FMT6_8_8_8_8_UNORM, (uint32_t)base, (uint32_t)(base >> 32), (uint32_t)(rt.bpr >> 6)};
    reg(s, 0x8c17, info, 4);                                   // RB_A2D_DEST_BUFFER_*
    uint32_t zero3[3] = {0, 0, 0};
    reg(s, 0x8c20, zero3, 3);
    reg(s, 0x8c2c, rgba, 4);                                   // RB_A2D_CLEAR_COLOR_DW0..3
    uint32_t tl[2] = {0, (w - 1) | ((h - 1) << 16)};
    reg(s, 0x8405, tl, 2);                                     // GRAS_A2D_DEST_TL/BR
    event(s, 0x3f);                                            // FD_LABEL
    wfi(s);
    emit(s, pkt7(0x2c, 1));                                    // CP_BLIT
    emit(s, 3);                                                // BLIT_OP_SCALE
    wfi(s);
    event_ts(s, 0x1d, ts, 0x12);
    event(s, 0x19);
    wfi(s);
}

id sky_draw_encoder(id<MTLDevice> dev, MTLRenderPassDescriptor *d, NSMutableData *stream, id owner) {
    MTLRenderPassColorAttachmentDescriptor *c = d.colorAttachments[0];
    id<MTLTexture> t = c.texture;
    IOSurfaceRef surf = t.iosurface;
    MTLPixelFormat f = t.pixelFormat;
    BOOL fmt_ok = f == MTLPixelFormatBGRA8Unorm || f == MTLPixelFormatRGBA8Unorm || f == MTLPixelFormatBGRA8Unorm_sRGB ||
                  f == MTLPixelFormatRGBA8Unorm_sRGB;
    SkyImport rt;
    if (!surf || !fmt_ok || !code_ready() || !import_surface(surf, &rt) || (rt.bpr & 63))
        return nil;
    uint32_t w = (uint32_t)t.width, h = (uint32_t)t.height;
    begin_pass(stream, rt, w, h, c);
    // BGRA memory order: COLOR_SWAP WXYZ (a3xx_color_swap 1) in RB_MRT[0].BUF_INFO
    BOOL bgra = f == MTLPixelFormatBGRA8Unorm || f == MTLPixelFormatBGRA8Unorm_sRGB;
    uint32_t info = FMT6_8_8_8_8_UNORM | ((bgra ? 1u : 0u) << 13);
    SkyDrawEncoder *e = [[SkyDrawEncoder alloc] initWithStream:stream owner:owner target:rt width:w height:h info:info];
    e.device = dev;
    return e;
}
