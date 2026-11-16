// SkyAdrenoMetal internals shared by the device and the object classes.

#import <Foundation/Foundation.h>
#import <IOKit/IOKitLib.h>
#import <Metal/Metal.h>

// user client of SkyAdrenoAccel / SkyAdrenoMetalAccel (xnu 0109)
enum { SKY_INFO, SKY_ALLOC, SKY_FREE, SKY_SUBMIT, SKY_WAIT, SKY_LOG };

extern io_connect_t sky_conn;
void sky_log(const char *fmt, ...) __printflike(1, 2);

// one GPU allocation: kernel handle, GPU address, CPU mapping
typedef struct {
    uint64_t handle, iova, size;
    void *cpu;
} SkyMem;

BOOL sky_alloc(uint64_t size, SkyMem *m);
void sky_free(SkyMem *m);

// Base of every object class: selectors nobody wrote yet are logged once
// and answer 0/nil instead of raising.
@interface SkyObject : NSObject
@end

@interface SkyBuffer : SkyObject <MTLBuffer>
- (instancetype)initWithDevice:(id<MTLDevice>)device length:(NSUInteger)length options:(MTLResourceOptions)options;
@property (readonly) SkyMem mem;
@end

@interface SkyCommandQueue : SkyObject <MTLCommandQueue>
- (instancetype)initWithDevice:(id<MTLDevice>)device;
@end

// methods copied into the runtime-made _MTLDevice subclass
@interface SkyDeviceMethods : NSObject
@end
