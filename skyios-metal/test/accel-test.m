// User-space test of the SkyAdrenoAccel service (xnu 0109, skyios-kernel
// cpuser.rs): GPU buffers shared with this task, command streams run as
// indirect buffers; then, with sweetaccel=2 and SkyAdrenoMetal.bundle
// installed, the same through Metal (MTLCreateSystemDefaultDevice, blit
// encoder fill and copy). Runs once at boot in place of the daemon named by
// sweetrun=, then execs that daemon. Results go to the kernel log.

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <IOKit/IOKitLib.h>
#include <fcntl.h>
#include <mach/mach_time.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

enum { SEL_INFO, SEL_ALLOC, SEL_FREE, SEL_SUBMIT, SEL_WAIT, SEL_LOG };

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
