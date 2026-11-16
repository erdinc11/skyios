// Puts a custom wallpaper on the iOS 16 lock and home screen through
// WallpaperKit + PosterBoard: installs the "SkyIOS" wallpaper collection
// (make-collection.sh; staged at STAGED by the boot-time installer, xnu
// 0110) into /Library/Wallpaper/Collections, makes it the default wallpaper
// (Collections~iphone.plist order, DefaultWallpapers~iphone.plist "default",
// originals kept as .sweet-old), then asks PosterBoard to delete its data
// store so it creates the default poster again. `posterwall undo` restores.
// Run once at boot by sweetrun; then execs the original binary.

#import <Foundation/Foundation.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <objc/message.h>
#include <objc/runtime.h>
#include <unistd.h>

#define STAGED @"/private/var/root/wp/C"
#define COLLECTIONS @"/Library/Wallpaper/Collections"
#define OURS COLLECTIONS @"/SkyIOS.wallpaperCollection"
#define INDEX COLLECTIONS @"/Collections~iphone.plist"
#define DEFAULTS @"/Library/Wallpaper/DefaultWallpapers~iphone.plist"
#define OUR_UUID @"5C1E5000-0000-4000-8000-000000009000"

static void say(NSString *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    NSString *s = [[NSString alloc] initWithFormat:fmt arguments:ap];
    va_end(ap);
    int fd = open("/dev/console", O_WRONLY | O_NOCTTY);
    if (fd >= 0) {
        dprintf(fd, "skyios-posterwall: %s\n", s.UTF8String);
        close(fd);
    }
}

static NSString *backup_of(NSString *p) {
    return [p stringByAppendingString:@".sweet-old"];
}

// edit a plist in its own format, keeping the first original as .sweet-old
static BOOL edit_plist(NSString *path, void (^edit)(NSMutableDictionary *)) {
    NSFileManager *fm = [NSFileManager defaultManager];
    if (![fm fileExistsAtPath:backup_of(path)] && ![fm copyItemAtPath:path toPath:backup_of(path) error:nil]) {
        say(@"backup of %@ failed", path);
        return NO;
    }
    NSData *d = [NSData dataWithContentsOfFile:backup_of(path)];
    NSPropertyListFormat fmt = NSPropertyListBinaryFormat_v1_0;
    NSMutableDictionary *p = [NSPropertyListSerialization propertyListWithData:d ?: [NSData data]
                                                                       options:NSPropertyListMutableContainersAndLeaves
                                                                        format:&fmt
                                                                         error:nil];
    if (![p isKindOfClass:[NSMutableDictionary class]]) {
        say(@"%@: not a dictionary", path);
        return NO;
    }
    edit(p);
    NSData *out = [NSPropertyListSerialization dataWithPropertyList:p format:fmt options:0 error:nil];
    BOOL ok = [out writeToFile:path atomically:YES];
    say(@"%@ written: %d", path, ok);
    return ok;
}

static void install(void) {
    NSFileManager *fm = [NSFileManager defaultManager];
    NSError *err = nil;
    [fm removeItemAtPath:OURS error:nil];
    if (![fm copyItemAtPath:STAGED toPath:OURS error:&err]) {
        say(@"copy collection: %@", err.localizedDescription);
        return;
    }
    say(@"collection installed at %@", OURS);
    edit_plist(INDEX, ^(NSMutableDictionary *p) {
        NSMutableArray *order = p[@"order"];
        [order removeObject:OUR_UUID];
        [order insertObject:OUR_UUID atIndex:0];
    });
    edit_plist(DEFAULTS, ^(NSMutableDictionary *p) {
        NSDictionary *ours = @{@"collectionIdentifier" : OUR_UUID, @"wallpaperIdentifier" : @9000};
        p[@"default"] = ours;
        // the phone says it is an iPhone X
        for (NSString *k in @[ @"iPhone10,3", @"iPhone10,6" ])
            [p removeObjectForKey:k];
    });
}

static void undo(void) {
    NSFileManager *fm = [NSFileManager defaultManager];
    [fm removeItemAtPath:OURS error:nil];
    for (NSString *p in @[ INDEX, DEFAULTS ])
        if ([fm fileExistsAtPath:backup_of(p)]) {
            [fm removeItemAtPath:p error:nil];
            [fm moveItemAtPath:backup_of(p) toPath:p error:nil];
        }
    say(@"undo done");
}

static void say_long(NSString *prefix, NSString *text) {
    // one console line per piece: object descriptions contain newlines
    NSArray *words = [text componentsSeparatedByCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
    text = [[words filteredArrayUsingPredicate:[NSPredicate predicateWithFormat:@"length > 0"]] componentsJoinedByString:@" "];
    for (NSUInteger off = 0; off < text.length; off += 130)
        say(@"%@%@", off ? @"  .." : prefix, [text substringWithRange:NSMakeRange(off, MIN(130, text.length - off))]);
}

// PRSService completions are (result object, NSError) (disassembly of
// -[PRSService fetchExtensionIdentifiersWithCompletion:]_block_invoke: blr
// with x1 = result, x2 = error, both retained objects)
typedef void (^objerr)(id, id);

static void call(id svc, NSString *selname, NSArray *args, NSString *label) {
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    objerr cb = ^(id a, id b) {
        say_long([label stringByAppendingString:@" -> "], [NSString stringWithFormat:@"%@ | %@", a ?: @"nil", b ?: @"no error"]);
        dispatch_semaphore_signal(done);
    };
    SEL sel = NSSelectorFromString(selname);
    if (args.count == 0)
        ((void (*)(id, SEL, id))objc_msgSend)(svc, sel, cb);
    else if (args.count == 1)
        ((void (*)(id, SEL, id, id))objc_msgSend)(svc, sel, args[0], cb);
    else
        ((void (*)(id, SEL, id, id, id))objc_msgSend)(svc, sel, args[0], args[1], cb);
    if (dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 30 * NSEC_PER_SEC)))
        say(@"%@: no answer in 30 s", label);
}

// probe: what PosterBoard has, then a poster from our collection
static void probe_posterboard(void) {
    if (!dlopen("/System/Library/PrivateFrameworks/PosterBoardServices.framework/PosterBoardServices", RTLD_NOW))
        return;
    id svc = [NSClassFromString(@"PRSService") new];
    call(svc, @"fetchExtensionIdentifiersWithCompletion:", @[], @"extensions");
    call(svc, @"fetchPosterDescriptorsForExtension:completion:", @[ @"com.apple.WallpaperKit.CollectionsPoster" ], @"collections descriptors");
    call(svc, @"fetchPosterConfigurations:", @[], @"configurations");
    call(svc, @"fetchSelectedConfiguration:", @[], @"selected");
}

// apply: CollectionsPoster descriptors refreshed, the one for our wallpaper
// (identifier containing "9000", else the first) made into a poster
// configuration, which is then selected
static id wait_obj(id svc, NSString *selname, NSArray *args, NSString *label) {
    __block id result = nil;
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    objerr cb = ^(id a, id b) {
        result = a;
        say_long([label stringByAppendingString:@" -> "], [NSString stringWithFormat:@"%@ | %@", a ?: @"nil", b ?: @"no error"]);
        dispatch_semaphore_signal(done);
    };
    SEL sel = NSSelectorFromString(selname);
    if (args.count == 0)
        ((void (*)(id, SEL, id))objc_msgSend)(svc, sel, cb);
    else if (args.count == 1)
        ((void (*)(id, SEL, id, id))objc_msgSend)(svc, sel, args[0], cb);
    else
        ((void (*)(id, SEL, id, id, id))objc_msgSend)(svc, sel, args[0], args[1], cb);
    if (dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 60 * NSEC_PER_SEC)))
        say(@"%@: no answer in 60 s", label);
    return result;
}

static void apply_poster(void) {
    if (!dlopen("/System/Library/PrivateFrameworks/PosterBoardServices.framework/PosterBoardServices", RTLD_NOW))
        return;
    id svc = [NSClassFromString(@"PRSService") new];
    NSString *ext = @"com.apple.WallpaperKit.CollectionsPoster";
    wait_obj(svc, @"refreshPosterDescriptorsForExtension:completion:", @[ ext ], @"refresh descriptors");
    id descs = wait_obj(svc, @"fetchPosterDescriptorsForExtension:completion:", @[ ext ], @"descriptors");
    NSString *pick = nil;
    for (id d in ([descs respondsToSelector:@selector(countByEnumeratingWithState:objects:count:)] ? descs : @[])) {
        NSString *ident = nil;
        for (NSString *k in @[ @"descriptorIdentifier", @"identifier", @"posterDescriptorIdentifier" ])
            if ([d respondsToSelector:NSSelectorFromString(k)]) {
                ident = ((id(*)(id, SEL))objc_msgSend)(d, NSSelectorFromString(k));
                break;
            }
        say(@"descriptor %@ identifier %@", d, ident);
        if ([ident isKindOfClass:[NSString class]] && (!pick || [ident containsString:@"9000"]))
            pick = ident;
    }
    if (!pick) {
        say(@"no descriptor to use");
        return;
    }
    say(@"using descriptor %@", pick);
    id config = wait_obj(svc, @"createPosterConfigurationForProviderIdentifier:posterDescriptorIdentifier:completion:",
                         @[ ext, pick ], @"create configuration");
    if (config)
        wait_obj(svc, @"updateToSelectedConfiguration:completion:", @[ config ], @"select configuration");
    wait_obj(svc, @"fetchSelectedConfiguration:", @[], @"selected now");
}

// legacy: set the image through SpringBoard's legacy wallpaper service and
// list /var/mobile/Library/SpringBoard (sizes) before and after, to see
// whether SpringBoard manages to write LockBackground.cpbitmap
static void list_sb(NSString *when) {
    NSString *dir = @"/private/var/mobile/Library/SpringBoard";
    NSMutableArray *parts = [NSMutableArray array];
    for (NSString *n in [[[NSFileManager defaultManager] contentsOfDirectoryAtPath:dir error:nil] sortedArrayUsingSelector:@selector(compare:)]) {
        NSDictionary *a = [[NSFileManager defaultManager] attributesOfItemAtPath:[dir stringByAppendingPathComponent:n] error:nil];
        [parts addObject:[NSString stringWithFormat:@"%@=%llu", n, [a fileSize]]];
    }
    say_long([NSString stringWithFormat:@"SpringBoard dir %@: ", when], [parts componentsJoinedByString:@" "]);
}

static void legacy_set(void) {
    list_sb(@"before");
    if (!dlopen("/System/Library/PrivateFrameworks/SpringBoardUIServices.framework/SpringBoardUIServices", RTLD_NOW))
        return;
    void (*setimg)(id, NSInteger) = dlsym(RTLD_DEFAULT, "SBSUIWallpaperSetImageAsWallpaperForLocations");
    Class uiimage = NSClassFromString(@"UIImage");
    id img = ((id(*)(id, SEL, id))objc_msgSend)(uiimage, NSSelectorFromString(@"imageWithContentsOfFile:"),
                                               @"/private/var/root/skyios-wallpaper.img");
    say(@"image %@, set function %p", img, setimg);
    if (setimg && img)
        setimg(img, 3);
    sleep(15);
    list_sb(@"after");
}

// PosterBoard creates the default poster when its data store is empty
static void reset_posterboard(void) {
    if (!dlopen("/System/Library/PrivateFrameworks/PosterBoardServices.framework/PosterBoardServices", RTLD_NOW)) {
        say(@"dlopen PosterBoardServices: %s", dlerror());
        return;
    }
    id svc = [NSClassFromString(@"PRSService") new];
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    // completion types not known: log the first two words raw
    void (^cb)(uintptr_t, uintptr_t) = ^(uintptr_t a, uintptr_t b) {
        say(@"deleteDataStore -> %#lx %#lx", (unsigned long)a, (unsigned long)b);
        dispatch_semaphore_signal(done);
    };
    ((void (*)(id, SEL, id))objc_msgSend)(svc, NSSelectorFromString(@"deleteDataStoreWithCompletion:"), cb);
    if (dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 30 * NSEC_PER_SEC)))
        say(@"deleteDataStore: no answer in 30 s");
}

int main(int argc, char **argv, char **envp, char **apple) {
    @autoreleasepool {
        if (argc > 1 && strcmp(argv[1], "undo") == 0) {
            undo();
        } else if (access("/private/var/root/wp/legacy", F_OK) == 0) {
            say(@"legacy mode: waiting 90 s for SpringBoard");
            sleep(90);
            legacy_set();
        } else if (access("/private/var/root/wp/apply", F_OK) == 0) {
            say(@"apply mode: waiting 90 s for PosterBoard");
            sleep(90);
            apply_poster();
        } else if (access("/private/var/root/wp/probe", F_OK) == 0) {
            say(@"probe mode: waiting 90 s for PosterBoard");
            sleep(90);
            probe_posterboard();
        } else {
            install();
            say(@"waiting 90 s for PosterBoard");
            sleep(90);
            reset_posterboard();
        }
    }
    for (char **a = apple; a && *a; a++)
        if (strncmp(*a, "executable_path=", 16) == 0)
            execve(*a + 16, argv, envp);
    return 0;
}
