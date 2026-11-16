// Recovery after 0112s (SpringBoard crash-looping in the legacy poster
// snapshot, -[PBUIImageEncoder saveUIImage:] on a nil image): lists
// /var/mobile/Library/SpringBoard and moves the legacy wallpaper files
// (names containing Background, Wallpaper or .cpbitmap) into BACKUP/<time>, so
// SpringBoard stops taking the legacy-wallpaper path. Every move is logged.
// Run once at boot by sweetrun; then execs the original binary.

#import <Foundation/Foundation.h>
#include <fcntl.h>
#include <unistd.h>

#define SB @"/private/var/mobile/Library/SpringBoard"
#define BACKUP @"/private/var/mobile/Library/SpringBoard.sweet-legacy-wallpaper"

static void say(NSString *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    NSString *s = [[NSString alloc] initWithFormat:fmt arguments:ap];
    va_end(ap);
    int fd = open("/dev/console", O_WRONLY | O_NOCTTY);
    if (fd >= 0) {
        dprintf(fd, "skyios-sbfix: %s\n", s.UTF8String);
        close(fd);
    }
}

int main(int argc, char **argv, char **envp, char **apple) {
    @autoreleasepool {
        NSFileManager *fm = [NSFileManager defaultManager];
        NSArray *names = [[fm contentsOfDirectoryAtPath:SB error:nil] sortedArrayUsingSelector:@selector(compare:)];
        say(@"%@: %@", SB, [names componentsJoinedByString:@" "]);
        // one subdirectory per run (0113a: names already taken by the 0112t run)
        NSString *dest = [BACKUP stringByAppendingPathComponent:[NSString stringWithFormat:@"%lld", (long long)time(NULL)]];
        [fm createDirectoryAtPath:dest withIntermediateDirectories:YES attributes:nil error:nil];
        for (NSString *n in names) {
            if ([n rangeOfString:@"Background"].location == NSNotFound && [n rangeOfString:@"Wallpaper"].location == NSNotFound &&
                ![n hasSuffix:@".cpbitmap"])
                continue;
            NSError *err = nil;
            BOOL ok = [fm moveItemAtPath:[SB stringByAppendingPathComponent:n] toPath:[dest stringByAppendingPathComponent:n] error:&err];
            say(@"moved %@ -> %@: %d %@", n, dest, ok, err ? err.localizedDescription : @"");
        }
    }
    for (char **a = apple; a && *a; a++)
        if (strncmp(*a, "executable_path=", 16) == 0)
            execve(*a + 16, argv, envp);
    return 0;
}
