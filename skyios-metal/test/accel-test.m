// User-space test of the SkyAdrenoAccel service (xnu 0109, skyios-kernel
// cpuser.rs): GPU buffers shared with this task, command streams run as
// indirect buffers; then, with sweetaccel=2 and SkyAdrenoMetal.bundle
// installed, the same through Metal (MTLCreateSystemDefaultDevice, blit
// encoder fill and copy). Runs once at boot in place of the daemon named by
// sweetrun=, then execs that daemon. Results go to the kernel log.

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <IOKit/IOKitLib.h>
#include <IOSurface/IOSurfaceRef.h>
#include "cp3d-tables.h"
#include <fcntl.h>
#include <mach/mach_time.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

enum { SEL_INFO, SEL_ALLOC, SEL_FREE, SEL_SUBMIT, SEL_WAIT, SEL_LOG, SEL_IMPORT };

static io_connect_t conn;

static void say(const char *fmt, ...) {
    char text[160];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(text, sizeof(text), fmt, ap);
    va_end(ap);
    int fd = open("/dev/console", O_WRONLY | O_NOCTTY);
    if (fd >= 0) {
        dprintf(fd, "skyios-accel-test: %s\n", text);
        close(fd);
    }
    if (conn)
        IOConnectCallMethod(conn, SEL_LOG, NULL, 0, text, strlen(text), NULL, NULL, NULL, NULL);
    usleep(200000); // let the line reach the host before the next step
}

struct buf {
    uint64_t handle, iova;
    volatile uint32_t *cpu;
    uint32_t dw;
};

static int alloc(struct buf *b, uint64_t size) {
    uint64_t in = size, out[3];
    uint32_t n = 3;
    kern_return_t kr = IOConnectCallScalarMethod(conn, SEL_ALLOC, &in, 1, out, &n);
    if (kr != KERN_SUCCESS) {
        say("alloc %llu: %#x", size, kr);
        return -1;
    }
    b->handle = out[0];
    b->iova = out[1];
    b->cpu = (volatile uint32_t *)(uintptr_t)out[2];
    b->dw = 0;
    return 0;
}

static int submit_wait(struct buf *cmd, uint64_t ms, uint64_t *ns) {
    uint64_t in[3] = {cmd->handle, 0, cmd->dw}, seq = 0, st = 9;
    uint32_t n = 1;
    uint64_t t0 = mach_absolute_time();
    kern_return_t kr = IOConnectCallScalarMethod(conn, SEL_SUBMIT, in, 3, &seq, &n);
    if (kr != KERN_SUCCESS) {
        say("submit: %#x", kr);
        return -1;
    }
    uint64_t w[2] = {seq, ms};
    n = 1;
    kr = IOConnectCallScalarMethod(conn, SEL_WAIT, w, 2, &st, &n);
    if (ns) {
        mach_timebase_info_data_t tb;
        mach_timebase_info(&tb);
        *ns = (mach_absolute_time() - t0) * tb.numer / tb.denom;
    }
    if (kr != KERN_SUCCESS || st != 0) {
        say("wait seq %llu: kr %#x status %llu", seq, kr, st);
        return -1;
    }
    return 0;
}

// PM4 packets (adreno_pm4.xml): type 4 register writes, type 7 opcodes
static uint32_t parity(uint32_t v) {
    return (0x9669 >> (0xf & (v ^ (v >> 4) ^ (v >> 8) ^ (v >> 12) ^ (v >> 16) ^ (v >> 20) ^ (v >> 24) ^ (v >> 28)))) & 1;
}
static uint32_t pkt4(uint32_t reg, uint32_t cnt) {
    return 0x40000000 | (parity(reg) << 27) | (reg << 8) | (parity(cnt) << 7) | cnt;
}
static uint32_t pkt7(uint32_t op, uint32_t cnt) {
    return 0x70000000 | (parity(op) << 23) | (op << 16) | (parity(cnt) << 15) | cnt;
}

static void emit(struct buf *b, uint32_t v) { b->cpu[b->dw++] = v; }
static void reg(struct buf *b, uint32_t r, const uint32_t *v, uint32_t n) {
    emit(b, pkt4(r, n));
    for (uint32_t i = 0; i < n; i++)
        emit(b, v[i]);
}
static void reg1(struct buf *b, uint32_t r, uint32_t v) { reg(b, r, &v, 1); }
static void event(struct buf *b, uint32_t ev) {
    emit(b, pkt7(0x46, 1));
    emit(b, ev);
}

#define CP_WAIT_FOR_IDLE 0x26
#define CP_BLIT 0x2c
#define CP_MEM_WRITE 0x3d
#define FMT6_8_8_8_8_UNORM 0x30
#define CCU_CNTL_BYPASS ((0x10000 >> 12) << 23)

// 2D engine solid fill of [x0,x1) x [y0,y1) in a linear RGBA8 surface, the
// kernel's cp.rs solid_fill() sequence without the one-time static state
static void fill(struct buf *b, uint64_t dst, uint32_t pitch, uint32_t x0, uint32_t y0, uint32_t x1, uint32_t y1,
                 const uint32_t rgba[4]) {
    event(b, 0x18); // PC_CCU_INVALIDATE_DEPTH
    event(b, 0x19); // PC_CCU_INVALIDATE_COLOR
    emit(b, pkt7(CP_WAIT_FOR_IDLE, 0));
    reg1(b, 0x8e07, CCU_CNTL_BYPASS); // RB_CCU_CNTL
    uint32_t fmt = FMT6_8_8_8_8_UNORM;
    uint32_t blt = (1 << 7) | (fmt << 8) | (0xf << 20);
    reg1(b, 0x8c00, blt);                        // RB_A2D_BLT_CNTL
    reg1(b, 0x8400, blt);                        // GRAS_A2D_BLT_CNTL
    reg1(b, 0xacc0, (fmt << 3) | (0xf << 12));   // SP_A2D_OUTPUT_INFO
    reg1(b, 0x8c01, 0);                          // RB_A2D_PIXEL_CNTL
    uint32_t info[4] = {fmt, (uint32_t)dst, (uint32_t)(dst >> 32), pitch >> 6};
    reg(b, 0x8c17, info, 4);                     // RB_A2D_DEST_BUFFER_INFO..PITCH
    uint32_t zero[3] = {0, 0, 0};
    reg(b, 0x8c20, zero, 3);                     // RB_A2D_DEST_FLAG_BUFFER_BASE
    reg(b, 0x8c2c, rgba, 4);                     // RB_A2D_CLEAR_COLOR_DW0..3
    uint32_t tl[2] = {x0 | (y0 << 16), (x1 - 1) | ((y1 - 1) << 16)};
    reg(b, 0x8405, tl, 2);                       // GRAS_A2D_DEST_TL/BR
    event(b, 0x3f);                              // FD_LABEL
    emit(b, pkt7(CP_WAIT_FOR_IDLE, 0));
    emit(b, pkt7(CP_BLIT, 1));
    emit(b, 3);                                  // BLIT_OP_SCALE
    emit(b, pkt7(CP_WAIT_FOR_IDLE, 0));
}


// 3D draw from a user command stream, the kernel's cp3d.rs triangle():
// emit_init + emit_state + CP_DRAW_INDX_OFFSET, into a 64x64 RGBA8 target
static void lohi_reg(struct buf *b, uint32_t r, uint64_t a) {
    uint32_t v[2] = {(uint32_t)a, (uint32_t)(a >> 32)};
    reg(b, r, v, 2);
}

static void event_ts(struct buf *b, uint32_t ev, uint64_t a, uint32_t v) {
    emit(b, pkt7(0x46, 4));
    emit(b, ev);
    emit(b, (uint32_t)a);
    emit(b, (uint32_t)(a >> 32));
    emit(b, v);
}

// emit_init of cp3d.rs: static state, CCU in sysmem (bypass) mode
static void init3d(struct buf *b, uint64_t border, uint64_t ts) {
    event(b, 0x31); // CACHE_INVALIDATE
    reg1(b, 0xbb08, 0x000fffff); // SP_UPDATE_CNTL
    emit(b, pkt7(CP_WAIT_FOR_IDLE, 0));
    for (size_t i = 0; i < sizeof(CP3D_INIT) / sizeof(CP3D_INIT[0]); i++)
        reg1(b, CP3D_INIT[i][0], CP3D_INIT[i][1]);
    lohi_reg(b, 0xb302, border); // TPL1_GFX_BORDER_COLOR_BASE
    lohi_reg(b, 0xb180, border); // TPL1_CS_BORDER_COLOR_BASE
    emit(b, pkt7(0x43, 3));      // CP_SET_DRAW_STATE: DISABLE_ALL_GROUPS
    emit(b, 1 << 18);
    emit(b, 0);
    emit(b, 0);
    event_ts(b, 0x1d, ts, 0x10); // PC_CCU_FLUSH_COLOR_TS
    event_ts(b, 0x1c, ts, 0x11); // PC_CCU_FLUSH_DEPTH_TS
    event(b, 0x19);
    event(b, 0x18);
    emit(b, pkt7(CP_WAIT_FOR_IDLE, 0));
    reg1(b, 0x8e07, CCU_CNTL_BYPASS);
}

struct draw3d {
    const uint32_t (*overlay)[2];
    size_t noverlay;
    uint64_t vs, fs, vb;
    uint64_t desc; // texture descriptor (16 dwords) + sampler at +64, or 0
    uint32_t initiator, verts;
};

// emit_state + emit_draw of cp3d.rs into an RGBA8 target
static void draw3d(struct buf *b, const struct draw3d *d, uint64_t target, uint32_t pitch) {
    reg1(b, 0xbb08, 0x19f);
    for (size_t i = 0; i < sizeof(CP3D_STATE) / sizeof(CP3D_STATE[0]); i++)
        reg1(b, CP3D_STATE[i][0], CP3D_STATE[i][1]);
    for (size_t i = 0; i < d->noverlay; i++)
        reg1(b, d->overlay[i][0], d->overlay[i][1]);
    uint32_t mrt[6] = {0x30, pitch >> 6, 0, (uint32_t)target, (uint32_t)(target >> 32), 0};
    reg(b, 0x8822, mrt, 6);      // RB_MRT[0]: BUF_INFO, PITCH, ARRAY_PITCH, BASE, BASE_GMEM
    reg1(b, 0xa996, 0x30);
    uint32_t zero6[6] = {0};
    for (uint32_t i = 1; i < 8; i++) {
        reg(b, 0x8822 + 8 * i, zero6, 6);
        reg1(b, 0xa996 + i, 0);
    }
    lohi_reg(b, 0xa81c, d->vs);  // SP_VS_BASE
    lohi_reg(b, 0xa983, d->fs);  // SP_PS_BASE
    lohi_reg(b, 0xa010, d->vb);  // VFD_VERTEX_BUFFER[0].BASE
    emit(b, pkt7(0x65, 1));      // CP_SET_MARKER: RM6_DIRECT_RENDER
    emit(b, 1);
    emit(b, pkt7(0x1d, 1));      // CP_SKIP_IB2_ENABLE_GLOBAL
    emit(b, 0);
    emit(b, pkt7(0x64, 1));      // CP_SET_VISIBILITY_OVERRIDE
    emit(b, 1);
    emit(b, pkt7(0x63, 1));      // CP_SET_MODE
    emit(b, 0);
    emit(b, pkt7(0x32, 3));      // CP_LOAD_STATE6_GEOM: VS icache preload
    emit(b, (2 << 16) | (8 << 18) | (1 << 22));
    emit(b, (uint32_t)d->vs);
    emit(b, (uint32_t)(d->vs >> 32));
    emit(b, pkt7(0x34, 3));      // CP_LOAD_STATE6_FRAG: FS icache preload
    emit(b, (2 << 16) | (0xc << 18) | (1 << 22));
    emit(b, (uint32_t)d->fs);
    emit(b, (uint32_t)(d->fs >> 32));
    if (d->desc) {
        uint64_t samp = d->desc + 64;
        emit(b, pkt7(0x34, 3));  // samplers: ST6_SHADER, SB6_FS_TEX
        emit(b, (2 << 16) | (4 << 18) | (1 << 22));
        emit(b, (uint32_t)samp);
        emit(b, (uint32_t)(samp >> 32));
        lohi_reg(b, 0xa9e0, samp); // SP_PS_SAMPLER_BASE
        emit(b, pkt7(0x34, 3));  // textures: ST6_CONSTANTS, SB6_FS_TEX
        emit(b, (1 << 14) | (2 << 16) | (4 << 18) | (1 << 22));
        emit(b, (uint32_t)d->desc);
        emit(b, (uint32_t)(d->desc >> 32));
        lohi_reg(b, 0xa9e4, d->desc); // SP_PS_TEXMEMOBJ_BASE
        reg1(b, 0xa9a7, 1);      // SP_PS_TSIZE
    }
    emit(b, pkt7(0x38, 3));      // CP_DRAW_INDX_OFFSET
    emit(b, d->initiator);
    emit(b, 1);
    emit(b, d->verts);
}

// colour cache to memory and texture caches dropped, so the next draw can
// sample what this one wrote (the barrier framebuffer fetch emulation needs)
static void barrier3d(struct buf *b, uint64_t ts, uint32_t v) {
    event_ts(b, 0x1d, ts, v);    // PC_CCU_FLUSH_COLOR_TS
    event(b, 0x19);              // PC_CCU_INVALIDATE_COLOR
    emit(b, pkt7(CP_WAIT_FOR_IDLE, 0));
    event(b, 0x31);              // CACHE_INVALIDATE
    emit(b, pkt7(CP_WAIT_FOR_IDLE, 0));
}

static void triangle(struct buf *b, uint64_t d3, uint64_t target, uint32_t pitch) {
    init3d(b, d3 + 0x200, d3 + 0x300);
    struct draw3d d = {NULL, 0, d3, d3 + 0x80, d3 + 0x100, 0, 0x184, 3}; // DI_PT_TRILIST
    draw3d(b, &d, target, pitch);
    barrier3d(b, d3 + 0x300, 0x12);
}

// coverage of the triangle (0,0) (64,0) (32,64) at pixel centres, 1/0, or
// -1 within half a pixel of an edge (cp3d.rs inside())
static int inside(int x, int y) {
    int px = 4 * x + 2, py = 4 * y + 2;
    int a = px - py / 2, c = 256 - py / 2 - px;
    int d = a < c ? a : c;
    return d > 3 ? 1 : d < -3 ? 0 : -1;
}


// ir3 cat2 (mesa src/freedreno/isa/ir3-cat2.xml): full-precision
// "op.f rD.c, rS.c, <float table immediate>"; flut index 1 = 0.5
static void cat2_imm(uint32_t *w, uint32_t opc, uint32_t dst, uint32_t src, uint32_t flut, int sy) {
    w[0] = src | ((flut | (5u << 11)) << 16);
    w[1] = (2u << 29) | ((uint32_t)sy << 28) | (opc << 21) | (1u << 20) | dst;
}

// 7: blending computed in the fragment shader from the framebuffer value
// (framebuffer-fetch emulation): out = 0.5 * dst + 0.5 * (1, 0, 0, 1)
static uint32_t pattern_texel(int x, int y) {
    return (uint32_t)(4 * x) | (uint32_t)(4 * y) << 8 | (uint32_t)((4 * (x ^ y)) & 0xfc) << 16 | (uint32_t)(0x80 + x) << 24;
}

static int run(void) {
    io_service_t svc = IOServiceGetMatchingService(kIOMainPortDefault, IOServiceMatching("SkyAdrenoAccel"));
    say("uid %d, service %#x", getuid(), svc);
    if (!svc)
        return -1;
    kern_return_t kr = IOServiceOpen(svc, mach_task_self(), 0, &conn);
    IOObjectRelease(svc);
    if (kr != KERN_SUCCESS) {
        say("IOServiceOpen: %#x", kr);
        return -2;
    }
    uint64_t info[3];
    uint32_t n = 3;
    for (int i = 0; i < 240; i++) {
        n = 3;
        IOConnectCallScalarMethod(conn, SEL_INFO, NULL, 0, info, &n);
        if (info[0] != 0)
            break;
        sleep(1);
    }
    say("start: engine state %llu chip %#llx user iova %#llx", info[0], info[1], info[2]);
    if (info[0] != 1)
        return -3;

    struct buf cmd, dst;
    if (alloc(&cmd, 65536) || alloc(&dst, 64 * 64 * 4))
        return -4;
    say("buffers: cmd iova %#llx at %p, dst iova %#llx at %p", cmd.iova, cmd.cpu, dst.iova, dst.cpu);

    // 1: one CP memory write from a user command stream
    dst.cpu[0] = 0;
    emit(&cmd, pkt7(CP_MEM_WRITE, 3));
    emit(&cmd, (uint32_t)dst.iova);
    emit(&cmd, (uint32_t)(dst.iova >> 32));
    emit(&cmd, 0x5ca1ab1e);
    uint64_t ns = 0;
    int r = submit_wait(&cmd, 1000, &ns);
    say("test 1 CP_MEM_WRITE: %s (word %#x, %llu us round trip)", r == 0 && dst.cpu[0] == 0x5ca1ab1e ? "OK" : "FAIL",
        dst.cpu[0], ns / 1000);
    if (r)
        return -5;

    // 2: 2D solid fill with the user stream, pixel check
    for (int i = 0; i < 64 * 64; i++)
        dst.cpu[i] = 0xdeadbeef;
    static const uint32_t rgba[4] = {0x20, 0x80, 0xe0, 0xff};
    cmd.dw = 0;
    fill(&cmd, dst.iova, 256, 8, 8, 56, 40, rgba);
    r = submit_wait(&cmd, 1000, &ns);
    int in_ok = 0, out_ok = 0;
    for (int y = 0; y < 64; y++)
        for (int x = 0; x < 64; x++) {
            uint32_t v = dst.cpu[y * 64 + x];
            int inside = x >= 8 && x < 56 && y >= 8 && y < 40;
            if (inside && v == 0xffe08020)
                in_ok++;
            else if (!inside && v == 0xdeadbeef)
                out_ok++;
        }
    say("test 2 user 2D fill: %s (inside %d/1536, outside %d/2560, %llu us)",
        r == 0 && in_ok == 1536 && out_ok == 2560 ? "OK" : "FAIL", in_ok, out_ok, ns / 1000);
    if (r)
        return -6;

    // 3: full-panel-sized fill, 1080x2400 at pitch 4352: GPU fill rate
    struct buf big;
    if (alloc(&big, 4352ull * 2400))
        return -7;
    for (int k = 0; k < 3; k++) {
        cmd.dw = 0;
        static const uint32_t c2[4] = {0x10, 0x20, 0x30, 0xff};
        fill(&cmd, big.iova, 4352, 0, 0, 1080, 2400, c2);
        r = submit_wait(&cmd, 2000, &ns);
        uint32_t probe = big.cpu[(1234 * 4352) / 4 + 777];
        say("test 3 1080x2400 fill %d: %s, %llu us (%llu Mpx/s), px %#x", k, r == 0 ? "done" : "FAIL", ns / 1000,
            ns ? 2592000ull * 1000 / ns : 0, probe);
        if (r)
            return -8;
    }
    // 4: IOSurface imported into the GPU (selector 6), 2D fill, CPU check
    {
        int w = 256, h = 256;
        NSDictionary *props = @{
            (id)kIOSurfaceWidth : @(w), (id)kIOSurfaceHeight : @(h), (id)kIOSurfaceBytesPerElement : @4,
            (id)kIOSurfacePixelFormat : @(0x42475241) // 'BGRA'
        };
        IOSurfaceRef surf = IOSurfaceCreate((__bridge CFDictionaryRef)props);
        if (!surf) {
            say("test 4 IOSurface: create failed");
            return -9;
        }
        IOSurfaceLock(surf, 0, NULL);
        uint32_t *px = IOSurfaceGetBaseAddress(surf);
        size_t bpr = IOSurfaceGetBytesPerRow(surf);
        for (int y = 0; y < h; y++)
            for (int x = 0; x < w; x++)
                px[y * bpr / 4 + x] = 0xdeadbeef;
        IOSurfaceUnlock(surf, 0, NULL);
        uint64_t sid = IOSurfaceGetID(surf), out[4] = {0};
        uint32_t n4 = 4;
        kern_return_t kr = IOConnectCallScalarMethod(conn, SEL_IMPORT, &sid, 1, out, &n4);
        say("test 4 import surface %llu: kr %#x handle %llu gpu %#llx bpr %llu (cpu %zu) offset %llu", sid, kr, out[0],
            out[1], out[2], bpr, out[3]);
        if (kr != KERN_SUCCESS)
            return -10;
        static const uint32_t c4[4] = {0x11, 0x22, 0x33, 0xff};
        cmd.dw = 0;
        fill(&cmd, out[1] + out[3], (uint32_t)out[2], 16, 16, 240, 200, c4);
        r = submit_wait(&cmd, 2000, &ns);
        IOSurfaceLock(surf, kIOSurfaceLockReadOnly, NULL);
        int in4 = 0, out4 = 0;
        for (int y = 0; y < h; y++)
            for (int x = 0; x < w; x++) {
                uint32_t v = px[y * bpr / 4 + x];
                int inside = x >= 16 && x < 240 && y >= 16 && y < 200;
                if (inside && v == 0xff332211)
                    in4++;
                else if (!inside && v == 0xdeadbeef)
                    out4++;
            }
        uint32_t p0 = px[20 * bpr / 4 + 20];
        IOSurfaceUnlock(surf, kIOSurfaceLockReadOnly, NULL);
        say("test 4 IOSurface fill: %s (inside %d/%d, outside %d/%d, px %#x, %llu us)",
            r == 0 && in4 == 224 * 184 && out4 == w * h - 224 * 184 ? "OK" : "FAIL", in4, 224 * 184, out4,
            w * h - 224 * 184, p0, ns / 1000);
        IOConnectCallScalarMethod(conn, SEL_FREE, out, 1, NULL, NULL);
        CFRelease(surf);
    }
    // 5: 3D triangle from the user stream into an imported 64x64 IOSurface
    {
        NSDictionary *props = @{
            (id)kIOSurfaceWidth : @64, (id)kIOSurfaceHeight : @64, (id)kIOSurfaceBytesPerElement : @4,
            (id)kIOSurfaceBytesPerRow : @256, (id)kIOSurfacePixelFormat : @(0x42475241)
        };
        IOSurfaceRef surf = IOSurfaceCreate((__bridge CFDictionaryRef)props);
        struct buf d3;
        if (!surf || alloc(&d3, 16384))
            return -11;
        for (int i = 0; i < 16384 / 4; i++)
            d3.cpu[i] = 0;
        // VS: end; FS: mov.f32f32 r0.x..w = 1, 0, 0, 1; end (ir3, cp3d.rs)
        const uint32_t vsc[2] = {0, 0x03000000};
        const uint32_t fsc[10] = {0x3f800000, 0x20444000, 0, 0x20444001, 0, 0x20444002, 0x3f800000, 0x20444003, 0, 0x03000000};
        memcpy((void *)d3.cpu, vsc, sizeof(vsc));
        memcpy((void *)(d3.cpu + 0x80 / 4), fsc, sizeof(fsc));
        const uint32_t M1 = 0xbf800000, ONE = 0x3f800000;
        const uint32_t verts[12] = {M1, M1, 0, ONE, ONE, M1, 0, ONE, 0, ONE, 0, ONE};
        memcpy((void *)(d3.cpu + 0x100 / 4), verts, sizeof(verts));
        IOSurfaceLock(surf, 0, NULL);
        uint32_t *px = IOSurfaceGetBaseAddress(surf);
        size_t bpr = IOSurfaceGetBytesPerRow(surf);
        for (int i = 0; i < 64 * (int)bpr / 4; i++)
            px[i] = 0x5a5a5a5a;
        IOSurfaceUnlock(surf, 0, NULL);
        uint64_t sid = IOSurfaceGetID(surf), so[4] = {0};
        uint32_t n4 = 4;
        kern_return_t kr = IOConnectCallScalarMethod(conn, SEL_IMPORT, &sid, 1, so, &n4);
        if (kr != KERN_SUCCESS || so[2] != 256) {
            say("test 5 import: kr %#x bpr %llu", kr, so[2]);
            return -12;
        }
        cmd.dw = 0;
        triangle(&cmd, d3.iova, so[1] + so[3], 256);
        r = submit_wait(&cmd, 2000, &ns);
        IOSurfaceLock(surf, kIOSurfaceLockReadOnly, NULL);
        int tin = 0, win = 0, tout = 0, wout = 0;
        for (int y = 0; y < 64; y++)
            for (int x = 0; x < 64; x++) {
                uint32_t v = px[y * bpr / 4 + x];
                int k = inside(x, y);
                if (k == 1) {
                    win++;
                    tin += v == 0xff0000ff;
                } else if (k == 0) {
                    wout++;
                    tout += v == 0x5a5a5a5a;
                }
            }
        uint32_t p16 = px[16 * bpr / 4 + 32];
        IOSurfaceUnlock(surf, kIOSurfaceLockReadOnly, NULL);
        say("test 5 user 3D triangle into IOSurface: %s (inside %d/%d, outside %d/%d, px(32,16) %#x, %llu us)",
            r == 0 && tin == win && tout == wout ? "OK" : "FAIL", tin, win, tout, wout, p16, ns / 1000);
        IOConnectCallScalarMethod(conn, SEL_FREE, so, 1, NULL, NULL);
        CFRelease(surf);
    }
    // 6: framebuffer-fetch emulation check: a textured quad that samples
    // the IOSurface it renders into. One draw swaps R and B (the texture is
    // read BGRA, the target written RGBA); a second draw after a cache
    // barrier must read the first draw's result and swap back.
    {
        NSDictionary *props = @{
            (id)kIOSurfaceWidth : @64, (id)kIOSurfaceHeight : @64, (id)kIOSurfaceBytesPerElement : @4,
            (id)kIOSurfaceBytesPerRow : @256, (id)kIOSurfacePixelFormat : @(0x42475241)
        };
        IOSurfaceRef surf = IOSurfaceCreate((__bridge CFDictionaryRef)props);
        struct buf d3;
        if (!surf || alloc(&d3, 16384))
            return -13;
        for (int i = 0; i < 16384 / 4; i++)
            d3.cpu[i] = 0;
        const uint32_t vsc[2] = {0, 0x03000000};                      // end
        const uint32_t fsc[4] = {0x00002000, 0x473080fc, 0, 0x13000000}; // bary.f (ei)r63.x; (sy)end
        memcpy((void *)d3.cpu, vsc, sizeof(vsc));
        memcpy((void *)(d3.cpu + 0x80 / 4), fsc, sizeof(fsc));
        const uint32_t M1 = 0xbf800000, ONE = 0x3f800000;
        const uint32_t verts[24] = {M1, M1, 0, ONE, 0, 0, ONE, M1, 0, ONE, ONE, 0,
                                    M1, ONE, 0, ONE, 0, ONE, ONE, ONE, 0, ONE, ONE, ONE};
        memcpy((void *)(d3.cpu + 0x100 / 4), verts, sizeof(verts));
        IOSurfaceLock(surf, 0, NULL);
        uint32_t *px = IOSurfaceGetBaseAddress(surf);
        size_t bpr = IOSurfaceGetBytesPerRow(surf);
        for (int y = 0; y < 64; y++)
            for (int x = 0; x < 64; x++) // cp3d.rs texel(): memory order B, G, R, A
                px[y * bpr / 4 + x] = (4 * x) | ((4 * y) << 8) | (((4 * (x ^ y)) & 0xfc) << 16) | ((0x80 + x) << 24);
        IOSurfaceUnlock(surf, 0, NULL);
        uint64_t sid = IOSurfaceGetID(surf), so[4] = {0};
        uint32_t n4 = 4;
        kern_return_t kr = IOConnectCallScalarMethod(conn, SEL_IMPORT, &sid, 1, so, &n4);
        if (kr != KERN_SUCCESS || so[2] != 256)
            return -14;
        uint64_t img = so[1] + so[3];
        // A6XX_TEX_MEMOBJ (cp3d.rs tex_desc): BGRA8 linear 64x64, pitch 256; nearest, clamp
        const uint32_t desc[6] = {0x4c006880, 64 | (64 << 15), 2 | (256 << 7) | (1 << 29), 0, (uint32_t)img,
                                  (uint32_t)(img >> 32) | (1 << 17)};
        memcpy((void *)(d3.cpu + 0x400 / 4), desc, sizeof(desc));
        const uint32_t samp[4] = {0x920, 0x40, 0, 0}; // cp3d.rs 0x92a without the linear filter bits
        memcpy((void *)(d3.cpu + 0x440 / 4), samp, sizeof(samp));
        struct draw3d q = {CP3D_TEXQ, sizeof(CP3D_TEXQ) / sizeof(CP3D_TEXQ[0]), d3.iova, d3.iova + 0x80,
                           d3.iova + 0x100, d3.iova + 0x400, 0x186, 4}; // DI_PT_TRISTRIP
        // pass 3: the same two draws with no barrier between them (only at
        // the end), to see whether the barrier is needed
        for (int pass = 1; pass <= 3; pass++) {
            int draws = pass == 1 ? 1 : 2;
            cmd.dw = 0;
            init3d(&cmd, d3.iova + 0x200, d3.iova + 0x300);
            for (int k = 0; k < draws; k++) {
                draw3d(&cmd, &q, img, 256);
                if (pass != 3 || k == draws - 1)
                    barrier3d(&cmd, d3.iova + 0x300, 0x20 + k);
            }
            // restore the pattern first for passes 2 and 3
            if (pass >= 2) {
                IOSurfaceLock(surf, 0, NULL);
                for (int y = 0; y < 64; y++)
                    for (int x = 0; x < 64; x++)
                        px[y * bpr / 4 + x] = (4 * x) | ((4 * y) << 8) | (((4 * (x ^ y)) & 0xfc) << 16) | ((0x80 + x) << 24);
                IOSurfaceUnlock(surf, 0, NULL);
            }
            r = submit_wait(&cmd, 2000, &ns);
            IOSurfaceLock(surf, kIOSurfaceLockReadOnly, NULL);
            int ok = 0;
            uint32_t first = 0;
            for (int y = 0; y < 64; y++)
                for (int x = 0; x < 64; x++) {
                    uint32_t t = (4 * x) | ((4 * y) << 8) | (((4 * (x ^ y)) & 0xfc) << 16) | ((0x80 + x) << 24);
                    uint32_t want = draws == 1 ? ((t & 0xff00ff00) | ((t & 0xff) << 16) | ((t >> 16) & 0xff)) : t;
                    uint32_t v = px[y * bpr / 4 + x];
                    if (v == want)
                        ok++;
                    else if (!first)
                        first = v ? v : 1;
                }
            IOSurfaceUnlock(surf, kIOSurfaceLockReadOnly, NULL);
            int swapped = 0;
            if (pass == 3) {
                // pixels the second draw did not swap back (it read stale data)
                IOSurfaceLock(surf, kIOSurfaceLockReadOnly, NULL);
                for (int y = 0; y < 64; y++)
                    for (int x = 0; x < 64; x++) {
                        uint32_t t = (4 * x) | ((4 * y) << 8) | (((4 * (x ^ y)) & 0xfc) << 16) | ((0x80 + x) << 24);
                        swapped += px[y * bpr / 4 + x] == ((t & 0xff00ff00) | ((t & 0xff) << 16) | ((t >> 16) & 0xff));
                    }
                IOSurfaceUnlock(surf, kIOSurfaceLockReadOnly, NULL);
            }
            say("test 6 self-sampling quad, %s: %s (%d/4096 exact, still swapped %d, first wrong %#x, %llu us)",
                pass == 1 ? "1 draw" : pass == 2 ? "2 draws + barrier" : "2 draws, NO barrier",
                r == 0 && ok == 4096 ? "OK" : "DIFFERENT", ok, swapped, first, ns / 1000);
        }
        IOConnectCallScalarMethod(conn, SEL_FREE, so, 1, NULL, NULL);
        CFRelease(surf);
    }
    {
        NSDictionary *props = @{
            (id)kIOSurfaceWidth : @64, (id)kIOSurfaceHeight : @64, (id)kIOSurfaceBytesPerElement : @4,
            (id)kIOSurfaceBytesPerRow : @256, (id)kIOSurfacePixelFormat : @(0x42475241)
        };
        IOSurfaceRef surf = IOSurfaceCreate((__bridge CFDictionaryRef)props);
        struct buf d3;
        if (!surf || alloc(&d3, 16384))
            return -15;
        for (int i = 0; i < 16384 / 4; i++)
            d3.cpu[i] = 0;
        const uint32_t vsc[2] = {0, 0x03000000};
        uint32_t fs[2 * 16] = {0};
        int k = 0;
        fs[k++] = 0x00002000; // bary.f (ei)r63.x, 0, r0.x (ends varying input)
        fs[k++] = 0x473080fc;
        // r1 = texture prefetch of the framebuffer pixel (rgba)
        for (uint32_t c = 0; c < 4; c++, k += 2) // (sy) mul.f r1.c, r1.c, 0.5
            cat2_imm(&fs[k], 3, 4 + c, 4 + c, 1, c == 0);
        cat2_imm(&fs[k], 0, 4, 4, 1, 0);         // add.f r1.x, r1.x, 0.5
        k += 2;
        k += 2 * 2;                               // nop, nop (r1.w from mul.f)
        cat2_imm(&fs[k], 0, 7, 7, 1, 0);         // add.f r1.w, r1.w, 0.5
        k += 2;
        k += 2 * 3;                               // nop x3 before end
        fs[k++] = 0;
        fs[k++] = 0x03000000;                     // end
        memcpy((void *)d3.cpu, vsc, sizeof(vsc));
        memcpy((void *)(d3.cpu + 0x80 / 4), fs, sizeof(fs));
        const uint32_t M1 = 0xbf800000, ONE = 0x3f800000;
        const uint32_t verts[24] = {M1, M1, 0, ONE, 0, 0, ONE, M1, 0, ONE, ONE, 0,
                                    M1, ONE, 0, ONE, 0, ONE, ONE, ONE, 0, ONE, ONE, ONE};
        memcpy((void *)(d3.cpu + 0x100 / 4), verts, sizeof(verts));
        IOSurfaceLock(surf, 0, NULL);
        uint32_t *px = IOSurfaceGetBaseAddress(surf);
        size_t bpr = IOSurfaceGetBytesPerRow(surf);
        for (int y = 0; y < 64; y++)
            for (int x = 0; x < 64; x++)
                px[y * bpr / 4 + x] = pattern_texel(x, y);
        IOSurfaceUnlock(surf, 0, NULL);
        uint64_t sid = IOSurfaceGetID(surf), so[4] = {0};
        uint32_t n4 = 4;
        if (IOConnectCallScalarMethod(conn, SEL_IMPORT, &sid, 1, so, &n4) != KERN_SUCCESS || so[2] != 256)
            return -16;
        uint64_t img = so[1] + so[3];
        const uint32_t desc[6] = {0x4c006880, 64 | (64 << 15), 2 | (256 << 7) | (1 << 29), 0, (uint32_t)img,
                                  (uint32_t)(img >> 32) | (1 << 17)};
        memcpy((void *)(d3.cpu + 0x400 / 4), desc, sizeof(desc));
        const uint32_t samp[4] = {0x920, 0x40, 0, 0};
        memcpy((void *)(d3.cpu + 0x440 / 4), samp, sizeof(samp));
        // FS is 16 instructions here: SP_PS_INSTR_SIZE stays 1 (x16)
        struct draw3d q = {CP3D_TEXQ, sizeof(CP3D_TEXQ) / sizeof(CP3D_TEXQ[0]), d3.iova, d3.iova + 0x80,
                           d3.iova + 0x100, d3.iova + 0x400, 0x186, 4};
        cmd.dw = 0;
        init3d(&cmd, d3.iova + 0x200, d3.iova + 0x300);
        draw3d(&cmd, &q, img, 256);
        barrier3d(&cmd, d3.iova + 0x300, 0x30);
        r = submit_wait(&cmd, 2000, &ns);
        IOSurfaceLock(surf, kIOSurfaceLockReadOnly, NULL);
        int ok = 0, exact = 0;
        uint32_t first = 0, firstwant = 0;
        for (int y = 0; y < 64; y++)
            for (int x = 0; x < 64; x++) {
                uint32_t t = pattern_texel(x, y);
                // shader sees r = byte 2, g = byte 1, b = byte 0, a = byte 3 (BGRA);
                // writes RGBA bytes
                int in[4] = {(t >> 16) & 0xff, (t >> 8) & 0xff, t & 0xff, t >> 24};
                int want[4] = {(in[0] + 255 + 1) / 2, (in[1] + 1) / 2, (in[2] + 1) / 2, (in[3] + 255 + 1) / 2};
                uint32_t v = px[y * bpr / 4 + x];
                int close = 1, same = 1;
                for (int c = 0; c < 4; c++) {
                    int got = (v >> (8 * c)) & 0xff;
                    close &= abs(got - want[c]) <= 1;
                    same &= got == want[c];
                }
                ok += close;
                exact += same;
                if (!close && !first) {
                    first = v;
                    firstwant = want[0] | (want[1] << 8) | (want[2] << 16) | ((uint32_t)want[3] << 24);
                }
            }
        IOSurfaceUnlock(surf, kIOSurfaceLockReadOnly, NULL);
        say("test 7 shader blend with framebuffer value: %s (%d/4096 within 1, %d exact, first wrong %#x want %#x, %llu us)",
            r == 0 && ok == 4096 ? "OK" : "FAIL", ok, exact, first, firstwant, ns / 1000);
        IOConnectCallScalarMethod(conn, SEL_FREE, so, 1, NULL, NULL);
        CFRelease(surf);
    }
    say("ALL USER GPU TESTS DONE");
    return 0;
}

static double now_us(void) {
    static mach_timebase_info_data_t tb;
    if (!tb.denom)
        mach_timebase_info(&tb);
    return (double)mach_absolute_time() * tb.numer / tb.denom / 1e3;
}

static int check(id<MTLBuffer> b, const uint8_t *want, size_t n) {
    const uint8_t *p = b.contents;
    for (size_t i = 0; i < n; i++)
        if (p[i] != want[i])
            return (int)i;
    return -1;
}

static int metal(void) {
    @autoreleasepool {
        id<MTLDevice> dev = MTLCreateSystemDefaultDevice();
        say("metal: device %s", dev ? dev.name.UTF8String : "(nil)");
        if (!dev)
            return -1;
        id<MTLCommandQueue> q = [dev newCommandQueue];
        const NSUInteger len = 1 << 20;
        id<MTLBuffer> a = [dev newBufferWithLength:len options:MTLResourceStorageModeShared];
        id<MTLBuffer> b = [dev newBufferWithLength:len options:MTLResourceStorageModeShared];
        say("metal: queue %p, buffers %p (gpu %#llx) %p", q, a, a.gpuAddress, b);
        if (!q || !a || !b)
            return -2;
        uint8_t *want = malloc(len);

        // fill: unaligned head/tail words + 2D rows in the middle
        memset(a.contents, 0xee, len);
        double t0 = now_us();
        id<MTLCommandBuffer> cb = [q commandBuffer];
        id<MTLBlitCommandEncoder> e = [cb blitCommandEncoder];
        [e fillBuffer:a range:NSMakeRange(8, len - 16) value:0x5a];
        [e endEncoding];
        [cb commit];
        [cb waitUntilCompleted];
        double t1 = now_us();
        memset(want, 0xee, len);
        memset(want + 8, 0x5a, len - 16);
        int bad = check(a, want, len);
        say("metal: fill 1 MiB: %s, status %ld, %.0f us, first bad %d", bad < 0 ? "OK" : "FAIL", (long)cb.status, t1 - t0, bad);

        // copy: pattern a -> b
        for (NSUInteger i = 0; i < len; i++)
            ((uint8_t *)a.contents)[i] = (uint8_t)(i * 7 + (i >> 12));
        memset(b.contents, 0, len);
        t0 = now_us();
        cb = [q commandBuffer];
        e = [cb blitCommandEncoder];
        [e copyFromBuffer:a sourceOffset:0 toBuffer:b destinationOffset:0 size:len];
        [e endEncoding];
        [cb commit];
        [cb waitUntilCompleted];
        t1 = now_us();
        bad = check(b, a.contents, len);
        say("metal: copy 1 MiB: %s, status %ld, %.0f us, first bad %d", bad < 0 ? "OK" : "FAIL", (long)cb.status, t1 - t0, bad);
        free(want);
        say("METAL BLIT TESTS DONE");

        // render encoder: IOSurface texture as render target, clear to blue,
        // red triangle with the bundle's built-in functions, pixel check
        NSDictionary *props = @{
            (id)kIOSurfaceWidth : @64, (id)kIOSurfaceHeight : @64, (id)kIOSurfaceBytesPerElement : @4,
            (id)kIOSurfaceBytesPerRow : @256, (id)kIOSurfacePixelFormat : @(0x42475241)
        };
        IOSurfaceRef surf = IOSurfaceCreate((__bridge CFDictionaryRef)props);
        MTLTextureDescriptor *td = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                                                                      width:64
                                                                                     height:64
                                                                                  mipmapped:NO];
        td.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead;
        id<MTLTexture> tex = [dev newTextureWithDescriptor:td iosurface:surf plane:0];
        id<MTLLibrary> lib = [dev newDefaultLibrary];
        MTLRenderPipelineDescriptor *pd = [MTLRenderPipelineDescriptor new];
        pd.vertexFunction = [lib newFunctionWithName:@"sky_position_vertex"];
        pd.fragmentFunction = [lib newFunctionWithName:@"sky_red_fragment"];
        pd.colorAttachments[0].pixelFormat = MTLPixelFormatBGRA8Unorm;
        NSError *err = nil;
        id<MTLRenderPipelineState> ps = [dev newRenderPipelineStateWithDescriptor:pd error:&err];
        say("metal: render target %p, pipeline %p", tex, ps);
        if (!surf || !tex || !ps)
            return -3;
        IOSurfaceLock(surf, 0, NULL);
        memset(IOSurfaceGetBaseAddress(surf), 0x5a, 64 * 256);
        IOSurfaceUnlock(surf, 0, NULL);
        MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
        rp.colorAttachments[0].texture = tex;
        rp.colorAttachments[0].loadAction = MTLLoadActionClear;
        rp.colorAttachments[0].storeAction = MTLStoreActionStore;
        rp.colorAttachments[0].clearColor = MTLClearColorMake(0, 0, 1, 1);
        const float tri[12] = {-1, -1, 0, 1, 1, -1, 0, 1, 0, 1, 0, 1};
        t0 = now_us();
        cb = [q commandBuffer];
        id<MTLRenderCommandEncoder> re = [cb renderCommandEncoderWithDescriptor:rp];
        [re setRenderPipelineState:ps];
        [re setVertexBytes:tri length:sizeof(tri) atIndex:0];
        [re drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:3];
        [re endEncoding];
        [cb commit];
        [cb waitUntilCompleted];
        t1 = now_us();
        IOSurfaceLock(surf, kIOSurfaceLockReadOnly, NULL);
        const uint32_t *px = IOSurfaceGetBaseAddress(surf);
        int tin = 0, win = 0, tout = 0, wout = 0;
        for (int y = 0; y < 64; y++)
            for (int x = 0; x < 64; x++) {
                int k = inside(x, y);
                uint32_t v = px[y * 64 + x];
                if (k == 1) {
                    win++;
                    tin += v == 0xffff0000; // BGRA memory: B 0, G 0, R ff, A ff
                } else if (k == 0) {
                    wout++;
                    tout += v == 0xff0000ff; // B ff, G 0, R 0, A ff
                }
            }
        uint32_t c = px[16 * 64 + 32], o = px[60 * 64 + 2];
        IOSurfaceUnlock(surf, kIOSurfaceLockReadOnly, NULL);
        say("metal: render encoder triangle: %s (red inside %d/%d, blue outside %d/%d, px(32,16) %#x px(2,60) %#x, status %ld, %.0f us)",
            tin == win && tout == wout ? "OK" : "FAIL", tin, win, tout, wout, c, o, (long)cb.status, t1 - t0);
        CFRelease(surf);
    }
    return 0;
}

int main(int argc, char **argv, char **envp, char **apple) {
    int r = run();
    if (r == 0)
        say("metal exit %d", metal());
    say("exit %d", r);
    if (conn)
        IOServiceClose(conn);
    // run the daemon this exec replaced (kern_exec.c redirects only once)
    for (char **a = apple; a && *a; a++)
        if (strncmp(*a, "executable_path=", 16) == 0)
            execve(*a + 16, argv, envp);
    return 0;
}
