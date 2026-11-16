// SkyAdrenoMetal.bundle, stage 3 probe: Metal.framework loads this bundle
// for the IOAcceleratorES service SkyAdrenoMetalAccel (xnu 0109,
// sweetaccel=2) and calls -initWithAcceleratorPort: on SkyAdrenoMetalDevice,
// which must be a subclass of Metal's private _MTLDevice. The class is built
// at load time on top of _MTLDevice. Every selector it does not implement is
// logged once to the kernel log (user client selector 5) and answers 0/nil,
// so a boot shows which part of MTLDevice the system's clients use.

#import "SkyMetal.h"
#import <objc/message.h>
#import <objc/runtime.h>
#include <os/lock.h>
#include <stdarg.h>
#include <unistd.h>

static os_unfair_lock sky_lock = OS_UNFAIR_LOCK_INIT;

static id sky_zero(id self, SEL _cmd) {
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

// SkyMetalProcs (kernel, boot-arg sweetmetal_procs): comma list of process
// names that get this device, "*" for all
static BOOL sky_allowed(io_service_t port) {
    CFTypeRef v = IORegistryEntryCreateCFProperty(port, CFSTR("SkyMetalProcs"), kCFAllocatorDefault, 0);
    NSString *list = (__bridge_transfer NSString *)v;
    if (![list isKindOfClass:[NSString class]])
        return NO;
    NSString *me = [[NSProcessInfo processInfo] processName];
    for (NSString *p in [list componentsSeparatedByString:@","])
        if ([p isEqualToString:@"*"] || [p isEqualToString:me])
            return YES;
    return NO;
}

static id sky_init(id self, SEL _cmd, io_service_t port) {
    if (!sky_allowed(port))
        return nil;
    if (!sky_conn)
        IOServiceOpen(port, mach_task_self(), 0, &sky_conn);
    struct objc_super sup = {self, class_getSuperclass(object_getClass(self))};
    self = ((id(*)(struct objc_super *, SEL))objc_msgSendSuper)(&sup, @selector(init));
    sky_log("initWithAcceleratorPort: %#x from pid %d -> %p", port, getpid(), self);
    return self;
}

// Constant answers. The A11 values are what AGXMetalA11's AGXA11FamilyDevice
// returns (20H392 cache, methods that are just `mov w0, #N; ret`), used as
// placeholders until the Adreno's real limits are filled in; featureProfile
// 5 is A11's (it selects _MTLDevice initLimits' built-in limit set).
static const struct {
    const char *sel;
    uint64_t value;
} sky_consts[] = {
    {"maxThreadgroupMemoryLength", 0x8000},
    {"requiresRaytracingEmulation", 0x1},
    {"deviceLinearReadOnlyTextureAlignmentBytes", 0x10},
    {"supportsMemorylessRenderTargets", 0x1},
    {"indirectArgumentBufferCapabilities", 0x1},
    {"supportsVertexAmplification", 0},
    {"isRTZRoundingSupported", 0},
    {"featureProfile", 0x5},
    {"isFloat32FilteringSupported", 0},
    {"cmdBufArgsSize", 0x38},
    {"supportsFragmentOnlyEncoders", 0x1},
    {"supportsBufferlessClientStorageTexture", 0},
    {"supports32bpcMSAATextures", 0},
    {"supportsTexture2DMultisampleArray", 0},
    {"supportsBlackOrWhiteSamplerBorderColors", 0},
    {"supportsMirrorClampToEdgeSamplerMode", 0},
    {"supportsCustomBorderColor", 0},
    {"supportsSamplerAddressModeClampToHalfBorder", 0},
    {"supportsBCTextureCompression", 0},
    {"supportsShaderBarycentricCoordinates", 0},
    {"supportsQueryTextureLOD", 0},
    {"supportsBufferBoundsChecking", 0},
    {"supportsLargeFramebufferConfigs", 0},
    {"isMsaa32bSupported", 0},
    {"isRGB10A2GammaSupported", 0},
    {"isASTCPixelFormatsSupported", 0x1},
    {"isMagicMipmapSupported", 0},
    {"supportsShaderMinLODClamp", 0},
    {"isTileSizeIndependentVaryingsSupported", 0},
    {"supportsAtomicUlongVoidMinMax", 0},
    {"supportsAtomicFloat", 0},
    {"supportsDeadlineProfile", 0x1},
    {"supportsComputeCompressedTextureWrite", 0},
    {"maxVertexTextures", 0x60},
    {"maxFragmentTextures", 0x60},
    {"maxComputeTextures", 0x60},
    {"maxIndirectTextures", 0x60},
    {"maxIndirectBuffers", 0x60},
    {"maxVisibilityQueryOffset", 0xfff8},
    {"supportsPerPlaneCompression", 0},
    {"isAnisoSampleFixSupported", 0},
    {"supportsDynamicLibraries", 0},
    {"supportsTLS", 0},
    {"supportsLateEvalEvent", 0x1},
    {"supportsStreamingCodecSignaling", 0},
    {"supportsGlobalVariableRelocation", 0},
    {"supportsGlobalVariableRelocationRender", 0},
    {"supportsMeshShaders", 0},
    {"isFixedLinePointFillDepthGradientSupported", 0},
    {"supportsBGR10A2", 0},
    {"supportsExplicitVisibilityGroups", 0},
    {"singleFPConfig", 0x54},
    {"doubleFPConfig", 0},
    {"deviceLinearTextureAlignmentBytes", 0x10},
    {"areProgrammableSamplePositionsSupported", 0x1},
    {"maxRasterizationRateLayerCount", 0},
    {"maximumComputeSubstreams", 0},
    {"buildBVHForRIA", 0},
    {"maxColorAttachments", 8},
    {"maxIndirectSamplers", 16},
    {"maxIndirectSamplersPerDevice", 2048},
    {"maxBufferLength", 0x10000000},
    {"maxTextureWidth2D", 16384},
    {"maxTextureHeight2D", 16384},
    {"argumentBuffersSupport", 0},
    {"registryID", 0x5ce0618},
    {"isLowPower", 0},
    {"isHeadless", 0},
    {"isRemovable", 0},
    {"hasUnifiedMemory", 1},
    {"recommendedMaxWorkingSetSize", 0x40000000},
};

static uint64_t sky_const(id self, SEL _cmd) {
    const char *name = sel_getName(_cmd);
    for (size_t i = 0; i < sizeof(sky_consts) / sizeof(sky_consts[0]); i++)
        if (strcmp(sky_consts[i].sel, name) == 0)
            return sky_consts[i].value;
    return 0;
}


__attribute__((constructor)) static void sky_register(void) {
    sky_record_init();
    Class base = NSClassFromString(@"_MTLDevice");
    if (!base || objc_getClass("SkyAdrenoMetalDevice"))
        return;
    Class cls = objc_allocateClassPair(base, "SkyAdrenoMetalDevice", 0);
    class_addMethod(cls, sel_registerName("initWithAcceleratorPort:"), (IMP)sky_init, "@@:I");
    // the Objective-C implementations of SkyDeviceMethods (SkyObjects.m)
    unsigned int n = 0;
    Method *ms = class_copyMethodList([SkyDeviceMethods class], &n);
    for (unsigned int i = 0; i < n; i++)
        class_addMethod(cls, method_getName(ms[i]), method_getImplementation(ms[i]), method_getTypeEncoding(ms[i]));
    free(ms);
    for (size_t i = 0; i < sizeof(sky_consts) / sizeof(sky_consts[0]); i++)
        class_addMethod(cls, sel_registerName(sky_consts[i].sel), (IMP)sky_const, "Q@:");
    // _MTLAddDevice asserts both
    static const char *protos[] = {"MTLDevice", "MTLDeviceSPI"};
    for (size_t i = 0; i < 2; i++) {
        Protocol *p = objc_getProtocol(protos[i]);
        if (p)
            class_addProtocol(cls, p);
    }
    class_addMethod(object_getClass((id)cls), @selector(resolveInstanceMethod:), (IMP)sky_resolve, "c@::");
    objc_registerClassPair(cls);
}
