// SkyAdrenoMetal.bundle, stage 3 probe: Metal.framework loads this bundle
// for the IOAcceleratorES service SkyAdrenoMetalAccel (xnu 0109,
// sweetaccel=2) and calls -initWithAcceleratorPort: on SkyAdrenoMetalDevice,
// which must be a subclass of Metal's private _MTLDevice. The class is built
// at load time on top of _MTLDevice. Every selector it does not implement is
// logged once to the kernel log (user client selector 5) and answers 0/nil,
// so a boot shows which part of MTLDevice the system's clients use.

#import <Foundation/Foundation.h>
#import <IOKit/IOKitLib.h>
#import <objc/message.h>
#import <objc/runtime.h>
#include <os/lock.h>
#include <stdarg.h>
#include <unistd.h>

static io_connect_t sky_conn;
static os_unfair_lock sky_lock = OS_UNFAIR_LOCK_INIT;

static void sky_log(const char *fmt, ...) {
    char text[160];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(text, sizeof(text), fmt, ap);
    va_end(ap);
    if (sky_conn)
        IOConnectCallMethod(sky_conn, 5, NULL, 0, text, strlen(text), NULL, NULL, NULL, NULL);
}

static id sky_zero(id self, SEL _cmd, ...) {
    return nil;
}

static BOOL sky_resolve(id cls, SEL _cmd, SEL sel) {
    static NSMutableSet *seen;
    os_unfair_lock_lock(&sky_lock);
    if (!seen)
        seen = [NSMutableSet new];
    NSString *name = NSStringFromSelector(sel);
    BOOL first = ![seen containsObject:name];
    [seen addObject:name];
    os_unfair_lock_unlock(&sky_lock);
    if (first)
        sky_log("missing -%s", sel_getName(sel));
    // a fake method: right for selectors returning an object, integer or
    // nothing, wrong (but logged) for struct returns
    class_addMethod((Class)cls, sel, (IMP)sky_zero, "@@:");
    return YES;
}

static id sky_init(id self, SEL _cmd, io_service_t port) {
    if (!sky_conn)
        IOServiceOpen(port, mach_task_self(), 0, &sky_conn);
    char name[64] = "";
    proc_name_self(name, sizeof(name));
    struct objc_super sup = {self, class_getSuperclass(object_getClass(self))};
    self = ((id(*)(struct objc_super *, SEL))objc_msgSendSuper)(&sup, @selector(init));
    sky_log("initWithAcceleratorPort: %#x from pid %d -> %p", port, getpid(), self);
    return self;
}

static id sky_name(id self, SEL _cmd) {
    return @"Adreno 618 (SkyIOS)";
}

__attribute__((constructor)) static void sky_register(void) {
    Class base = NSClassFromString(@"_MTLDevice");
    if (!base || objc_getClass("SkyAdrenoMetalDevice"))
        return;
    Class cls = objc_allocateClassPair(base, "SkyAdrenoMetalDevice", 0);
    class_addMethod(cls, sel_registerName("initWithAcceleratorPort:"), (IMP)sky_init, "@@:I");
    class_addMethod(cls, @selector(name), (IMP)sky_name, "@@:");
    class_addMethod(object_getClass((id)cls), @selector(resolveInstanceMethod:), (IMP)sky_resolve, "c@::");
    objc_registerClassPair(cls);
}
