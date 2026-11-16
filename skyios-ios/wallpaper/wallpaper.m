// Sets a wallpaper on the lock and home screen: the image file installed
// next to the tool (WALLPAPER_FILE, any format UIImage reads) or else a
// generated one (gradient + "SkyIOS"), through SpringBoard's wallpaper service (SpringBoardUIServices
// +[SBSUIWallpaperService setWallpaperImage:...], needs the entitlement
// com.apple.springboard.wallpaper-access). Run once at boot by sweetrun
// (xnu 0110) in place of another binary, which it execs afterwards.

#import <CoreText/CoreText.h>
#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <objc/message.h>
#include <unistd.h>

static void say(NSString *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    NSString *s = [[NSString alloc] initWithFormat:fmt arguments:ap];
    va_end(ap);
    int fd = open("/dev/console", O_WRONLY | O_NOCTTY);
    if (fd >= 0) {
        dprintf(fd, "skyios-wallpaper: %s\n", s.UTF8String);
        close(fd);
    }
}

// 1125x2436 (the iPhone X screen iOS draws): diagonal gradient + title
static UIImage *make_image(void) {
    size_t w = 1125, h = 2436;
    CGColorSpaceRef cs = CGColorSpaceCreateDeviceRGB();
    CGContextRef c = CGBitmapContextCreate(NULL, w, h, 8, 0, cs, kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little);
    CGFloat comps[] = {0.98, 0.42, 0.20, 1.0, 0.62, 0.16, 0.62, 1.0, 0.10, 0.12, 0.45, 1.0};
    CGFloat locs[] = {0.0, 0.5, 1.0};
    CGGradientRef g = CGGradientCreateWithColorComponents(cs, comps, locs, 3);
    CGContextDrawLinearGradient(c, g, CGPointMake(0, h), CGPointMake(w, 0), 0);
    CTFontRef font = CTFontCreateWithName(CFSTR("Helvetica-Bold"), 150, NULL);
    NSDictionary *attrs = @{
        (id)kCTFontAttributeName : (__bridge id)font,
        (id)kCTForegroundColorAttributeName : (__bridge id)[UIColor colorWithWhite:1 alpha:0.9].CGColor
    };
    NSAttributedString *text = [[NSAttributedString alloc] initWithString:@"SkyIOS" attributes:attrs];
    CTLineRef line = CTLineCreateWithAttributedString((__bridge CFAttributedStringRef)text);
    CGRect b = CTLineGetImageBounds(line, c);
    CGContextSetTextPosition(c, (w - b.size.width) / 2 - b.origin.x, h * 0.42);
    CTLineDraw(line, c);
    CGImageRef img = CGBitmapContextCreateImage(c);
    UIImage *ui = [UIImage imageWithCGImage:img scale:3 orientation:UIImageOrientationUp];
    CGImageRelease(img);
    CFRelease(line);
    CFRelease(font);
    CGGradientRelease(g);
    CGContextRelease(c);
    CGColorSpaceRelease(cs);
    return ui;
}

#define WALLPAPER_FILE "/private/var/root/skyios-wallpaper.img"

static void set_wallpaper(void) {
    if (!dlopen("/System/Library/PrivateFrameworks/SpringBoardUIServices.framework/SpringBoardUIServices", RTLD_NOW)) {
        say(@"dlopen SpringBoardUIServices: %s", dlerror());
        return;
    }
    // SpringBoardUIServices' own wrapper: default options, IOSurface + XPC
    // object, thumbnail and hash, then +[SBSUIWallpaperService setWallpaperImage:...]
    // (calling that method directly with UIImages traps in BSXPC, 0112c)
    void (*setimg)(UIImage *, NSInteger) = dlsym(RTLD_DEFAULT, "SBSUIWallpaperSetImageAsWallpaperForLocations");
    if (!setimg) {
        say(@"no SBSUIWallpaperSetImageAsWallpaperForLocations");
        return;
    }
    UIImage *img = [UIImage imageWithContentsOfFile:@WALLPAPER_FILE];
    if (img)
        img = [UIImage imageWithCGImage:img.CGImage scale:3 orientation:UIImageOrientationUp];
    else
        img = make_image();
    say(@"image %.0fx%.0f @%.0fx, asking SpringBoard", img.size.width, img.size.height, img.scale);
    // locations: 1 lock screen, 2 home screen, 3 both
    setimg(img, 3);
    say(@"SBSUIWallpaperSetImageAsWallpaperForLocations called");
}

int main(int argc, char **argv, char **envp, char **apple) {
    @autoreleasepool {
        unsigned delay = 90;
        say(@"waiting %u s for SpringBoard", delay);
        sleep(delay);
        set_wallpaper();
        // 0112d: the display went off right after the change; keep it on for
        // the sweetshot screenshots (SpringBoardServices SBSUndimScreen)
        dlopen("/System/Library/PrivateFrameworks/SpringBoardServices.framework/SpringBoardServices", RTLD_NOW);
        void (*undim)(void) = dlsym(RTLD_DEFAULT, "SBSUndimScreen");
        say(@"SBSUndimScreen %s", undim ? "found" : "missing");
        for (int i = 0; undim && i < 12; i++) {
            undim();
            sleep(15);
        }
    }
    for (char **a = apple; a && *a; a++)
        if (strncmp(*a, "executable_path=", 16) == 0)
            execve(*a + 16, argv, envp);
    return 0;
}
