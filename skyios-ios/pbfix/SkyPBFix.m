// Inserted into SpringBoard (DYLD_INSERT_LIBRARIES in its launchd job,
// posterwall "inject" mode; needs boot-arg sweetdyldvars=1, xnu 0113).
// PaperBoardUI renders wallpaper snapshots with -[PBUICALocalRenderer
// renderRequest:error:] through CoreImage or a CoreAnimation renderer into a
// Metal texture; SpringBoard has no Metal device on this port, the result is
// nil and SpringBoard traps in -[PBUIImageEncoder saveUIImage:] (0112s,
// 0112z, 0113j). For image requests (PBUITreatImageRequest: CGImage +
// downscaleFactor) this draws the image into a BGRA IOSurface on the CPU
// instead; the wallpaper treatment (dimming) is not applied.

#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>
#import <IOSurface/IOSurfaceRef.h>
#import <objc/message.h>
#import <objc/runtime.h>
#include <mach-o/dyld.h>
#include <fcntl.h>
#include <unistd.h>

static void say(const char *fmt, ...) {
    char text[200];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(text, sizeof(text), fmt, ap);
    va_end(ap);
    // SpringBoard runs as mobile and cannot open /dev/console: append to a
    // file that posterwall (readlog mode, root) prints to the kernel log
    int fd = open("/private/var/mobile/Library/Logs/SkyPBFix.log", O_WRONLY | O_CREAT | O_APPEND, 0644);
    if (fd >= 0) {
        dprintf(fd, "[%d] %s\n", getpid(), text);
        close(fd);
    }
}

typedef id (*render_imp)(id, SEL, id, NSError **);
static render_imp orig_local, orig_remote;

static id cpu_render(id request) {
    SEL simg = NSSelectorFromString(@"image"), sdown = NSSelectorFromString(@"downscaleFactor");
    if (![request respondsToSelector:simg])
        return nil;
    CGImageRef img = ((CGImageRef(*)(id, SEL))objc_msgSend)(request, simg);
    if (!img)
        return nil;
    double down = [request respondsToSelector:sdown] ? ((double (*)(id, SEL))objc_msgSend)(request, sdown) : 1;
    if (!(down >= 1))
        down = 1;
    size_t w = (size_t)(CGImageGetWidth(img) / down), h = (size_t)(CGImageGetHeight(img) / down);
    if (!w || !h)
        return nil;
    NSDictionary *props = @{
        (id)kIOSurfaceWidth : @(w), (id)kIOSurfaceHeight : @(h), (id)kIOSurfaceBytesPerElement : @4,
        (id)kIOSurfacePixelFormat : @(0x42475241) // 'BGRA'
    };
    IOSurfaceRef s = IOSurfaceCreate((__bridge CFDictionaryRef)props);
    if (!s)
        return nil;
    IOSurfaceLock(s, 0, NULL);
    CGColorSpaceRef cs = CGColorSpaceCreateDeviceRGB();
    CGContextRef c = CGBitmapContextCreate(IOSurfaceGetBaseAddress(s), w, h, 8, IOSurfaceGetBytesPerRow(s), cs,
                                           kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little);
    if (c) {
        CGContextDrawImage(c, CGRectMake(0, 0, w, h), img);
        CGContextRelease(c);
    }
    CGColorSpaceRelease(cs);
    IOSurfaceUnlock(s, 0, NULL);
    return (__bridge_transfer id)s;
}

static id local_render(id self, SEL _cmd, id request, NSError **err) {
    id r = orig_local(self, _cmd, request, err);
    if (r)
        return r;
    r = cpu_render(request);
    static int n;
    if (n++ < 8)
        say("local renderer gave nil for %s, CPU render -> %p", object_getClassName(request), (__bridge void *)r);
    return r;
}

static id remote_render(id self, SEL _cmd, id request, NSError **err) {
    id r = orig_remote(self, _cmd, request, err);
    return r ?: cpu_render(request);
}

static void try_hook(void) {
    static int done;
    if (done || !NSClassFromString(@"PBUICALocalRenderer"))
        return;
    done = 1;
    Method m = class_getInstanceMethod(NSClassFromString(@"PBUICALocalRenderer"), NSSelectorFromString(@"renderRequest:error:"));
    if (m)
        orig_local = (render_imp)method_setImplementation(m, (IMP)local_render);
    Method r = class_getInstanceMethod(NSClassFromString(@"PBUICARemoteRenderer"), NSSelectorFromString(@"renderRequest:error:"));
    if (r)
        orig_remote = (render_imp)method_setImplementation(r, (IMP)remote_render);
    say("hooked renderRequest:error: local %d remote %d", m != NULL, r != NULL);
}

// inserted libraries load before the main executable's dependencies
// (PaperBoardUI), so hook when that image is added
static void added(const struct mach_header *mh, intptr_t slide) {
    try_hook();
}

__attribute__((constructor)) static void sky_pbfix_init(void) {
    say("loaded");
    _dyld_register_func_for_add_image(added);
}
