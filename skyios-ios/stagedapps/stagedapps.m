// Moves the iOS 16 "staged system apps" (Calendar, Notes, Weather, Clock,
// Maps, Mail, ... in /private/var/staged_system_apps, normally installed by
// installd after a restore, which never happens on this port) to
// /Applications, so LaunchServices registers them as system apps after its
// store is rebuilt (boot-arg sweetlsreset=1). Same volume: renames only.
// Every move is appended to MOVED_LOG; `stagedapps undo` moves them back.
// Run once at boot by sweetrun (xnu 0110); it then execs the original binary.

#import <Foundation/Foundation.h>
#include <fcntl.h>
#include <unistd.h>

#define STAGED "/private/var/staged_system_apps"
#define MOVED_LOG "/private/var/root/skyios-stagedapps-moved.txt"

static void say(NSString *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    NSString *s = [[NSString alloc] initWithFormat:fmt arguments:ap];
    va_end(ap);
    int fd = open("/dev/console", O_WRONLY | O_NOCTTY);
    if (fd >= 0) {
        dprintf(fd, "skyios-stagedapps: %s\n", s.UTF8String);
        close(fd);
    }
}

static void move_all(void) {
    NSFileManager *fm = [NSFileManager defaultManager];
    NSError *err = nil;
    NSArray *names = [fm contentsOfDirectoryAtPath:@STAGED error:&err];
    if (!names) {
        say(@"cannot list %s: %@", STAGED, err.localizedDescription);
        return;
    }
    NSMutableString *log = [NSMutableString string];
    int moved = 0, skipped = 0, failed = 0;
    for (NSString *n in [names sortedArrayUsingSelector:@selector(compare:)]) {
        if (![n hasSuffix:@".app"])
            continue;
        NSString *from = [@STAGED stringByAppendingPathComponent:n];
        NSString *to = [@"/Applications" stringByAppendingPathComponent:n];
        if ([fm fileExistsAtPath:to]) {
            skipped++;
            continue;
        }
        if ([fm moveItemAtPath:from toPath:to error:&err]) {
            moved++;
            [log appendFormat:@"%@\n", n];
        } else {
            failed++;
            say(@"move %@: %@", n, err.localizedDescription);
        }
    }
    if (log.length) {
        NSFileHandle *h = [NSFileHandle fileHandleForWritingAtPath:@MOVED_LOG];
        if (!h) {
            [fm createFileAtPath:@MOVED_LOG contents:nil attributes:nil];
            h = [NSFileHandle fileHandleForWritingAtPath:@MOVED_LOG];
        }
        [h seekToEndOfFile];
        [h writeData:[log dataUsingEncoding:NSUTF8StringEncoding]];
        [h closeFile];
    }
    say(@"moved %d apps to /Applications (%d already there, %d failed); list in %s", moved, skipped, failed, MOVED_LOG);
}

static void undo(void) {
    NSFileManager *fm = [NSFileManager defaultManager];
    NSString *list = [NSString stringWithContentsOfFile:@MOVED_LOG encoding:NSUTF8StringEncoding error:nil];
    int back = 0;
    for (NSString *n in [list componentsSeparatedByString:@"\n"]) {
        if (n.length && [fm moveItemAtPath:[@"/Applications" stringByAppendingPathComponent:n]
                                    toPath:[@STAGED stringByAppendingPathComponent:n]
                                     error:nil])
            back++;
    }
    say(@"undo: moved %d apps back to %s", back, STAGED);
}

int main(int argc, char **argv, char **envp, char **apple) {
    @autoreleasepool {
        if (argc > 1 && strcmp(argv[1], "undo") == 0)
            undo();
        else
            move_all();
    }
    for (char **a = apple; a && *a; a++)
        if (strncmp(*a, "executable_path=", 16) == 0)
            execve(*a + 16, argv, envp);
    return 0;
}
